//! Security building blocks: auth helpers, optional CSRF (feature **`csrf`**), and docs for threat model.

pub mod auth;

#[cfg(feature = "csrf")]
pub mod csrf;

pub use auth::{
    parse_authorization_bearer, route_roles_csv, AuthStrategyGuard, BearerToken,
    OptionalBearerToken, XRoleMetadataGuard,
};

#[cfg(feature = "csrf")]
pub use csrf::{csrf_double_submit_middleware, CsrfProtectionConfig};
