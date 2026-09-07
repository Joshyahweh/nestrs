//! OAuth2 client / resource-server error type. Variants cover every
//! failure mode the public API can return: token endpoint errors
//! (per RFC 6749 §5.2), JWKS fetch failures, signature / claim
//! validation failures, configuration errors, and provider quirks.

use std::time::Duration;
use thiserror::Error;

/// Every error the public API can surface. `Display` is `tracing`-friendly
/// and `Error` is derived so callers can use `?` everywhere.
#[derive(Debug, Error)]
pub enum OAuth2Error {
    /// The IdP returned a non-2xx response on the token endpoint.
    /// `status` is the HTTP status; `body` is the raw response body
    /// (often a JSON-encoded `error` / `error_description` per RFC 6749
    /// §5.2).
    #[error("token endpoint returned {status}: {body}")]
    TokenEndpoint { status: u16, body: String },

    /// A specific RFC 6749 §5.2 `error` code, surfaced as a typed enum
    /// rather than a string. Mapping from the wire string is in
    /// `From<TokenEndpointError>` (private).
    #[error("OAuth2 error: {0}")]
    Grant(GrantError),

    /// JWKS HTTP fetch failed (network error, non-2xx, or parse error).
    /// `retries` is how many times we tried before giving up.
    #[error("JWKS fetch failed after {retries} retries: {source}")]
    JwksFetch {
        retries: u32,
        #[source]
        source: reqwest::Error,
    },

    /// Token signature / algorithm verification failed.
    #[error("JWT signature invalid: {0}")]
    InvalidSignature(String),

    /// Required claim missing or malformed.
    #[error("JWT claim invalid: {claim}: {reason}")]
    InvalidClaim { claim: String, reason: String },

    /// `iss` / `aud` / `exp` / `nbf` validation failed.
    #[error("JWT validation failed: {0}")]
    Validation(String),

    /// The token referenced a `kid` that isn't in the JWKS (and a refresh
    /// didn't surface it either).
    #[error("no matching JWK for kid={0}")]
    UnknownKid(String),

    /// User-supplied config is invalid (e.g. empty client_id, malformed URL).
    #[error("invalid OAuth2 config: {0}")]
    InvalidConfig(String),

    /// `reqwest`-level transport error not tied to a specific call.
    /// No `#[from]` derive because the source is sometimes wrapped
    /// (e.g. `HttpClientError::Reqwest(Box<reqwest::Error>)` from the
    /// v5 `oauth2` crate); we always construct this variant directly.
    #[error("OAuth2 transport error: {0}")]
    Transport(reqwest::Error),

    /// Underlying `oauth2` crate error (most often URL parsing or
    /// `StandardErrorResponse` deserialisation).
    #[error("OAuth2 client error: {0}")]
    Client(String),

    /// The refresh / exchange call hit the configured timeout.
    #[error("OAuth2 request timed out after {0:?}")]
    Timeout(Duration),

    /// Catch-all for unexpected provider responses (shape changes,
    /// undocumented fields, etc.).
    #[error("unexpected provider response: {0}")]
    Unexpected(String),
}

/// RFC 6749 §5.2 closed set of `error` codes. We map the wire string
/// into this enum so the caller's `match` is exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GrantError {
    #[error("invalid_request")]
    InvalidRequest,
    #[error("invalid_client")]
    InvalidClient,
    #[error("invalid_grant")]
    InvalidGrant,
    #[error("unauthorized_client")]
    UnauthorizedClient,
    #[error("unsupported_grant_type")]
    UnsupportedGrantType,
    #[error("invalid_scope")]
    InvalidScope,
    /// A `error` field we don't recognise — `Other` carries the wire
    /// string for diagnostics.
    #[error("{0}")]
    Other(&'static str),
}

impl GrantError {
    /// Map the wire string to a `GrantError`. `Other` is the fallback
    /// for unknown codes (RFC 6749 §5.2 says implementations MAY
    /// use extension error codes; we surface them as `Other`).
    pub fn from_wire(s: &str) -> Self {
        match s {
            "invalid_request" => Self::InvalidRequest,
            "invalid_client" => Self::InvalidClient,
            "invalid_grant" => Self::InvalidGrant,
            "unauthorized_client" => Self::UnauthorizedClient,
            "unsupported_grant_type" => Self::UnsupportedGrantType,
            "invalid_scope" => Self::InvalidScope,
            other => {
                // `&'static str` lifetime requires a const-promotable value;
                // we leak the unknown string into a const-promotable `Box<str>`-less
                // representation. For diagnostics this is fine — it only fires
                // when the provider returns something we don't recognise.
                Self::Other(Box::leak(other.to_owned().into_boxed_str()))
            }
        }
    }
}
