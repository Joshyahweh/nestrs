//! HTTP surface of the authorization server: `/authorize`, `/token`,
//! `/introspect`, `/revoke`, RFC 8414 discovery, and the JWKS endpoint.
//!
//! Mapping conventions:
//!
//! - Token responses (success **and** error) carry
//!   `Cache-Control: no-store` + `Pragma: no-cache` (RFC 6749 §5.1).
//! - `/authorize` never redirects until client + redirect URI are
//!   verified (open-redirect guard, see
//!   [`AuthorizeFailure`]); after
//!   verification, errors travel back via the redirect per §4.1.2.1.
//! - Client auth accepts `client_secret_basic` (HTTP Basic, preferred)
//!   and `client_secret_post` (form fields) — never both at once
//!   (RFC 6749 §2.3).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Form, Request, State};
use axum::http::header::{CACHE_CONTROL, LOCATION, PRAGMA, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::{json, Value};

use super::model::OAuth2ClientRecord;
use super::service::{
    AuthorizationServer, AuthorizeFailure, AuthorizeParams, TokenFailure, TokenRequest,
};

/// Build the standalone authorization-server router (axum). Mount into
/// any axum app, or let
/// [`OAuth2AuthorizationServerModule::register`](super::OAuth2AuthorizationServerModule::register)
/// compose it into a nestrs application.
pub fn router(server: Arc<AuthorizationServer>) -> Router {
    let prefix = server.config().endpoint_prefix.to_string();
    let discovery_route = server.config().discovery_path.to_string();
    let jwks_route = server.config().jwks_path.to_string();
    Router::new()
        .route(&format!("{prefix}/authorize"), get(authorize))
        .route(&format!("{prefix}/token"), post(token))
        .route(&format!("{prefix}/introspect"), post(introspect))
        .route(&format!("{prefix}/revoke"), post(revoke))
        .route(&discovery_route, get(discovery))
        .route(&jwks_route, get(jwks))
        .with_state(server)
}

/// GET `{prefix}/authorize` — RFC 6749 §4.1.1 (+ RFC 7636 PKCE).
/// 302-redirects to the client with `code` + `state` on success.
async fn authorize(State(server): State<Arc<AuthorizationServer>>, req: Request) -> Response {
    let (parts, _body) = req.into_parts();
    let query = parts.uri.query();
    let param = |key: &str| {
        query.and_then(|q| {
            url::form_urlencoded::parse(q.as_bytes())
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.into_owned())
        })
    };
    let non_empty = |value: Option<String>| value.filter(|v| !v.is_empty());
    let params = AuthorizeParams {
        client_id: non_empty(param("client_id")),
        redirect_uri: non_empty(param("redirect_uri")),
        response_type: non_empty(param("response_type")),
        scope: non_empty(param("scope")),
        // state must round-trip verbatim — an empty state echoes as
        // an empty state, so it is not filtered.
        state: param("state"),
        code_challenge: non_empty(param("code_challenge")),
        code_challenge_method: non_empty(param("code_challenge_method")),
        subject: (server.resource_owner())(&parts),
    };

    match server.authorize(&params).await {
        Ok(url) => redirect_302(url.as_str()),
        Err(AuthorizeFailure::Direct {
            status,
            error,
            description,
        }) => oauth_json(
            status,
            json!({ "error": error, "error_description": description }),
        ),
        Err(AuthorizeFailure::Redirect {
            redirect_uri,
            error,
            description,
            state,
        }) => {
            let mut url = url::Url::parse(&redirect_uri)
                .expect("redirect_uri was validated against the client registration");
            url.query_pairs_mut()
                .append_pair("error", error)
                .append_pair("error_description", &description);
            if let Some(state) = state {
                url.query_pairs_mut().append_pair("state", &state);
            }
            redirect_302(url.as_str())
        }
    }
}

/// POST `{prefix}/token` — RFC 6749 §3.2 (+ §4.1.3 code, §6 refresh,
/// §4.4 client credentials).
async fn token(
    State(server): State<Arc<AuthorizationServer>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    let record = match require_client(&server, &headers, &form).await {
        Ok(record) => record,
        Err(response) => return *response,
    };

    let Some(grant_type) = form
        .get("grant_type")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
    else {
        return token_error(
            TokenFailure::InvalidRequest {
                description: "grant_type is required".to_string(),
            },
            false,
        );
    };
    let f = |key: &str| form.get(key).cloned().filter(|v| !v.is_empty());
    let request = TokenRequest {
        grant_type,
        code: f("code"),
        redirect_uri: f("redirect_uri"),
        code_verifier: f("code_verifier"),
        refresh_token: f("refresh_token"),
        scope: form.get("scope").cloned().filter(|v| !v.trim().is_empty()),
    };

    match server.token(&request, &record).await {
        Ok(tokens) => {
            let mut body = json!({
                "access_token": tokens.access_token,
                "token_type": "Bearer",
                "expires_in": tokens.expires_in,
                "scope": tokens.scope,
            });
            if let Some(refresh) = tokens.refresh_token {
                body["refresh_token"] = json!(refresh);
            }
            oauth_json(200, body)
        }
        Err(failure) => token_error(failure, false),
    }
}

/// POST `{prefix}/introspect` — RFC 7662. Requires client
/// authentication; the response is always 200 with `active` true/false
/// (a bad token is data, not an error — §2.3).
async fn introspect(
    State(server): State<Arc<AuthorizationServer>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if let Err(response) = require_client(&server, &headers, &form).await {
        return *response;
    }

    // token_type_hint is advisory (RFC 7662 §2.1) — detection is
    // signature-vs-opaque, not hinted.
    let Some(token) = form
        .get("token")
        .map(String::as_str)
        .filter(|t| !t.is_empty())
    else {
        return oauth_json(
            400,
            json!({ "error": "invalid_request", "error_description": "token is required" }),
        );
    };
    oauth_json(200, server.introspect(token).await)
}

/// POST `{prefix}/revoke` — RFC 7009. Always 200 once the client
/// authenticated (invalid/unknown tokens included, §2.2).
async fn revoke(
    State(server): State<Arc<AuthorizationServer>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if let Err(response) = require_client(&server, &headers, &form).await {
        return *response;
    }

    let Some(token) = form
        .get("token")
        .map(String::as_str)
        .filter(|t| !t.is_empty())
    else {
        return oauth_json(
            400,
            json!({ "error": "invalid_request", "error_description": "token is required" }),
        );
    };
    server.revoke(token).await;
    no_store(StatusCode::OK.into_response())
}

async fn discovery(State(server): State<Arc<AuthorizationServer>>) -> Response {
    Json(server.discovery_document()).into_response()
}

async fn jwks(State(server): State<Arc<AuthorizationServer>>) -> Response {
    // Keys are content-addressed (kid); a short shared cache is safe
    // and keeps verifier-side JWKS fetches cheap.
    let mut response = Json(server.jwks_document().clone()).into_response();
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    response
}

/// Client authentication shared by the token / introspection /
/// revocation endpoints: resolve credentials, verify against the
/// client store. Errors come back as ready-to-return responses (boxed
/// — `Response` is large and this crosses the `Err`-variant size
/// lint; the box is transparent at the call sites via deref-move).
async fn require_client(
    server: &Arc<AuthorizationServer>,
    headers: &HeaderMap,
    form: &HashMap<String, String>,
) -> Result<OAuth2ClientRecord, Box<Response>> {
    let via_basic = matches!(
        basic_auth(headers),
        BasicAuth::Valid(..) | BasicAuth::Malformed
    );
    match extract_client_credentials(headers, form) {
        Ok(credentials) => {
            match server
                .authenticate_client(
                    credentials.client_id.as_deref(),
                    credentials.secret.as_deref(),
                )
                .await
            {
                Ok(record) => Ok(record),
                Err(failure) => Err(Box::new(token_error(failure, via_basic))),
            }
        }
        Err(failure) => Err(Box::new(token_error(failure, via_basic))),
    }
}

struct ClientCredentials {
    client_id: Option<String>,
    secret: Option<String>,
}

/// Resolve client credentials from the request: HTTP Basic (RFC 6749
/// §2.3.1 — the id/secret are `application/x-www-form-urlencoded`-
/// encoded before the base64 wrap) or form fields. Using both at once
/// is rejected.
fn extract_client_credentials(
    headers: &HeaderMap,
    form: &HashMap<String, String>,
) -> Result<ClientCredentials, TokenFailure> {
    let form_id = form.get("client_id").filter(|v| !v.is_empty()).cloned();
    let form_secret = form.get("client_secret").filter(|v| !v.is_empty()).cloned();

    match basic_auth(headers) {
        BasicAuth::Absent => Ok(ClientCredentials {
            client_id: form_id,
            secret: form_secret,
        }),
        BasicAuth::Malformed => Err(TokenFailure::InvalidClient {
            description: "malformed Basic authorization header".to_string(),
        }),
        BasicAuth::Valid(id, secret) => {
            if form_secret.is_some() {
                return Err(TokenFailure::InvalidRequest {
                    description: "multiple client authentication methods used (Basic and form)"
                        .to_string(),
                });
            }
            if let Some(fid) = &form_id {
                if fid != &id {
                    return Err(TokenFailure::InvalidRequest {
                        description: "client_id in the form does not match the Basic credentials"
                            .to_string(),
                    });
                }
            }
            Ok(ClientCredentials {
                client_id: Some(id),
                secret: Some(secret),
            })
        }
    }
}

enum BasicAuth {
    Absent,
    Malformed,
    Valid(String, String),
}

fn basic_auth(headers: &HeaderMap) -> BasicAuth {
    let Some(value) = headers.get(axum::http::header::AUTHORIZATION) else {
        return BasicAuth::Absent;
    };
    let Ok(value) = value.to_str() else {
        return BasicAuth::Malformed;
    };
    // Scheme is case-insensitive (RFC 7235). A non-Basic scheme here is
    // not client credentials — leave it absent rather than malformed.
    let Some((scheme, encoded)) = value.split_once(' ') else {
        return if value.eq_ignore_ascii_case("basic") {
            BasicAuth::Malformed
        } else {
            BasicAuth::Absent
        };
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return BasicAuth::Absent;
    }
    let Ok(decoded) = STANDARD.decode(encoded.trim()) else {
        return BasicAuth::Malformed;
    };
    let Ok(credentials) = String::from_utf8(decoded) else {
        return BasicAuth::Malformed;
    };
    // RFC 6749 §2.3.1: split on the FIRST colon — secrets may contain
    // colons.
    let Some((id_raw, secret_raw)) = credentials.split_once(':') else {
        return BasicAuth::Malformed;
    };
    // Both halves were application/x-www-form-urlencoded before the
    // base64 wrap; percent-decode each. A raw reserved character
    // (`&`, `=`, ...) cannot have been a legal encoding — reject.
    let (Some(id), Some(secret)) = (form_decode_single(id_raw), form_decode_single(secret_raw))
    else {
        return BasicAuth::Malformed;
    };
    BasicAuth::Valid(id, secret)
}

/// Percent-decode a single form value by round-tripping it through
/// `form_urlencoded::parse` as a lone key. `None` when the raw value
/// contains characters that cannot have been legally encoded.
fn form_decode_single(raw: &str) -> Option<String> {
    let padded = format!("{raw}=");
    let mut pairs = url::form_urlencoded::parse(padded.as_bytes());
    match (pairs.next(), pairs.next()) {
        (Some((key, value)), None) if value.is_empty() => Some(key.into_owned()),
        _ => None,
    }
}

fn token_error(failure: TokenFailure, via_basic: bool) -> Response {
    let mut response = oauth_json(
        failure.status(),
        json!({
            "error": failure.code(),
            "error_description": failure.description(),
        }),
    );
    if matches!(failure, TokenFailure::InvalidClient { .. }) && via_basic {
        response.headers_mut().insert(
            WWW_AUTHENTICATE,
            HeaderValue::from_static("Basic realm=\"oauth2\""),
        );
    }
    response
}

fn oauth_json(status: u16, body: Value) -> Response {
    let mut response = (
        StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST),
        Json(body),
    )
        .into_response();
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

fn no_store(response: Response) -> Response {
    let mut response = response;
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

fn redirect_302(location: &str) -> Response {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        LOCATION,
        HeaderValue::from_str(location).expect("valid location"),
    );
    response
}
