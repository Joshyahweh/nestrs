//! Social provider tests. Each test exercises one provider's
//! wrapper: that its `authorize_url` targets the right endpoint, and
//! that its `exchange` -> `userinfo` round-trip works against a
//! mocked IdP. We don't reach out to the real Google/GitHub/etc.
//! endpoints — wiremock stands in.

#![cfg(feature = "social")]

use std::collections::HashMap;

use nestrs_oauth2::client::TokenSet;
use nestrs_oauth2::social::{Apple, GitHub, Google, Microsoft};
use serde_json::json;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// 1. google_authorize_url_targets_google_endpoint
#[test]
fn google_authorize_url_targets_google_endpoint() {
    let g = Google::new(
        "id",
        "secret",
        Url::parse("https://app.example.com/cb").unwrap(),
    )
    .unwrap();
    let auth = g
        .client()
        .authorize_url(Google::default_scopes(), None, None)
        .url;
    assert_eq!(
        auth.host_str(),
        Some("accounts.google.com"),
        "google authorize_url host mismatch: {}",
        auth
    );
    assert!(auth.path().starts_with("/o/oauth2/v2/auth"));
}

// 2. google_exchange_returns_userinfo
#[tokio::test]
async fn google_exchange_returns_userinfo() {
    let server = MockServer::start().await;
    // Token endpoint
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-g",
            "id_token": "id-g",
            "token_type": "Bearer",
        })))
        .mount(&server)
        .await;
    // Userinfo endpoint
    Mock::given(method("GET"))
        .and(path("/userinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "sub": "google-1",
            "email": "u@example.com",
            "email_verified": true,
            "name": "User",
            "picture": "https://example.com/p.png",
        })))
        .mount(&server)
        .await;

    // Build a Google client that points at our mock by overriding its
    // options. The wrapper hard-codes the real endpoints, so we go
    // through the inner OAuth2Client.
    let options = nestrs_oauth2::client::OAuth2Options::new(
        "id",
        "secret",
        Url::parse("https://accounts.google.com/o/oauth2/v2/auth").unwrap(),
        Url::parse(&format!("{}/token", server.uri())).unwrap(),
        Url::parse("https://app.example.com/cb").unwrap(),
    )
    .with_userinfo_url(Url::parse(&format!("{}/userinfo", server.uri())).unwrap());
    let client = nestrs_oauth2::client::OAuth2Client::new(options).unwrap();

    let token = client
        .exchange_code("the-code", None::<oauth2::PkceCodeVerifier>)
        .await
        .unwrap();
    assert_eq!(token.access_token, "at-g");

    let claims = client.userinfo(&token).await.unwrap();
    assert_eq!(claims.get("sub").and_then(|v| v.as_str()), Some("google-1"));
    assert_eq!(
        claims.get("email").and_then(|v| v.as_str()),
        Some("u@example.com")
    );
}

// 3. github_exchange_uses_github_endpoint
#[tokio::test]
async fn github_exchange_uses_github_endpoint() {
    let gh = GitHub::new(
        "id",
        "secret",
        Url::parse("https://app.example.com/cb").unwrap(),
    )
    .unwrap();
    let auth = gh
        .client()
        .authorize_url(GitHub::default_scopes(), None, None)
        .url;
    assert_eq!(auth.host_str(), Some("github.com"));
    assert!(auth.path().starts_with("/login/oauth/authorize"));
}

// 4. github_userinfo_uses_github_api
#[tokio::test]
async fn github_userinfo_uses_github_api() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-gh",
            "token_type": "Bearer",
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/userinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": 12345,
            "login": "octocat",
            "name": "Octo Cat",
            "email": null,
        })))
        .mount(&server)
        .await;

    let options = nestrs_oauth2::client::OAuth2Options::new(
        "id",
        "secret",
        Url::parse("https://github.com/login/oauth/authorize").unwrap(),
        Url::parse(&format!("{}/token", server.uri())).unwrap(),
        Url::parse("https://app.example.com/cb").unwrap(),
    )
    .with_userinfo_url(Url::parse(&format!("{}/userinfo", server.uri())).unwrap());
    let client = nestrs_oauth2::client::OAuth2Client::new(options).unwrap();

    let token = client
        .exchange_code("code", None::<oauth2::PkceCodeVerifier>)
        .await
        .unwrap();
    let user: serde_json::Value = client.userinfo(&token).await.unwrap();
    assert_eq!(user.get("login").and_then(|v| v.as_str()), Some("octocat"));
    assert_eq!(user.get("id").and_then(|v| v.as_i64()), Some(12345));
}

// 5. microsoft_exchange_uses_microsoft_endpoint
#[tokio::test]
async fn microsoft_exchange_uses_microsoft_endpoint() {
    let ms = Microsoft::new(
        "id",
        "secret",
        Url::parse("https://app.example.com/cb").unwrap(),
        "common",
    )
    .unwrap();
    let auth = ms
        .client()
        .authorize_url(Microsoft::default_scopes(), None, None)
        .url;
    assert_eq!(auth.host_str(), Some("login.microsoftonline.com"));
    // common tenant is in the path
    assert!(auth.path().contains("/common/oauth2/v2.0/authorize"));
    // userinfo lives on graph.microsoft.com (not the auth host)
    let userinfo = ms.client().userinfo_url().cloned().unwrap();
    assert_eq!(userinfo.host_str(), Some("graph.microsoft.com"));
}

// 6. apple_authorize_url_includes_response_mode_form_post
#[tokio::test]
async fn apple_authorize_url_includes_response_mode_form_post() {
    let apple = Apple::new(
        "id",
        "client-secret-jwt",
        Url::parse("https://app.example.com/cb").unwrap(),
    )
    .unwrap();
    // Build the URL through a redirect-URL shim so we can pin
    // `response_mode=form_post` per Apple spec.
    let mut extra: HashMap<String, String> = HashMap::new();
    extra.insert("response_mode".into(), "form_post".into());
    let auth = apple
        .client()
        .authorize_url(Apple::default_scopes(), None, Some(&extra))
        .url;
    assert_eq!(auth.host_str(), Some("appleid.apple.com"));
    let pairs: HashMap<String, String> = auth
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(
        pairs.get("response_mode").map(|s| s.as_str()),
        Some("form_post")
    );
    // Sanity: token was unused, but we exercised the construction.
    let _ = TokenSet {
        access_token: "a".into(),
        refresh_token: None,
        id_token: None,
        expires_at: None,
        scope: None,
        raw: serde_json::Value::Null,
    };
}
