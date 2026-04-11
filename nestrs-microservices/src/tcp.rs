use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

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
    details: Option<serde_json::Value>,
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

#[async_trait]
impl Transport for TcpTransport {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        let id = self.next_id();
        let req = MicroserviceRequest {
            id: id.clone(),
            kind: PacketKind::Send,
            pattern: pattern.to_string(),
            payload,
        };

        let mut stream = TcpStream::connect(self.options.addr).await.map_err(|e| {
            TransportError::new(format!("tcp transport connect failed: {e}"))
        })?;

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

        let mut reader = BufReader::new(stream);
        let mut resp_line = String::new();
        let n = reader
            .read_line(&mut resp_line)
            .await
            .map_err(|e| TransportError::new(format!("read response failed: {e}")))?;
        if n == 0 {
            return Err(TransportError::new("tcp transport: empty response"));
        }
        let resp: MicroserviceResponse = serde_json::from_str(resp_line.trim_end_matches('\n'))
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
    }

    async fn emit_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<(), TransportError> {
        let id = self.next_id();
        let req = MicroserviceRequest {
            id,
            kind: PacketKind::Emit,
            pattern: pattern.to_string(),
            payload,
        };

        let mut stream = TcpStream::connect(self.options.addr).await.map_err(|e| {
            TransportError::new(format!("tcp transport connect failed: {e}"))
        })?;

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
    pub fn new(options: TcpMicroserviceOptions, handlers: Vec<Arc<dyn MicroserviceHandler>>) -> Self {
        Self { options, handlers }
    }

    pub async fn listen(self) -> Result<(), TransportError> {
        self.listen_with_shutdown(std::future::pending::<()>()).await
    }

    pub async fn listen_with_shutdown<F>(self, shutdown: F) -> Result<(), TransportError>
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind(self.options.addr)
            .await
            .map_err(|e| TransportError::new(format!("tcp microservice bind failed: {e}")))?;

        let handlers = Arc::new(self.handlers);

        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                _ = &mut shutdown => {
                    break;
                }
                accepted = listener.accept() => {
                    let (stream, _peer) = accepted
                        .map_err(|e| TransportError::new(format!("tcp microservice accept failed: {e}")))?;
                    let handlers = handlers.clone();
                    tokio::spawn(async move {
                        serve_connection(stream, handlers).await;
                    });
                }
            }
        }

        Ok(())
    }
}

async fn serve_connection(stream: TcpStream, handlers: Arc<Vec<Arc<dyn MicroserviceHandler>>>) {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let req: MicroserviceRequest = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                // best-effort error frame for malformed payloads
                let _ = write_half
                    .write_all(
                        br#"{"id":"0","ok":false,"error":{"message":"invalid request"}}"#,
                    )
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

