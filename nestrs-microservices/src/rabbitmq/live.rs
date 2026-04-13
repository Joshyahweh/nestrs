use crate::wire::{dispatch_emit, dispatch_send, WireError, WireKind, WireRequest, WireResponse};
use crate::{MicroserviceHandler, MicroserviceServer, Transport, TransportError};
use async_trait::async_trait;
use futures_util::StreamExt;
use lapin::{options::*, types::FieldTable, BasicProperties, Connection, ConnectionProperties};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::OnceCell;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct RabbitMqTransportOptions {
    pub url: String,
    pub work_queue: String,
    pub request_timeout: std::time::Duration,
}

impl RabbitMqTransportOptions {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            work_queue: "nestrs.micro".to_string(),
            request_timeout: std::time::Duration::from_secs(30),
        }
    }

    pub fn with_work_queue(mut self, q: impl Into<String>) -> Self {
        self.work_queue = q.into();
        self
    }

    pub fn with_request_timeout(mut self, d: std::time::Duration) -> Self {
        self.request_timeout = d;
        self
    }
}

#[derive(Clone)]
pub struct RabbitMqTransport {
    options: RabbitMqTransportOptions,
    conn: Arc<OnceCell<Arc<Connection>>>,
}

impl RabbitMqTransport {
    pub fn new(options: RabbitMqTransportOptions) -> Self {
        Self {
            options,
            conn: Arc::new(OnceCell::new()),
        }
    }

    async fn connection(&self) -> Result<Arc<Connection>, TransportError> {
        self.conn
            .get_or_try_init(|| async {
                Connection::connect(&self.options.url, ConnectionProperties::default())
                    .await
                    .map(Arc::new)
                    .map_err(|e| TransportError::new(format!("rabbitmq connect failed: {e}")))
            })
            .await?;
        Ok(self
            .conn
            .get()
            .expect("rabbitmq connection initialized")
            .clone())
    }
}

#[async_trait]
impl Transport for RabbitMqTransport {
    async fn send_json(&self, pattern: &str, payload: Value) -> Result<Value, TransportError> {
        let conn = self.connection().await?;
        let consume_ch = (*conn)
            .create_channel()
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq channel failed: {e}")))?;
        let reply_name = format!("nestrs.reply.{}", Uuid::new_v4());
        consume_ch
            .queue_declare(
                &reply_name,
                QueueDeclareOptions {
                    exclusive: true,
                    auto_delete: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| {
                TransportError::new(format!("rabbitmq declare reply queue failed: {e}"))
            })?;

        let mut consumer = consume_ch
            .basic_consume(
                &reply_name,
                "nestrs_reply",
                BasicConsumeOptions {
                    no_ack: false,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq consume failed: {e}")))?;

        let wire = WireRequest {
            kind: WireKind::Send,
            pattern: pattern.to_string(),
            payload,
            reply: Some(reply_name),
            correlation_id: None,
        };
        let body = serde_json::to_vec(&wire)
            .map_err(|e| TransportError::new(format!("serialize request failed: {e}")))?;

        let pub_ch = (*conn)
            .create_channel()
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq publish channel failed: {e}")))?;
        pub_ch
            .basic_publish(
                "",
                &self.options.work_queue,
                BasicPublishOptions::default(),
                &body,
                BasicProperties::default(),
            )
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq publish failed: {e}")))?;

        #[cfg(feature = "microservice-metrics")]
        metrics::counter!("nestrs_microservice_rabbitmq_publish_total", "kind" => "send")
            .increment(1);

        let delivery = tokio::time::timeout(self.options.request_timeout, consumer.next())
            .await
            .map_err(|_| TransportError::new("rabbitmq request timed out"))?
            .ok_or_else(|| TransportError::new("rabbitmq reply stream ended"))?
            .map_err(|e| TransportError::new(format!("rabbitmq consumer error: {e}")))?;

        delivery
            .ack(BasicAckOptions::default())
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq ack failed: {e}")))?;

        let wire: WireResponse = serde_json::from_slice(delivery.data.as_slice())
            .map_err(|e| TransportError::new(format!("deserialize response failed: {e}")))?;
        if wire.ok {
            Ok(wire.payload.unwrap_or(Value::Null))
        } else {
            let err = wire.error.unwrap_or(WireError {
                message: "microservice error".to_string(),
                details: None,
            });
            let mut out = TransportError::new(err.message);
            if let Some(details) = err.details {
                out = out.with_details(details);
            }
            Err(out)
        }
    }

    async fn emit_json(&self, pattern: &str, payload: Value) -> Result<(), TransportError> {
        let conn = self.connection().await?;
        let ch = (*conn)
            .create_channel()
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq channel failed: {e}")))?;
        let wire = WireRequest {
            kind: WireKind::Emit,
            pattern: pattern.to_string(),
            payload,
            reply: None,
            correlation_id: None,
        };
        let body = serde_json::to_vec(&wire)
            .map_err(|e| TransportError::new(format!("serialize event failed: {e}")))?;
        ch.basic_publish(
            "",
            &self.options.work_queue,
            BasicPublishOptions::default(),
            &body,
            BasicProperties::default(),
        )
        .await
        .map_err(|e| TransportError::new(format!("rabbitmq publish failed: {e}")))?;

        #[cfg(feature = "microservice-metrics")]
        metrics::counter!("nestrs_microservice_rabbitmq_publish_total", "kind" => "emit")
            .increment(1);

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct RabbitMqMicroserviceOptions {
    pub url: String,
    pub work_queue: String,
    pub prefetch: u16,
    pub durable_queue: bool,
}

impl RabbitMqMicroserviceOptions {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            work_queue: "nestrs.micro".to_string(),
            prefetch: 32,
            durable_queue: true,
        }
    }

    pub fn with_work_queue(mut self, q: impl Into<String>) -> Self {
        self.work_queue = q.into();
        self
    }

    pub fn with_prefetch(mut self, n: u16) -> Self {
        self.prefetch = n;
        self
    }

    pub fn durable_queue(mut self, v: bool) -> Self {
        self.durable_queue = v;
        self
    }
}

pub struct RabbitMqMicroserviceServer {
    options: RabbitMqMicroserviceOptions,
    handlers: Vec<Arc<dyn MicroserviceHandler>>,
}

impl RabbitMqMicroserviceServer {
    pub fn new(
        options: RabbitMqMicroserviceOptions,
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
        let handlers = Arc::new(self.handlers);
        let conn = Arc::new(
            Connection::connect(&self.options.url, ConnectionProperties::default())
                .await
                .map_err(|e| TransportError::new(format!("rabbitmq connect failed: {e}")))?,
        );

        let channel = (*conn)
            .create_channel()
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq channel failed: {e}")))?;

        channel
            .queue_declare(
                &self.options.work_queue,
                QueueDeclareOptions {
                    durable: self.options.durable_queue,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq declare queue failed: {e}")))?;

        channel
            .basic_qos(self.options.prefetch, BasicQosOptions::default())
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq qos failed: {e}")))?;

        let mut consumer = channel
            .basic_consume(
                &self.options.work_queue,
                "nestrs_microservice",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(|e| TransportError::new(format!("rabbitmq consume failed: {e}")))?;

        tokio::pin!(shutdown);
        loop {
            tokio::select! {
                _ = &mut shutdown => break,
                d = consumer.next() => {
                    let Some(delivery) = d else { break };
                    let delivery = match delivery {
                        Ok(d) => d,
                        Err(_) => continue,
                    };

                    let ack_ch = channel.clone();
                    let tag = delivery.delivery_tag;
                    let handlers = handlers.clone();
                    let conn = conn.clone();

                    #[cfg(feature = "microservice-metrics")]
                    metrics::counter!("nestrs_microservice_rabbitmq_deliver_total").increment(1);

                    tokio::spawn(async move {
                        let req: WireRequest = match serde_json::from_slice(&delivery.data) {
                            Ok(v) => v,
                            Err(_) => {
                                let _ = ack_ch
                                    .basic_nack(
                                        tag,
                                        BasicNackOptions {
                                            requeue: false,
                                            ..Default::default()
                                        },
                                    )
                                    .await;
                                return;
                            }
                        };

                        match req.kind {
                            WireKind::Send => {
                                let Some(reply_q) = req.reply else {
                                    let _ = ack_ch.basic_ack(tag, BasicAckOptions::default()).await;
                                    return;
                                };
                                let res =
                                    dispatch_send(&handlers, &req.pattern, req.payload.clone()).await;
                                let wire = match res {
                                    Ok(v) => WireResponse {
                                        ok: true,
                                        payload: Some(v),
                                        error: None,
                                    },
                                    Err(e) => WireResponse {
                                        ok: false,
                                        payload: None,
                                        error: Some(WireError {
                                            message: e.message,
                                            details: e.details,
                                        }),
                                    },
                                };
                                if let Ok(bytes) = serde_json::to_vec(&wire) {
                                    if let Ok(ch) = (*conn).create_channel().await {
                                        let _ = ch
                                            .basic_publish(
                                                "",
                                                &reply_q,
                                                BasicPublishOptions::default(),
                                                &bytes,
                                                BasicProperties::default(),
                                            )
                                            .await;
                                    }
                                }
                            }
                            WireKind::Emit => {
                                dispatch_emit(&handlers, &req.pattern, req.payload.clone()).await;
                            }
                        }
                        let _ = ack_ch.basic_ack(tag, BasicAckOptions::default()).await;
                    });
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl MicroserviceServer for RabbitMqMicroserviceServer {
    async fn listen_with_shutdown(
        self: Box<Self>,
        shutdown: crate::ShutdownFuture,
    ) -> Result<(), TransportError> {
        (*self).listen_with_shutdown(shutdown).await
    }
}
