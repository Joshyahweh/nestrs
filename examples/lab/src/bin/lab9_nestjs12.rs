//! Lab 9 — NestJS 12 parity smoke (Wave 0–5).
//!
//! Default mode runs in-process HTTP checks against `into_router()` and exits
//! non-zero on failure. Pass `serve` to keep listening for manual curl.
//!
//! ```text
//! cargo run -p lab --bin lab9_nestjs12
//! cargo run -p lab --bin lab9_nestjs12 -- serve
//! ```

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tower::ServiceExt;

static MAIL_INBOX: OnceLock<Arc<nestrs::mail::InMemoryMailTransport>> = OnceLock::new();

fn mail_module() -> DynamicModule {
    let inbox = nestrs::mail::InMemoryMailTransport::new();
    let _ = MAIL_INBOX.set(Arc::clone(&inbox));
    nestrs::mail::MailModule::register(nestrs::mail::MailOptions {
        transport: inbox,
    })
}

fn ai_module() -> DynamicModule {
    use nestrs::ai::{AiModule, AiOptions, FakeModel, LanguageModel};
    let fake = FakeModel::new(vec![
        Ok(json!({"text": "hello-from-fake-model"})),
        Ok(json!({"text": "hello-from-fake-model"})),
        Ok(json!({"text": "hello-from-fake-model"})),
    ]);
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        Arc::new(fake) as Arc<dyn LanguageModel>,
    );
    AiModule::register(AiOptions {
        models,
        defaults_timeout_ms: 5_000,
        max_steps: 3,
    })
}

#[injectable]
pub struct DemoState {
    resilience: Arc<nestrs::resilience::ResilienceService>,
    mail: Arc<nestrs::mail::MailService>,
    ai: Arc<nestrs::ai::AiService>,
    locks: Arc<nestrs::locks::LocksService>,
    outbox: Arc<nestrs::outbox::OutboxRelay>,
    workflows: Arc<nestrs::workflows::WorkflowsService>,
    webhooks: Arc<nestrs::webhooks::WebhooksService>,
}

#[controller(prefix = "/n12")]
pub struct DemoController;

#[routes(state = DemoState)]
impl DemoController {
    #[get("/health")]
    pub async fn health() -> Json<Value> {
        Json(json!({ "ok": true, "lab": "nestjs12" }))
    }

    /// Wave 0 — `errorCode` on `HttpException`.
    #[get("/weak-password")]
    pub async fn weak_password() -> Result<(), HttpException> {
        Err(BadRequestException::new("Password is too weak").with_error_code("WEAK_PASSWORD"))
    }

    /// Wave 1 — resilience retry then success.
    #[get("/resilience")]
    pub async fn resilience(State(state): State<Arc<DemoState>>) -> Json<Value> {
        use nestrs::resilience::{PolicyBundle, ResilienceError, RetryPolicy};
        use std::sync::atomic::{AtomicU32, Ordering};
        let attempts = AtomicU32::new(0);
        let policy = PolicyBundle {
            retry: Some(RetryPolicy {
                attempts: 4,
                delay: Duration::from_millis(1),
                factor: 1.0,
                max_delay: Duration::from_millis(1),
                jitter: false,
            }),
            ..Default::default()
        };
        let n = state
            .resilience
            .execute("lab9", policy, || async {
                let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    return Err(ResilienceError::Other(format!("fail-{n}")));
                }
                Ok(n)
            })
            .await
            .expect("eventually ok");
        Json(json!({ "attempts": n }))
    }

    /// Wave 2 — mail via in-memory transport.
    #[post("/mail")]
    pub async fn mail(State(state): State<Arc<DemoState>>) -> Json<Value> {
        use nestrs::mail::{render_template, MailMessage};
        let mut data = HashMap::new();
        data.insert("name".into(), "<script>".into());
        let html = render_template("Hello {{name}}", &data);
        state
            .mail
            .send(MailMessage {
                to: "demo@nestrs.dev".into(),
                subject: "lab9".into(),
                html: html.clone(),
                idempotency_key: Some("lab9-mail".into()),
            })
            .await
            .expect("mail send");
        Json(json!({ "escaped": html.contains("&lt;script&gt;") }))
    }

    /// Wave 3 — AI FakeModel.
    #[post("/ai")]
    pub async fn ai(State(state): State<Arc<DemoState>>) -> Json<Value> {
        use nestrs::ai::GenerateTextRequest;
        let out = state
            .ai
            .generate_text(GenerateTextRequest {
                model: None,
                label: "lab9".into(),
                prompt: "hi".into(),
                instructions: None,
                tools: vec![],
            })
            .await
            .expect("ai");
        Json(json!({ "text": out.text }))
    }

    /// Wave 1 — locks exclusivity.
    #[get("/locks")]
    pub async fn locks(State(state): State<Arc<DemoState>>) -> Json<Value> {
        let g1 = state.locks.try_acquire("lab9").await.expect("first");
        let second = state.locks.try_acquire("lab9").await.is_none();
        g1.release().await;
        let third = state.locks.try_acquire("lab9").await.is_some();
        Json(json!({ "exclusive": second, "reacquire": third }))
    }

    /// Wave 1 — outbox enqueue + relay.
    #[post("/outbox")]
    pub async fn outbox(State(state): State<Arc<DemoState>>) -> Json<Value> {
        use nestrs::outbox::PublishFn;
        use std::sync::atomic::{AtomicU32, Ordering};
        state
            .outbox
            .add("lab9", json!({ "hello": "world" }))
            .await;
        let published = Arc::new(AtomicU32::new(0));
        let published_c = Arc::clone(&published);
        let publish: PublishFn = Arc::new(move |_topic, _payload| {
            published_c.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(()) })
        });
        state.outbox.tick(&publish).await;
        Json(json!({ "published": published.load(Ordering::SeqCst) }))
    }

    /// Wave 3 — workflow step survives “restart” (new service handle, same store).
    #[post("/workflows")]
    pub async fn workflows(State(state): State<Arc<DemoState>>) -> Json<Value> {
        let inst = state.workflows.start("order", json!({ "id": 1 })).await;
        state
            .workflows
            .complete_step(&inst.id, "charge", json!({ "ok": true }))
            .await
            .expect("step");
        let loaded = state.workflows.get(&inst.id).await.expect("loaded");
        Json(json!({
            "id": loaded.id,
            "has_charge": loaded.steps.contains_key("charge"),
        }))
    }

    /// Wave 2 — Standard Webhooks sign/verify.
    #[post("/webhooks/roundtrip")]
    pub async fn webhooks(State(state): State<Arc<DemoState>>) -> Json<Value> {
        let body = br#"{"event":"lab9"}"#;
        let signed = state.webhooks.sign("msg_lab9", 1_700_000_000, body);
        state
            .webhooks
            .verify(
                "msg_lab9",
                "1700000000",
                &signed.signature_header,
                body,
            )
            .expect("verify");
        Json(json!({ "ok": true }))
    }
}

#[module(
    imports = [
        nestrs::resilience::ResilienceModule::register(nestrs::resilience::ResilienceOptions::default()),
        mail_module(),
        ai_module(),
        nestrs::locks::LocksModule::register(nestrs::locks::LocksOptions::default()),
        nestrs::outbox::OutboxModule::register(nestrs::outbox::OutboxOptions::default()),
        nestrs::workflows::WorkflowsModule::register(nestrs::workflows::WorkflowsOptions::default()),
        nestrs::webhooks::WebhooksModule::register(nestrs::webhooks::WebhooksOptions::new("whsec_lab9")),
    ],
    providers = [DemoState],
    controllers = [DemoController]
)]
pub struct LabModule;

async fn get(router: &axum::Router, path: &str) -> (StatusCode, Value) {
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("oneshot");
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 64 * 1024).await.expect("body");
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v)
}

async fn post(router: &axum::Router, path: &str) -> (StatusCode, Value) {
    let res = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .body(Body::empty())
                .expect("req"),
        )
        .await
        .expect("oneshot");
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 64 * 1024).await.expect("body");
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, v)
}

fn check(name: &str, ok: bool, detail: impl std::fmt::Display) -> bool {
    if ok {
        println!("  PASS  {name} — {detail}");
    } else {
        println!("  FAIL  {name} — {detail}");
    }
    ok
}

async fn run_smoke() -> bool {
    println!("NestJS 12 lab smoke (in-process router)\n");
    let app = NestFactory::create::<LabModule>().set_global_prefix("lab");
    let router = app.into_router();

    let mut ok = true;

    let (st, v) = get(&router, "/lab/n12/health").await;
    ok &= check("health", st == StatusCode::OK && v["ok"] == true, format!("{st} {v}"));

    let (st, v) = get(&router, "/lab/n12/weak-password").await;
    ok &= check(
        "errorCode",
        st == StatusCode::BAD_REQUEST && v["errorCode"] == "WEAK_PASSWORD",
        format!("{st} {v}"),
    );

    let (st, v) = get(&router, "/lab/n12/resilience").await;
    ok &= check(
        "resilience",
        st == StatusCode::OK && v["attempts"] == 3,
        format!("{st} {v}"),
    );

    let (st, v) = post(&router, "/lab/n12/mail").await;
    ok &= check(
        "mail",
        st == StatusCode::OK && v["escaped"] == true,
        format!("{st} {v}"),
    );
    if let Some(inbox) = MAIL_INBOX.get() {
        let n = inbox.inbox().await.len();
        ok &= check("mail inbox", n >= 1, format!("{n} message(s)"));
    } else {
        ok &= check("mail inbox", false, "inbox handle missing");
    }

    let (st, v) = post(&router, "/lab/n12/ai").await;
    ok &= check(
        "ai FakeModel",
        st == StatusCode::OK && v["text"] == "hello-from-fake-model",
        format!("{st} {v}"),
    );

    let (st, v) = get(&router, "/lab/n12/locks").await;
    ok &= check(
        "locks",
        st == StatusCode::OK && v["exclusive"] == true && v["reacquire"] == true,
        format!("{st} {v}"),
    );

    let (st, v) = post(&router, "/lab/n12/outbox").await;
    ok &= check(
        "outbox",
        st == StatusCode::OK && v["published"] == 1,
        format!("{st} {v}"),
    );

    let (st, v) = post(&router, "/lab/n12/workflows").await;
    ok &= check(
        "workflows",
        st == StatusCode::OK && v["has_charge"] == true,
        format!("{st} {v}"),
    );

    let (st, v) = post(&router, "/lab/n12/webhooks/roundtrip").await;
    ok &= check(
        "webhooks",
        st == StatusCode::OK && v["ok"] == true,
        format!("{st} {v}"),
    );

    // Direct store race (Wave 5 property-style).
    {
        use nestrs::idempotency::{IdempotencyStore, InMemoryIdempotencyStore};
        let store = Arc::new(InMemoryIdempotencyStore::new(Duration::from_secs(60)));
        let mut handles = Vec::new();
        for _ in 0..16 {
            let s = Arc::clone(&store);
            handles.push(tokio::spawn(async move { s.begin_or_get("lab9:race").await }));
        }
        let mut winners = 0usize;
        for h in handles {
            if matches!(h.await.expect("join"), Ok(None)) {
                winners += 1;
            }
        }
        ok &= check("idempotency race", winners == 1, format!("winners={winners}"));
    }

    println!();
    if ok {
        println!("All NestJS 12 lab checks passed.");
    } else {
        println!("Some NestJS 12 lab checks failed.");
    }
    ok
}

#[tokio::main]
async fn main() {
    let mode = std::env::args().nth(1);
    if mode.as_deref() == Some("serve") {
        println!("Listening on http://127.0.0.1:3612/lab/n12/health");
        NestFactory::create::<LabModule>()
            .set_global_prefix("lab")
            .listen_graceful(3612)
            .await;
        return;
    }

    if !run_smoke().await {
        std::process::exit(1);
    }
}
