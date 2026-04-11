use axum::body::to_bytes;
use axum::http::{HeaderValue, StatusCode};
use nestrs::prelude::*;
use std::sync::Arc;

fn test_i18n_options() -> I18nOptions {
    let mut o = I18nOptions::new().with_fallback_locale("en");
    o.insert("en", "greeting", "Hello");
    o.insert("fr", "greeting", "Bonjour");
    o.insert("en", "welcome", "Hello, {name}!");
    o.insert("fr", "welcome", "Bonjour, {name}!");
    o
}

#[injectable]
struct AppState;

#[controller(prefix = "/i18n")]
struct I18nController;

#[routes(state = AppState)]
impl I18nController {
    #[get("/greet")]
    async fn greet(State(_): State<Arc<AppState>>, i18n: I18n) -> String {
        i18n.t("greeting")
    }

    #[get("/welcome")]
    async fn welcome(State(_): State<Arc<AppState>>, i18n: I18n) -> String {
        i18n.t_with("welcome", &[("name", "Ada")])
    }
}

#[module(
    imports = [I18nModule::register(test_i18n_options())],
    providers = [AppState],
    controllers = [I18nController]
)]
struct AppModule;

#[tokio::test]
async fn i18n_resolves_locale_and_translates() {
    let m = TestingModule::builder::<AppModule>()
        .configure_http(|app| app.use_i18n())
        .compile()
        .await;

    let client = m.http_client();

    // Accept-Language selects fr.
    let res = client
        .get("/i18n/greet")
        .header(
            axum::http::header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("fr-FR,fr;q=0.9,en;q=0.8"),
        )
        .send()
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "Bonjour");

    // Query param overrides header.
    let res = client
        .get("/i18n/greet?lang=en")
        .header(
            axum::http::header::ACCEPT_LANGUAGE,
            HeaderValue::from_static("fr"),
        )
        .send()
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "Hello");

    // Interpolation.
    let res = client.get("/i18n/welcome?lang=fr").send().await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = to_bytes(res.into_body(), 1024).await.expect("body");
    assert_eq!(std::str::from_utf8(&body).expect("utf8"), "Bonjour, Ada!");
}
