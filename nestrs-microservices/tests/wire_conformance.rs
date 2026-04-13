//! Golden JSON fixtures for [`nestrs_microservices::wire`] — lock the cross-transport contract
//! (Redis, Kafka, MQTT, RabbitMQ, custom, and gRPC JSON bodies) across crate versions.
//!
//! If you change serde attributes or field names on `WireRequest` / `WireResponse` / `WireError`,
//! update the files under `tests/fixtures/` and any consumers’ parsers.

use nestrs_microservices::wire::{WireError, WireKind, WireRequest, WireResponse};
use serde_json::{json, Value};

fn assert_roundtrip_fixture(path: &str) {
    let raw = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(path),
    )
    .unwrap_or_else(|e| panic!("read {path}: {e}"));
    let v1: Value = serde_json::from_str(&raw).expect("fixture parse");
    let again = serde_json::to_string(&v1).expect("re-stringify");
    let v2: Value = serde_json::from_str(&again).expect("round-trip parse");
    assert_eq!(v1, v2, "fixture {path} should round-trip as Value");
}

#[test]
fn golden_wire_request_send_serializes_to_fixture() {
    let req = WireRequest {
        kind: WireKind::Send,
        pattern: "user.get".into(),
        payload: json!({"id": 1}),
        reply: None,
        correlation_id: None,
    };
    let got = serde_json::to_value(&req).unwrap();
    let raw = include_str!("fixtures/wire_request_send.json");
    let expected: Value = serde_json::from_str(raw).unwrap();
    assert_eq!(got, expected, "WireRequest send minimal JSON drift");
    assert_roundtrip_fixture("wire_request_send.json");
}

#[test]
fn golden_wire_request_emit_matches_fixture() {
    let raw = include_str!("fixtures/wire_request_emit.json");
    let req: WireRequest = serde_json::from_str(raw).unwrap();
    assert!(matches!(req.kind, WireKind::Emit));
    assert_eq!(req.pattern, "order.created");
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        serde_json::from_str::<Value>(raw).unwrap()
    );
    assert_roundtrip_fixture("wire_request_emit.json");
}

#[test]
fn golden_wire_request_with_reply_and_correlation() {
    let raw = include_str!("fixtures/wire_request_with_reply.json");
    let req: WireRequest = serde_json::from_str(raw).unwrap();
    assert!(matches!(req.kind, WireKind::Send));
    assert_eq!(req.reply.as_deref(), Some("amq.gen-reply-1"));
    assert_eq!(req.correlation_id.as_deref(), Some("cid-9"));
    assert_eq!(
        serde_json::to_value(&req).unwrap(),
        serde_json::from_str::<Value>(raw).unwrap()
    );
    assert_roundtrip_fixture("wire_request_with_reply.json");
}

#[test]
fn golden_wire_response_ok_with_payload() {
    let raw = include_str!("fixtures/wire_response_ok.json");
    let res: WireResponse = serde_json::from_str(raw).unwrap();
    assert!(res.ok);
    assert_eq!(res.payload, Some(json!({"result": "ok"})));
    assert!(res.error.is_none());
    assert_eq!(
        serde_json::to_value(&res).unwrap(),
        serde_json::from_str::<Value>(raw).unwrap()
    );
    assert_roundtrip_fixture("wire_response_ok.json");
}

#[test]
fn golden_wire_response_ok_omits_null_payload() {
    let raw = include_str!("fixtures/wire_response_ok_no_payload.json");
    let res: WireResponse = serde_json::from_str(raw).unwrap();
    assert!(res.ok);
    assert!(res.payload.is_none());
    assert_eq!(
        serde_json::to_value(&res).unwrap(),
        serde_json::from_str::<Value>(raw).unwrap()
    );
    assert_roundtrip_fixture("wire_response_ok_no_payload.json");
}

#[test]
fn golden_wire_response_error_shape() {
    let raw = include_str!("fixtures/wire_response_error.json");
    let res: WireResponse = serde_json::from_str(raw).unwrap();
    assert!(!res.ok);
    let err = res.error.as_ref().expect("error");
    assert_eq!(err.message, "not found");
    assert_eq!(err.details.as_ref(), Some(&json!({"code": 404})));
    assert_eq!(
        serde_json::to_value(&res).unwrap(),
        serde_json::from_str::<Value>(raw).unwrap()
    );
    assert_roundtrip_fixture("wire_response_error.json");
}

#[test]
fn golden_wire_error_minimal() {
    let raw = include_str!("fixtures/wire_error_minimal.json");
    let err: WireError = serde_json::from_str(raw).unwrap();
    assert_eq!(err.message, "failed");
    assert!(err.details.is_none());
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        serde_json::from_str::<Value>(raw).unwrap()
    );
    assert_roundtrip_fixture("wire_error_minimal.json");
}

#[test]
fn wire_kind_snake_case_json() {
    assert_eq!(serde_json::to_string(&WireKind::Send).unwrap(), "\"send\"");
    assert_eq!(serde_json::to_string(&WireKind::Emit).unwrap(), "\"emit\"");
}
