use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Cap on a single newline-delimited JSON frame (request or response) on
/// both ends. A peer that streams bytes without ever sending a newline can
/// no longer grow memory without bound: past the cap the frame is rejected
/// and the connection dropped. 1 MiB is generous for microservice RPC
/// payloads.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Server-side idle timeout between frames on one connection. A silent or
/// half-open connection is dropped once it exceeds this, releasing its
/// task and buffered bytes.
const CONNECTION_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Whole-RPC budget for the client transport (connect + write + read
/// response): a server that accepts and never responds fails the call
/// instead of hanging the caller.
const CLIENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Concurrent in-flight server connections. Past the cap new connections
/// are closed immediately (logged) rather than spawning unbounded tasks —
/// the OS backlog absorbs the remainder.
const MAX_CONNECTIONS: usize = 1024;

#[derive(Clone, Debug)]
pub struct TcpTransportOptions {
    pub addr: SocketAddr,
}

impl TcpTransportOptions {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

/// Simple JSON-over-TCP transport (NestJS `Transport.TCP` analogue).
///
/// Wire format: newline-delimited JSON.
#[derive(Clone)]
pub struct TcpTransport {
    options: TcpTransportOptions,
    seq: Arc<AtomicU64>,
}

impl TcpTransport {
    pub fn new(options: TcpTransportOptions) -> Self {
        Self {
            options,
            seq: Arc::new(AtomicU64::new(1)),
        }
    }

    fn next_id(&self) -> String {
        self.seq.fetch_add(1, Ordering::Relaxed).to_string()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PacketKind {
    Send,
    Emit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MicroserviceRequest {
    id: String,
    kind: PacketKind,
    pattern: String,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MicroserviceErrorPayload {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    // Boxed to mirror `TransportError::details` (serializes identically).
    details: Option<Box<serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MicroserviceResponse {
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<MicroserviceErrorPayload>,
}

/// Reads one newline-terminated frame, **bounded** by `cap` bytes. Unlike
/// `AsyncBufReadExt::read_line` (which grows the buffer without limit), a
/// frame larger than `cap` aborts with an error — the caller drops the
/// connection. Bytes past the newline stay in the reader's buffer, so
/// pipelined frames survive. `Ok(None)` is a clean EOF.
async fn read_capped_line<R>(reader: &mut R, cap: usize) -> Result<Option<String>, String>
where
    R: tokio::io::AsyncBufRead + Unpin,
{
    let mut line: Vec<u8> = Vec::with_capacity(256);
    loop {
        let available = match reader.fill_buf().await {
            Ok(a) => a,
            Err(e) => return Err(format!("read failed: {e}")),
        };
        if available.is_empty() {
            // EOF: a partial frame without its terminator is malformed.
            return if line.is_empty() {
                Ok(None)
            } else {
                Err("connection closed mid-frame".to_string())
            };
        }
        match available.iter().position(|&b| b == b'\n') {
            Some(pos) => {
                line.extend_from_slice(&available[..pos]);
                let consumed = pos + 1;
                reader.consume(consumed);
                if line.len() > cap {
                    return Err(format!("frame exceeds {cap} byte limit"));
                }
                return String::from_utf8(line)
                    .map(Some)
                    .map_err(|_| "frame is not valid UTF-8".to_string());
            }
            None => {
                let len = available.len();
                line.extend_from_slice(available);
                reader.consume(len);
                if line.len() > cap {
                    return Err(format!("frame exceeds {cap} byte limit"));
                }
            }
        }
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        // The whole RPC (connect + write + read) is bounded: a server that
        // accepts and never responds fails the call instead of hanging the
        // caller.
        tokio::time::timeout(CLIENT_REQUEST_TIMEOUT, async {
            let id = self.next_id();
            let req = MicroserviceRequest {
                id: id.clone(),
                kind: PacketKind::Send,
                pattern: pattern.to_string(),
                payload,
            };

            let mut stream = TcpStream::connect(self.options.addr)
                .await
                .map_err(|e| TransportError::new(format!("tcp transport connect failed: {e}")))?;

            let line = serde_json::to_string(&req)
                .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;
            stream
                .write_all(line.as_bytes())
                .await
                .map_err(|e| TransportError::new(format!("write request failed: {e}")))?;
            stream
                .write_all(b"\n")
                .await
                .map_err(|e| TransportError::new(format!("write request newline failed: {e}")))?;
            stream
                .flush()
                .await
                .map_err(|e| TransportError::new(format!("flush request failed: {e}")))?;

            // Bounded read: a hostile server cannot stream an unbounded
            // "response" line either.
            let mut reader = BufReader::new(stream);
            let resp_line = match read_capped_line(&mut reader, MAX_FRAME_BYTES).await {
                Ok(Some(l)) => l,
                Ok(None) => return Err(TransportError::new("tcp transport: empty response")),
                Err(reason) => return Err(TransportError::new(format!("read response: {reason}"))),
            };
            let resp: MicroserviceResponse = serde_json::from_str(&resp_line)
                .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))?;
            if resp.id != id {
                return Err(TransportError::new("tcp transport: response id mismatch"));
            }
            if resp.ok {
                Ok(resp.payload.unwrap_or(serde_json::Value::Null))
            } else {
                let mut err = TransportError::new(
                    resp.error
                        .as_ref()
                        .map(|e| e.message.as_str())
                        .unwrap_or("microservice error"),
                );
                if let Some(details) = resp.error.and_then(|e| e.details) {
                    err = err.with_details(details);
                }
                Err(err)
            }
        })
        .await
        .map_err(|_| {
            TransportError::new(format!(
                "tcp transport: request timed out after {CLIENT_REQUEST_TIMEOUT:?} \
                 (connect + write + read)"
            ))
        })?
    }

    async fn emit_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<(), TransportError> {
        tokio::time::timeout(CLIENT_REQUEST_TIMEOUT, async {
            let id = self.next_id();
            let req = MicroserviceRequest {
                id,
                kind: PacketKind::Emit,
                pattern: pattern.to_string(),
                payload,
            };

            let mut stream = TcpStream::connect(self.options.addr)
                .await
                .map_err(|e| TransportError::new(format!("tcp transport connect failed: {e}")))?;

            let line = serde_json::to_string(&req)
                .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;
            stream
                .write_all(line.as_bytes())
                .await
                .map_err(|e| TransportError::new(format!("write event failed: {e}")))?;
            stream
                .write_all(b"\n")
                .await
                .map_err(|e| TransportError::new(format!("write event newline failed: {e}")))?;
            stream
                .flush()
                .await
                .map_err(|e| TransportError::new(format!("flush event failed: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|_| {
            TransportError::new(format!(
                "tcp transport: emit timed out after {CLIENT_REQUEST_TIMEOUT:?}"
            ))
        })?
    }
}

#[derive(Clone, Debug)]
pub struct TcpMicroserviceOptions {
    pub addr: SocketAddr,
}

impl TcpMicroserviceOptions {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

pub struct TcpMicroserviceServer {
    options: TcpMicroserviceOptions,
    handlers: Vec<Arc<dyn MicroserviceHandler>>,
}

impl TcpMicroserviceServer {
    pub fn new(
        options: TcpMicroserviceOptions,
        handlers: Vec<Arc<dyn MicroserviceHandler>>,
    ) -> Self {
        Self { options, handlers }
    }

    pub async fn listen(self) -> Result<(), TransportError> {
        self.listen_with_shutdown(std::future::pending::<()>())
            .await
    }

    pub async fn listen_with_shutdown<F>(self, shutdown: F) -> Result<(), TransportError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind(self.options.addr)
            .await
            .map_err(|e| TransportError::new(format!("tcp microservice bind failed: {e}")))?;

        let handlers = Arc::new(self.handlers);
        let conn_slots = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));

        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    break;
                }
                accepted = listener.accept() => {
                    let (stream, _peer) = accepted
                        .map_err(|e| TransportError::new(format!("tcp microservice accept failed: {e}")))?;
                    // Fail-fast cap: never spawn unbounded connection tasks.
                    // Past the cap the new connection is closed immediately —
                    // the OS backlog absorbs the remainder.
                    match conn_slots.clone().try_acquire_owned() {
                        Ok(permit) => {
                            let handlers = handlers.clone();
                            tokio::spawn(async move {
                                let _permit = permit;
                                serve_connection(stream, handlers).await;
                            });
                        }
                        Err(_) => {
                            tracing::warn!(target: "nestrs_microservices",
                                "tcp microservice: {MAX_CONNECTIONS} concurrent connection cap reached; dropping new connection");
                            // Dropping `stream` closes it.
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

async fn serve_connection(stream: TcpStream, handlers: Arc<Vec<Arc<dyn MicroserviceHandler>>>) {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    loop {
        let frame = match tokio::time::timeout(
            CONNECTION_IDLE_TIMEOUT,
            read_capped_line(&mut reader, MAX_FRAME_BYTES),
        )
        .await
        {
            // Idle or half-open connection: drop it, releasing its task.
            Err(_elapsed) => return,
            Ok(Err(reason)) => {
                // Malformed or oversized frame: reply with a generic error
                // frame (never echoing attacker bytes) and drop the
                // connection.
                let _ = write_half
                    .write_all(br#"{"id":"0","ok":false,"error":{"message":"frame rejected"}}"#)
                    .await;
                let _ = write_half.write_all(b"\n").await;
                tracing::warn!(target: "nestrs_microservices",
                    "tcp microservice: dropping connection: {reason}");
                return;
            }
            Ok(Ok(None)) => return, // clean EOF
            Ok(Ok(Some(frame))) => frame,
        };

        let req: MicroserviceRequest = match serde_json::from_str(&frame) {
            Ok(v) => v,
            Err(_) => {
                // best-effort error frame for malformed payloads
                let _ = write_half
                    .write_all(br#"{"id":"0","ok":false,"error":{"message":"invalid request"}}"#)
                    .await;
                let _ = write_half.write_all(b"\n").await;
                continue;
            }
        };

        match req.kind {
            PacketKind::Send => {
                let res = dispatch_send(&handlers, &req.pattern, req.payload).await;
                let wire = match res {
                    Ok(payload) => MicroserviceResponse {
                        id: req.id,
                        ok: true,
                        payload: Some(payload),
                        error: None,
                    },
                    Err(e) => MicroserviceResponse {
                        id: req.id,
                        ok: false,
                        payload: None,
                        error: Some(MicroserviceErrorPayload {
                            message: e.message,
                            details: e.details,
                        }),
                    },
                };

                if let Ok(text) = serde_json::to_string(&wire) {
                    let _ = write_half.write_all(text.as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                }
            }
            PacketKind::Emit => {
                dispatch_emit(&handlers, &req.pattern, req.payload).await;
            }
        }
    }
}

async fn dispatch_send(
    handlers: &[Arc<dyn MicroserviceHandler>],
    pattern: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, TransportError> {
    for h in handlers {
        if let Some(res) = h.handle_message(pattern, payload.clone()).await {
            return res;
        }
    }
    Err(TransportError::new(format!(
        "no microservice handler for pattern `{pattern}`"
    )))
}

async fn dispatch_emit(
    handlers: &[Arc<dyn MicroserviceHandler>],
    pattern: &str,
    payload: serde_json::Value,
) {
    for h in handlers {
        let _ = h.handle_event(pattern, payload.clone()).await;
    }
}

#[async_trait]
impl crate::MicroserviceServer for TcpMicroserviceServer {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: crate::ShutdownFuture,
    ) -> Result<(), TransportError> {
        (*self).listen_with_shutdown(shutdown).await
    }
}
