//! gRPC transport using **tonic** with JSON payloads. Request/response bodies use the same
//! [`crate::wire`] shapes as Redis/Kafka (serialized to bytes inside protobuf fields).
//!
//! ## Ergonomics
//!
//! - **Client:** [`GrpcTransportOptions::new`] + [`GrpcTransportOptions::with_request_timeout`].
//! - **Server:** [`GrpcMicroserviceOptions::bind`] / [`GrpcMicroserviceOptions::new`] with
//!   [`NestFactory::create_microservice_grpc`](https://docs.rs/nestrs/latest/nestrs/struct.NestFactory.html#method.create_microservice_grpc)
//!   on the umbrella crate (feature **`microservices-grpc`**).

use crate::{MicroserviceHandler, Transport, TransportError};
use async_trait::async_trait;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::OnceCell;

pub mod proto {
    tonic::include_proto!("nestrs.microservices");
}

#[derive(Clone, Debug)]
pub struct GrpcTransportOptions {
    /// Example: `http://127.0.0.1:50051`
    pub endpoint: String,
    pub request_timeout: std::time::Duration,
}

impl GrpcTransportOptions {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            request_timeout: std::time::Duration::from_secs(5),
        }
    }

    /// Overrides the default **5s** timeout used for `send` / `emit` RPCs.
    pub fn with_request_timeout(mut self, request_timeout: std::time::Duration) -> Self {
        self.request_timeout = request_timeout;
        self
    }
}

#[derive(Clone)]
pub struct GrpcTransport {
    options: GrpcTransportOptions,
    channel: OnceCell<tonic::transport::Channel>,
}

impl GrpcTransport {
    pub fn new(options: GrpcTransportOptions) -> Self {
        Self {
            options,
            channel: OnceCell::new(),
        }
    }

    async fn channel(&self) -> Result<tonic::transport::Channel, TransportError> {
        let ch = self
            .channel
            .get_or_try_init(|| async {
                tonic::transport::Channel::from_shared(self.options.endpoint.clone())
                    .map_err(|e| TransportError::new(format!("grpc endpoint invalid: {e}")))?
                    .connect()
                    .await
                    .map_err(|e| TransportError::new(format!("grpc connect failed: {e}")))
            })
            .await?;
        Ok(ch.clone())
    }
}

#[async_trait]
impl Transport for GrpcTransport {
    async fn send_json(&self, pattern: &str, payload: Value) -> Result<Value, TransportError> {
        let channel = self.channel().await?;
        let mut client = proto::microservice_client::MicroserviceClient::new(channel);

        let payload_json = serde_json::to_vec(&payload)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        let req = proto::SendRequest {
            pattern: pattern.to_string(),
            payload_json,
        };

        let resp = tokio::time::timeout(self.options.request_timeout, client.send(req))
            .await
            .map_err(|_| TransportError::new("grpc request timed out"))?
            .map_err(|e| TransportError::new(format!("grpc send failed: {e}")))?
            .into_inner();

        if resp.ok {
            if resp.payload_json.is_empty() {
                return Ok(Value::Null);
            }
            serde_json::from_slice(&resp.payload_json)
                .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))
        } else {
            let mut err = TransportError::new(resp.error_message);
            if resp.has_error_details {
                if let Ok(details) = serde_json::from_slice::<Value>(&resp.error_details_json) {
                    err = err.with_details(details);
                }
            }
            Err(err)
        }
    }

    async fn emit_json(&self, pattern: &str, payload: Value) -> Result<(), TransportError> {
        let channel = self.channel().await?;
        let mut client = proto::microservice_client::MicroserviceClient::new(channel);

        let payload_json = serde_json::to_vec(&payload)
            .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;

        let req = proto::EmitRequest {
            pattern: pattern.to_string(),
            payload_json,
        };

        let resp = tokio::time::timeout(self.options.request_timeout, client.emit(req))
            .await
            .map_err(|_| TransportError::new("grpc emit timed out"))?
            .map_err(|e| TransportError::new(format!("grpc emit failed: {e}")))?
            .into_inner();

        if resp.ok {
            Ok(())
        } else {
            let mut err = TransportError::new(resp.error_message);
            if resp.has_error_details {
                if let Ok(details) = serde_json::from_slice::<Value>(&resp.error_details_json) {
                    err = err.with_details(details);
                }
            }
            Err(err)
        }
    }
}

#[derive(Clone, Debug)]
pub struct GrpcMicroserviceOptions {
    pub addr: SocketAddr,
}

impl GrpcMicroserviceOptions {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }

    /// Binds the gRPC microservice server; same as [`Self::new`], useful in fluent call chains.
    pub fn bind(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

pub struct GrpcMicroserviceServer {
    options: GrpcMicroserviceOptions,
    handlers: Vec<Arc<dyn MicroserviceHandler>>,
}

impl GrpcMicroserviceServer {
    pub fn new(
        options: GrpcMicroserviceOptions,
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
        let svc = ServiceImpl {
            handlers: Arc::new(self.handlers),
        };

        tonic::transport::Server::builder()
            .add_service(proto::microservice_server::MicroserviceServer::new(svc))
            .serve_with_shutdown(self.options.addr, shutdown)
            .await
            .map_err(|e| TransportError::new(format!("grpc serve failed: {e}")))?;

        Ok(())
    }
}

#[derive(Clone)]
struct ServiceImpl {
    handlers: Arc<Vec<Arc<dyn MicroserviceHandler>>>,
}

#[tonic::async_trait]
impl proto::microservice_server::Microservice for ServiceImpl {
    async fn send(
        &self,
        request: tonic::Request<proto::SendRequest>,
    ) -> Result<tonic::Response<proto::SendResponse>, tonic::Status> {
        let req = request.into_inner();
        let payload: Value = serde_json::from_slice(&req.payload_json)
            .map_err(|_| tonic::Status::invalid_argument("invalid json payload"))?;

        let res = crate::wire::dispatch_send(&self.handlers[..], &req.pattern, payload).await;
        let out = match res {
            Ok(payload) => proto::SendResponse {
                ok: true,
                payload_json: serde_json::to_vec(&payload).unwrap_or_default(),
                error_message: String::new(),
                error_details_json: Vec::new(),
                has_error_details: false,
            },
            Err(e) => proto::SendResponse {
                ok: false,
                payload_json: Vec::new(),
                error_message: e.message,
                error_details_json: e
                    .details
                    .as_ref()
                    .and_then(|v| serde_json::to_vec(v).ok())
                    .unwrap_or_default(),
                has_error_details: e.details.is_some(),
            },
        };

        Ok(tonic::Response::new(out))
    }

    async fn emit(
        &self,
        request: tonic::Request<proto::EmitRequest>,
    ) -> Result<tonic::Response<proto::EmitResponse>, tonic::Status> {
        let req = request.into_inner();
        let payload: Value = serde_json::from_slice(&req.payload_json)
            .map_err(|_| tonic::Status::invalid_argument("invalid json payload"))?;

        crate::wire::dispatch_emit(&self.handlers[..], &req.pattern, payload).await;

        Ok(tonic::Response::new(proto::EmitResponse {
            ok: true,
            error_message: String::new(),
            error_details_json: Vec::new(),
            has_error_details: false,
        }))
    }
}

#[async_trait]
impl crate::MicroserviceServer for GrpcMicroserviceServer {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: crate::ShutdownFuture,
    ) -> Result<(), TransportError> {
        (*self).listen_with_shutdown(shutdown).await
    }
}
