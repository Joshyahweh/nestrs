use crate::sign::sign_standard_webhook;
use subtle::ConstantTimeEq;

#[derive(Debug, Clone)]
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

pub fn verify_standard_webhook(
    secret: &str,
    msg_id: &str,
    timestamp: &str,
    signature_header: &str,
    body: &[u8],
) -> Result<(), WebhookVerifyError> {
    let ts: i64 = timestamp
        .parse()
        .map_err(|_| WebhookVerifyError::BadTimestamp)?;
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
