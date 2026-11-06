//! Property-style round-trip tests for Standard Webhooks signing (security gate).

use nestrs_webhooks::{sign_standard_webhook, verify_standard_webhook};

#[test]
fn roundtrip_random_bodies() {
    let secret = "whsec_dGVzdA=="; // base64("test")
    for i in 0..64u64 {
        let body = format!(r#"{{"n":{i},"msg":"hello-{i}"}}"#);
        let msg_id = format!("msg_{i}");
        let ts = 1_700_000_000 + i as i64;
        let signed = sign_standard_webhook(secret, &msg_id, ts, body.as_bytes());
        verify_standard_webhook(
            secret,
            &msg_id,
            &ts.to_string(),
            &signed.signature_header,
            body.as_bytes(),
        )
        .unwrap_or_else(|e| panic!("verify failed for i={i}: {e}"));
    }
}

#[test]
fn rejects_tampered_body() {
    let secret = "whsec_dGVzdA==";
    let body = br#"{"ok":true}"#;
    let signed = sign_standard_webhook(secret, "msg_x", 1_700_000_000, body);
    let err = verify_standard_webhook(
        secret,
        "msg_x",
        "1700000000",
        &signed.signature_header,
        br#"{"ok":false}"#,
    );
    assert!(err.is_err());
}

#[test]
fn rejects_wrong_secret() {
    let body = br#"{"ok":true}"#;
    let signed = sign_standard_webhook("whsec_dGVzdA==", "msg_x", 1_700_000_000, body);
    let err = verify_standard_webhook(
        "whsec_b3RoZXI=",
        "msg_x",
        "1700000000",
        &signed.signature_header,
        body,
    );
    assert!(err.is_err());
}
