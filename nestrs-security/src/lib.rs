//! Security building blocks: auth helpers (Bearer parsing, extractors,
//! [`AuthStrategyGuard`]), helmet-style response-header middleware, and the
//! double-submit CSRF middleware (feature **`csrf`**).
//!
//! Extracted from the `nestrs/src/security/*` modules so apps can pull just this
//! crate without the rest of the framework; the umbrella crate re-exports every
//! public symbol at the same path.
//!
//! ## Auth (default)
//!
//! - [`parse_authorization_bearer`] — case-insensitive `Bearer` scheme parser.
//! - [`BearerToken`] / [`OptionalBearerToken`] — axum extractors.
//! - [`AuthStrategyGuard<S>`] — runs any [`AuthStrategy::validate`] for `S:
//!   AuthStrategy + Default`.
//! - [`DemoXRoleMetadataGuard`] — demo `#[roles(...)]` metadata check (client-
//!   trusted `x-role` header; **do not use in production**).
//!
//! ## CSRF (feature **`csrf`**)
//!
//! - [`CsrfProtectionConfig`] + [`csrf_double_submit_middleware`] — server-side
//!   cookie value vs `X-CSRF-Token` header value, constant-time compared.
//!
//! ## Helmet (Phase D surface — default)
//!
//! - [`HelmetConfig`] + [`helmet_middleware`] — `X-Frame-Options`,
//!   `X-Content-Type-Options`, `Strict-Transport-Security`, `Referrer-Policy`,
//!   `X-DNS-Prefetch-Control`, `Cross-Origin-Opener-Policy`. Install globally;
//!   the [`HelmetConfig::default`] matches the conservative defaults most apps
//!   ship with.
//!
//! **Docs:** mdBook **Security** (`docs/src/security.md`).

#![doc(html_root_url = "https://docs.rs/nestrs-security/1.0.0")]

mod auth;
mod helmet;

#[cfg(feature = "csrf")]
mod csrf;

// Re-exports from `nestrs-core` so callers can write
// `nestrs_security::AuthStrategy + nestrs_security::CanActivate` without
// depending on `nestrs-core` directly. Mirrors the umbrella `nestrs::core::*`
// surface, scoped to the symbols this crate actually uses.
pub use nestrs_core::{
    AuthError, AuthStrategy, CanActivate, GuardError, HandlerKey, MetadataRegistry,
};

pub use auth::{
    parse_authorization_bearer, route_roles_csv, AuthStrategyGuard, BearerToken,
    DemoXRoleMetadataGuard, OptionalBearerToken, SecurityRejection, XRoleMetadataGuard,
};
#[cfg(feature = "authz")]
pub use auth::route_metadata_csv;

#[cfg(feature = "csrf")]
pub use csrf::{csrf_double_submit_middleware, CsrfProtectionConfig};

pub use helmet::{helmet_middleware, HelmetConfig};
