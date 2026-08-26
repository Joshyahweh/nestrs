//! Lab 1 — security middleware under live fire: CSRF double-submit, rate limiting,
//! security headers, and trusted-proxy client-IP derivation.
//!
//! Run: `cargo run -p lab --bin lab1_security` then attack it with curl.

use nestrs::prelude::*;
use nestrs::ClientIp;
use nestrs::{CsrfProtectionConfig, RateLimitOptions, SecurityHeaders};
use std::sync::Arc;

#[injectable]
pub struct SecurityService;

impl SecurityService {
    pub fn ping(&self) -> &'static str {
        "pong"
    }
}

#[controller(prefix = "/sec")]
pub struct SecController;

#[routes(state = SecurityService)]
impl SecController {
    #[get("/ping")]
    pub async fn ping(State(svc): State<Arc<SecurityService>>) -> &'static str {
        svc.ping()
    }

    /// Issues the CSRF cookie (double-submit secret) and echoes it for the client.
    #[get("/token")]
    pub async fn token(cookies: tower_cookies::Cookies) -> String {
        let token = uuid_like();
        let cookie = tower_cookies::Cookie::build(("csrf_token", token.clone()))
            .path("/")
            .into();
        cookies.add(cookie);
        token
    }

    /// Unsafe method — must be rejected without a valid double-submit pair.
    #[post("/transfer")]
    pub async fn transfer() -> Json<serde_json::Value> {
        Json(serde_json::json!({ "ok": true, "moved": "to:acc-42" }))
    }

    /// Reports which client IP the framework derived (after trusted-proxy processing).
    #[get("/whoami")]
    pub async fn whoami(ClientIp(ip): ClientIp) -> String {
        ip.to_string()
    }
}

/// Cheap random-ish token for lab purposes only.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("lab-{nanos:x}")
}

#[module(controllers = [SecController], providers = [SecurityService])]
pub struct LabModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<LabModule>()
        .set_global_prefix("lab")
        .use_cookies()
        .use_csrf_protection(CsrfProtectionConfig::default())
        .use_security_headers(SecurityHeaders::default())
        .use_rate_limit(
            RateLimitOptions::builder()
                .max_requests(6)
                .window_secs(3)
                .build(),
        )
        .listen_graceful(3100)
        .await;
}
