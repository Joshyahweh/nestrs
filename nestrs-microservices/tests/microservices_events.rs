use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use nestrs_microservices::{ClientProxy, EventBus, Transport, TransportError};

#[derive(Default)]
struct InMemoryTransport;

#[async_trait]
impl Transport for InMemoryTransport {
    async fn send_json(
        &self,
        pattern: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, TransportError> {
        Ok(serde_json::json!({
            "pattern": pattern,
            "echo": payload
        }))
    }

    async fn emit_json(&self, _pattern: &str, _payload: serde_json::Value) -> Result<(), TransportError> {
        Ok(())
    }
}

#[tokio::test]
async fn client_proxy_send_round_trips_typed_payload() {
    let proxy = ClientProxy::new(Arc::new(InMemoryTransport));
    let res: serde_json::Value = proxy
        .send("user.get", &serde_json::json!({"id": 7}))
        .await
        .expect("send should succeed");

    assert_eq!(res["pattern"], "user.get");
    assert_eq!(res["echo"]["id"], 7);
}

#[tokio::test]
async fn event_bus_delivers_payload_to_subscribers() {
    let bus = EventBus::new();
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));

    let seen_a = seen.clone();
    bus.subscribe("order.created", move |payload| {
        let seen_a = seen_a.clone();
        async move {
            if let Some(id) = payload.get("id").and_then(|v| v.as_i64()) {
                seen_a.lock().expect("lock").push(format!("a:{id}"));
            }
        }
    });

    let seen_b = seen.clone();
    bus.subscribe("order.created", move |payload| {
        let seen_b = seen_b.clone();
        async move {
            if let Some(id) = payload.get("id").and_then(|v| v.as_i64()) {
                seen_b.lock().expect("lock").push(format!("b:{id}"));
            }
        }
    });

    bus.emit_json("order.created", serde_json::json!({ "id": 42 }))
        .await;

    let mut values = seen.lock().expect("lock").clone();
    values.sort();
    assert_eq!(values, vec!["a:42".to_string(), "b:42".to_string()]);
}
