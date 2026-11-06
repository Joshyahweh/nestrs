use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone)]
pub struct SignedWebhook {
    pub msg_id: String,
    pub timestamp: i64,
    pub signature_header: String,
}

/// Standard Webhooks signing: `v1,<base64(hmac_sha256(msg_id.timestamp.body))>`.
pub fn sign_standard_webhook(
    secret: &str,
    msg_id: &str,
    timestamp: i64,
    body: &[u8],
) -> SignedWebhook {
    let key = secret_bytes(secret);
    let mut mac = HmacSha256::new_from_slice(&key).expect("hmac key");
    let to_sign = format!("{msg_id}.{timestamp}.");
    mac.update(to_sign.as_bytes());
    mac.update(body);
    let sig = mac.finalize().into_bytes();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, sig);
    SignedWebhook {
        msg_id: msg_id.to_string(),
        timestamp,
        signature_header: format!("v1,{encoded}"),
    }
}

pub(crate) fn secret_bytes(secret: &str) -> Vec<u8> {
    if let Some(rest) = secret.strip_prefix("whsec_") {
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, rest)
            .unwrap_or_else(|_| secret.as_bytes().to_vec())
    } else {
        secret.as_bytes().to_vec()
    }
}
