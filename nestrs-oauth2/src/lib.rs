//! OAuth2 client (4 grant types incl. PKCE), JWKS-backed resource server,
//! social providers (Google / GitHub / Microsoft / Apple), and
//! `OAuth2Guard` for nestrs route protection.
//!
//! Gated by feature flags so users opt in to the surface they need.
//! Default: no features. To use the OAuth2 client, opt in to `client`.
//! To use the JWKS resource server, opt in to `resource-server`. To use
//! the social provider wrappers, opt in to `social`. The guard + module
//! are only meaningful with at least one of the above.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod client;
pub mod error;
pub mod resource_server;
pub mod social;

#[cfg(feature = "guard")]
pub mod guard;
#[cfg(feature = "guard")]
pub mod middleware;
#[cfg(feature = "guard")]
pub mod module;

pub use client::{OAuth2Client, OAuth2Options, TokenSet};
pub use error::OAuth2Error;
pub use resource_server::{JwksCache, JwtVerifier, TokenData, ValidationConfig};
pub use social::{Apple, GitHub, Google, Microsoft};

#[cfg(feature = "guard")]
pub use guard::OAuth2Guard;
#[cfg(feature = "guard")]
pub use middleware::install_oauth2_middleware;
#[cfg(feature = "guard")]
pub use module::OAuth2Module;
