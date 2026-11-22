use crate::sign::sign_standard_webhook;
use subtle::ConstantTimeEq;

/// Standard Webhooks replay window: five minutes either side of the verifier's clock.
pub const DEFAULT_WEBHOOK_TOLERANCE_SECS: i64 = 5 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebhookVerifyError {
    BadTimestamp,
    BadSignature,
}

impl std::fmt::Display for WebhookVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadTimestamp => {
                write!(
                    f,
                    "webhook timestamp is missing or outside the allowed tolerance"
                )
            }
            Self::BadSignature => write!(f, "invalid webhook signature"),
        }
    }
}

impl std::error::Error for WebhookVerifyError {}

pub fn verify_standard_webhook(
    secret: &str,
    msg_id: &str,
    timestamp: &str,
    signature_header: &str,
    body: &[u8],
) -> Result<(), WebhookVerifyError> {
    verify_standard_webhook_at(
        secret,
        msg_id,
        timestamp,
        signature_header,
        body,
        unix_now_secs(),
        DEFAULT_WEBHOOK_TOLERANCE_SECS,
    )
}

/// Verify a Standard Webhooks signature against an explicit clock.
///
/// `now_unix` and `tolerance_secs` are seconds. The timestamp must fall in
/// `[now_unix - tolerance_secs, now_unix + tolerance_secs]`. A negative
/// tolerance is treated as zero. Use this from tests, or when the caller
/// already knows the time the delivery was received.
pub fn verify_standard_webhook_at(
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
    if !timestamp_within_tolerance(ts, now_unix, tolerance_secs) {
        return Err(WebhookVerifyError::BadTimestamp);
    }
    let expected = sign_standard_webhook(secret, msg_id, ts, body);
    // Header may contain multiple `v1,<sig>` entries separated by spaces.
    let matched = signature_header.split_whitespace().any(|part| {
        expected
            .signature_header
            .as_bytes()
            .ct_eq(part.as_bytes())
            .into()
    });
    if matched {
        Ok(())
    } else {
        Err(WebhookVerifyError::BadSignature)
    }
}

fn unix_now_secs() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

fn timestamp_within_tolerance(ts: i64, now: i64, tolerance: i64) -> bool {
    let tolerance = tolerance.max(0);
    ts >= now.saturating_sub(tolerance) && ts <= now.saturating_add(tolerance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_timestamp_outside_the_window_even_with_a_valid_signature() {
        let secret = "whsec_dGVzdA==";
        let body = br#"{"ok":true}"#;
        let ts = 1_700_000_000_i64;
        let signed = sign_standard_webhook(secret, "msg_old", ts, body);
        let err = verify_standard_webhook_at(
            secret,
            "msg_old",
            &ts.to_string(),
            &signed.signature_header,
            body,
            ts + DEFAULT_WEBHOOK_TOLERANCE_SECS + 1,
            DEFAULT_WEBHOOK_TOLERANCE_SECS,
        );
        assert_eq!(err, Err(WebhookVerifyError::BadTimestamp));
    }

    #[test]
    fn rejects_a_future_timestamp_beyond_tolerance() {
        let secret = "whsec_dGVzdA==";
        let body = br#"{"ok":true}"#;
        let now = 1_700_000_000_i64;
        let ts = now + DEFAULT_WEBHOOK_TOLERANCE_SECS + 5;
        let signed = sign_standard_webhook(secret, "msg_new", ts, body);
        let err = verify_standard_webhook_at(
            secret,
            "msg_new",
            &ts.to_string(),
            &signed.signature_header,
            body,
            now,
            DEFAULT_WEBHOOK_TOLERANCE_SECS,
        );
        assert_eq!(err, Err(WebhookVerifyError::BadTimestamp));
    }

    #[test]
    fn accepts_a_timestamp_inside_the_window() {
        let secret = "whsec_dGVzdA==";
        let body = br#"{"ok":true}"#;
        let now = 1_700_000_000_i64;
        let ts = now - 30;
        let signed = sign_standard_webhook(secret, "msg_ok", ts, body);
        verify_standard_webhook_at(
            secret,
            "msg_ok",
            &ts.to_string(),
            &signed.signature_header,
            body,
            now,
            DEFAULT_WEBHOOK_TOLERANCE_SECS,
        )
        .expect("within tolerance");
    }

    #[test]
    fn negative_tolerance_does_not_widen_the_window() {
        let now = 1_700_000_000_i64;
        assert!(!super::timestamp_within_tolerance(now - 1, now, -5));
        assert!(super::timestamp_within_tolerance(now, now, -5));
    }
}
