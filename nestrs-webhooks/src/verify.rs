use crate::sign::sign_standard_webhook;
use subtle::ConstantTimeEq;

/// Replay window used by [`verify_standard_webhook`]. Matches the
/// Standard Webhooks recommendation (5 minutes, both directions).
pub const DEFAULT_WEBHOOK_TOLERANCE_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookVerifyError {
    BadTimestamp,
    BadSignature,
}

impl std::fmt::Display for WebhookVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadTimestamp => write!(f, "invalid webhook timestamp"),
            Self::BadSignature => write!(f, "invalid webhook signature"),
        }
    }
}

impl std::error::Error for WebhookVerifyError {}

/// Verify a Standard Webhooks signature and reject timestamps outside
/// [`DEFAULT_WEBHOOK_TOLERANCE_SECS`] of the current time.
///
/// A valid signature alone is not enough: without a freshness check a
/// captured delivery can be replayed indefinitely. Callers that must
/// accept an older delivery (a queue drain, a fixture) should use
/// [`verify_standard_webhook_within`] with an explicit `now_unix` and
/// tolerance instead of disabling the check.
pub fn verify_standard_webhook(
    secret: &str,
    msg_id: &str,
    timestamp: &str,
    signature_header: &str,
    body: &[u8],
) -> Result<(), WebhookVerifyError> {
    verify_standard_webhook_within(
        secret,
        msg_id,
        timestamp,
        signature_header,
        body,
        unix_now(),
        DEFAULT_WEBHOOK_TOLERANCE_SECS,
    )
}

/// Same checks as [`verify_standard_webhook`], with an explicit clock
/// and tolerance (seconds). A negative tolerance is treated as zero
/// (only the exact second is accepted).
pub fn verify_standard_webhook_within(
    secret: &str,
    msg_id: &str,
    timestamp: &str,
    signature_header: &str,
    body: &[u8],
    now_unix: i64,
    tolerance_secs: i64,
) -> Result<(), WebhookVerifyError> {
    let ts: i64 = timestamp
        .parse()
        .map_err(|_| WebhookVerifyError::BadTimestamp)?;
    // Authenticate before consulting the clock so a bad signature does
    // not become a timestamp oracle beyond the parse check above.
    if !signature_matches(secret, msg_id, ts, signature_header, body) {
        return Err(WebhookVerifyError::BadSignature);
    }
    let tolerance = tolerance_secs.max(0) as i128;
    let delta = (ts as i128) - (now_unix as i128);
    if delta.abs() > tolerance {
        return Err(WebhookVerifyError::BadTimestamp);
    }
    Ok(())
}

fn signature_matches(
    secret: &str,
    msg_id: &str,
    ts: i64,
    signature_header: &str,
    body: &[u8],
) -> bool {
    let expected = sign_standard_webhook(secret, msg_id, ts, body);
    // Header may contain multiple `v1,<sig>` entries separated by spaces.
    signature_header.split_whitespace().any(|part| {
        expected
            .signature_header
            .as_bytes()
            .ct_eq(part.as_bytes())
            .into()
    })
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_a_signature_inside_the_window() {
        let secret = "whsec_dGVzdA==";
        let body = br#"{"ok":true}"#;
        let ts = 1_700_000_000_i64;
        let signed = sign_standard_webhook(secret, "msg", ts, body);
        verify_standard_webhook_within(
            secret,
            "msg",
            &ts.to_string(),
            &signed.signature_header,
            body,
            ts,
            DEFAULT_WEBHOOK_TOLERANCE_SECS,
        )
        .expect("fresh signature");
    }

    #[test]
    fn rejects_a_replayed_signature_outside_the_window() {
        let secret = "whsec_dGVzdA==";
        let body = br#"{"ok":true}"#;
        let ts = 1_700_000_000_i64;
        let signed = sign_standard_webhook(secret, "msg", ts, body);
        let err = verify_standard_webhook_within(
            secret,
            "msg",
            &ts.to_string(),
            &signed.signature_header,
            body,
            ts + DEFAULT_WEBHOOK_TOLERANCE_SECS + 1,
            DEFAULT_WEBHOOK_TOLERANCE_SECS,
        )
        .unwrap_err();
        assert_eq!(err, WebhookVerifyError::BadTimestamp);
    }

    #[test]
    fn rejects_a_future_timestamp_outside_the_window() {
        let secret = "whsec_dGVzdA==";
        let body = br#"{"ok":true}"#;
        let now = 1_700_000_000_i64;
        let ts = now + DEFAULT_WEBHOOK_TOLERANCE_SECS + 30;
        let signed = sign_standard_webhook(secret, "msg", ts, body);
        let err = verify_standard_webhook_within(
            secret,
            "msg",
            &ts.to_string(),
            &signed.signature_header,
            body,
            now,
            DEFAULT_WEBHOOK_TOLERANCE_SECS,
        )
        .unwrap_err();
        assert_eq!(err, WebhookVerifyError::BadTimestamp);
    }

    #[test]
    fn tampered_body_is_a_bad_signature_even_when_fresh() {
        let secret = "whsec_dGVzdA==";
        let ts = 1_700_000_000_i64;
        let signed = sign_standard_webhook(secret, "msg", ts, br#"{"ok":true}"#);
        let err = verify_standard_webhook_within(
            secret,
            "msg",
            &ts.to_string(),
            &signed.signature_header,
            br#"{"ok":false}"#,
            ts,
            DEFAULT_WEBHOOK_TOLERANCE_SECS,
        )
        .unwrap_err();
        assert_eq!(err, WebhookVerifyError::BadSignature);
    }
}
