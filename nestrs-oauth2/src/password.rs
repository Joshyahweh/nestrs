//! Password hashing helpers for `nestrs-oauth2` — Wave 7.13 (Tier 3.5).
//!
//! ## API
//!
//! - [`Backend`] — enum over the supported algorithms (`Bcrypt`, `Argon2`).
//! - [`hash`] — default-backend form used by the `#[derive(HashOnNew)]` macro.
//!   Defaults to Argon2id.
//! - [`hash_with`] — explicit-backend form for users who want bcrypt
//!   or argon2 specifically.
//! - [`verify`] — single-hash form with prefix auto-detection
//!   (`$2...` → bcrypt, `$argon2...` → argon2).
//! - [`verify_any`] — alias for [`verify`] with auto-detection; named to
//!   mirror the NestJS `bcrypt.compare` / `argon2.verify` shape where
//!   the hash format is inspected before dispatch.
//! - [`verify_with`] — explicit backend.
//! - [`PasswordHasher`] trait + [`BcryptHasher`] / [`Argon2Hasher`]
//!   impls — swappable backend for userland code that wants to pick
//!   algorithms at runtime.
//!
//! ## Feature gating
//!
//! The whole module is gated on
//! `any(feature = "password-bcrypt", feature = "password-argon2")`.
//! If only one backend is enabled, calls that dispatch to the disabled
//! backend return [`HashError::BcryptDisabled`] /
//! [`HashError::Argon2Disabled`] rather than silently producing wrong
//! results.
//!
//! ## Backend auto-detection
//!
//! Hash prefixes:
//!
//! - Bcrypt: `$2a$`, `$2b$`, `$2x$`, `$2y$` — all match the `$2` prefix.
//! - Argon2: `$argon2id$`, `$argon2i$`, `$argon2d$` — match `$argon2`.
//!
//! Anything else returns [`HashError::UnknownPrefix`].
//!
//! ## Why these choices
//!
//! - **Argon2id default** — modern OWASP recommendation. Bcrypt is
//!   retained for legacy hash migration only.
//! - **`verify` returns `Result`, not `bool`** — distinguishes "hash
//!   format unknown" (a real error) from "password mismatch" (a normal
//!   auth failure). Callers that want a bool can `.unwrap_or(false)`.
//! - **No `bcrypt` re-export at the crate root** — the bcrypt crate is
//!   an implementation detail of the `password` module.
//! - **`PasswordHasher` trait + concrete `BcryptHasher` / `Argon2Hasher`**
//!   — gives users a swappable backend without forcing them into the
//!   trait.

use std::fmt;

use thiserror::Error;

/// Password-hashing backend selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Backend {
    /// Bcrypt (legacy). Use only for hash migration.
    Bcrypt,
    /// Argon2id (default). Modern OWASP recommendation.
    #[default]
    Argon2,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Backend::Bcrypt => f.write_str("bcrypt"),
            Backend::Argon2 => f.write_str("argon2id"),
        }
    }
}

/// Errors produced by the password-hashing helpers.
#[derive(Debug, Error)]
pub enum HashError {
    /// Caller asked for bcrypt but the `password-bcrypt` feature is off.
    #[error(
        "bcrypt backend requested but the `password-bcrypt` feature is not enabled \
         — add `features = [\"password-bcrypt\"]` to nestrs-oauth2 in Cargo.toml"
    )]
    BcryptDisabled,

    /// Caller asked for argon2 but the `password-argon2` feature is off.
    #[error(
        "argon2 backend requested but the `password-argon2` feature is not enabled \
         — add `features = [\"password-argon2\"]` to nestrs-oauth2 in Cargo.toml"
    )]
    Argon2Disabled,

    /// The backend's RNG failed. Extremely rare; almost always means a
    /// broken `getrandom` backend on the target platform.
    #[error("could not generate a random salt for password hashing")]
    SaltGeneration,

    /// Backend rejected the on-disk hash (corrupt / truncated / wrong
    /// algorithm).
    #[error("backend rejected the hash: {0}")]
    InvalidHash(String),

    /// Backend rejected the password (too long for the algorithm —
    /// bcrypt caps at 72 bytes; argon2 has a much higher cap).
    #[error("backend rejected the password: {0}")]
    InvalidPassword(String),

    /// Hash prefix doesn't match any known backend. Likely a corrupt
    /// DB row or a hash from an unsupported algorithm.
    #[error(
        "unknown hash prefix `{0}` — expected `$2...` for bcrypt or `$argon2...` \
         for argon2id / argon2i / argon2d"
    )]
    UnknownPrefix(String),
}

/// Hash `plain` with the default backend (Argon2id). This is the
/// entry point used by the `#[derive(HashOnNew)]` macro.
pub fn hash(plain: &str) -> Result<String, HashError> {
    hash_with(plain, Backend::default())
}

/// Hash `plain` with an explicit backend.
pub fn hash_with(plain: &str, backend: Backend) -> Result<String, HashError> {
    match backend {
        #[cfg(feature = "password-bcrypt")]
        Backend::Bcrypt => bcrypt_hash(plain),
        #[cfg(not(feature = "password-bcrypt"))]
        Backend::Bcrypt => Err(HashError::BcryptDisabled),

        #[cfg(feature = "password-argon2")]
        Backend::Argon2 => argon2_hash(plain),
        #[cfg(not(feature = "password-argon2"))]
        Backend::Argon2 => Err(HashError::Argon2Disabled),
    }
}

/// Verify `plain` against `hash`, dispatching on the hash prefix.
pub fn verify(plain: &str, hash: &str) -> Result<bool, HashError> {
    verify_any(plain, hash)
}

/// Alias for [`verify`] — the `verify_any` name mirrors the
/// NestJS `bcrypt.compare` / `argon2.verify` shape where the hash
/// format is inspected first. Returns:
///
/// - `Ok(true)` on password match.
/// - `Ok(false)` on password mismatch (hash was valid).
/// - `Err(UnknownPrefix)` if the hash prefix is neither `$2...` nor
///   `$argon2...`.
/// - `Err(BcryptDisabled)` / `Err(Argon2Disabled)` if the matched
///   backend's feature is off.
pub fn verify_any(plain: &str, hash: &str) -> Result<bool, HashError> {
    if hash.starts_with("$2") {
        return verify_with(plain, hash, Backend::Bcrypt);
    }
    if hash.starts_with("$argon2") {
        return verify_with(plain, hash, Backend::Argon2);
    }
    Err(HashError::UnknownPrefix(prefix_of(hash)))
}

/// Verify with an explicit backend.
pub fn verify_with(plain: &str, hash: &str, backend: Backend) -> Result<bool, HashError> {
    match backend {
        #[cfg(feature = "password-bcrypt")]
        Backend::Bcrypt => bcrypt_verify(plain, hash),
        #[cfg(not(feature = "password-bcrypt"))]
        Backend::Bcrypt => Err(HashError::BcryptDisabled),

        #[cfg(feature = "password-argon2")]
        Backend::Argon2 => argon2_verify(plain, hash),
        #[cfg(not(feature = "password-argon2"))]
        Backend::Argon2 => Err(HashError::Argon2Disabled),
    }
}

/// Trait abstracting over password-hashing backends. Useful for
/// userland code that wants to swap algorithms at runtime without
/// hard-coding the choice at the call site.
pub trait PasswordHasher {
    /// Hash a plain-text password. Returns the serialised hash string
    /// (including the algorithm prefix).
    fn hash(&self, plain: &str) -> Result<String, HashError>;

    /// Verify a plain-text password against an existing hash.
    /// Returns `Ok(true)` on match, `Ok(false)` on mismatch,
    /// `Err(InvalidHash)` if the hash is malformed.
    fn verify(&self, plain: &str, hash: &str) -> Result<bool, HashError>;
}

/// Bcrypt backend. Default cost factor follows the `bcrypt` crate's
/// `DEFAULT_COST` (12 as of bcrypt 0.15).
#[cfg(feature = "password-bcrypt")]
#[derive(Debug, Clone, Copy, Default)]
pub struct BcryptHasher;

#[cfg(feature = "password-bcrypt")]
impl PasswordHasher for BcryptHasher {
    fn hash(&self, plain: &str) -> Result<String, HashError> {
        bcrypt_hash(plain)
    }
    fn verify(&self, plain: &str, hash: &str) -> Result<bool, HashError> {
        bcrypt_verify(plain, hash)
    }
}

/// Argon2id backend. Default parameters follow the `argon2` crate's
/// `Argon2::default()` (m=19456 KiB, t=2, p=1).
#[cfg(feature = "password-argon2")]
#[derive(Debug, Clone, Copy, Default)]
pub struct Argon2Hasher;

#[cfg(feature = "password-argon2")]
impl PasswordHasher for Argon2Hasher {
    fn hash(&self, plain: &str) -> Result<String, HashError> {
        argon2_hash(plain)
    }
    fn verify(&self, plain: &str, hash: &str) -> Result<bool, HashError> {
        argon2_verify(plain, hash)
    }
}

// ---------------------------------------------------------------------------
// Bcrypt implementation
// ---------------------------------------------------------------------------

#[cfg(feature = "password-bcrypt")]
fn bcrypt_hash(plain: &str) -> Result<String, HashError> {
    use bcrypt::{hash as bcrypt_hash_inner, DEFAULT_COST};
    bcrypt_hash_inner(plain, DEFAULT_COST).map_err(|e| HashError::InvalidHash(e.to_string()))
}

#[cfg(feature = "password-bcrypt")]
fn bcrypt_verify(plain: &str, hash: &str) -> Result<bool, HashError> {
    use bcrypt::verify as bcrypt_verify_inner;
    match bcrypt_verify_inner(plain, hash) {
        Ok(true) => Ok(true),
        Ok(false) => Ok(false),
        Err(e) => Err(HashError::InvalidHash(e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// Argon2 implementation
// ---------------------------------------------------------------------------

#[cfg(feature = "password-argon2")]
fn argon2_hash(plain: &str) -> Result<String, HashError> {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher as _, SaltString},
        Argon2,
    };
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| match e {
            argon2::password_hash::Error::Password => HashError::InvalidPassword(e.to_string()),
            _ => HashError::InvalidHash(e.to_string()),
        })
}

#[cfg(feature = "password-argon2")]
fn argon2_verify(plain: &str, hash: &str) -> Result<bool, HashError> {
    use argon2::{
        password_hash::{PasswordHash, PasswordVerifier as _},
        Argon2,
    };
    let parsed = PasswordHash::new(hash).map_err(|e| HashError::InvalidHash(e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(plain.as_bytes(), &parsed)
        .is_ok())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// First up-to-12-char slice of the hash, used to build a useful
/// `UnknownPrefix` error message without leaking the full hash.
fn prefix_of(hash: &str) -> String {
    let end = hash
        .char_indices()
        .nth(12)
        .map(|(i, _)| i)
        .unwrap_or(hash.len());
    hash[..end].to_string()
}

// ---------------------------------------------------------------------------
// Tests — round-trip hash + verify for each enabled backend.
// Gated on the per-backend features so they only run when the
// corresponding backend is compiled in.
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "password-bcrypt"))]
#[test]
fn bcrypt_round_trip() {
    let h = hash_with("hunter2", Backend::Bcrypt).expect("bcrypt hash");
    assert!(h.starts_with("$2"), "bcrypt hash must start with $2: {h}");
    assert!(verify_with("hunter2", &h, Backend::Bcrypt).expect("verify ok"));
    assert!(!verify_with("wrong", &h, Backend::Bcrypt).expect("verify ok"));
}

#[cfg(all(test, feature = "password-argon2"))]
#[test]
fn argon2_round_trip() {
    let h = hash_with("hunter2", Backend::Argon2).expect("argon2 hash");
    assert!(
        h.starts_with("$argon2"),
        "argon2 hash must start with $argon2: {h}"
    );
    assert!(verify_with("hunter2", &h, Backend::Argon2).expect("verify ok"));
    assert!(!verify_with("wrong", &h, Backend::Argon2).expect("verify ok"));
}

#[cfg(all(test, feature = "password"))]
#[test]
fn verify_any_dispatches_on_prefix() {
    let bcrypt = hash_with("hunter2", Backend::Bcrypt).expect("bcrypt");
    let argon2 = hash_with("hunter2", Backend::Argon2).expect("argon2");
    assert!(verify_any("hunter2", &bcrypt).expect("verify ok"));
    assert!(verify_any("hunter2", &argon2).expect("verify ok"));
    assert!(!verify_any("wrong", &bcrypt).expect("verify ok"));
    assert!(!verify_any("wrong", &argon2).expect("verify ok"));
}

#[cfg(all(test, feature = "password"))]
#[test]
fn verify_any_rejects_unknown_prefix() {
    let err = verify_any("hunter2", "not-a-real-hash").expect_err("should reject");
    assert!(matches!(err, HashError::UnknownPrefix(_)), "got: {err}");
}

#[cfg(all(test, feature = "password-bcrypt", not(feature = "password-argon2")))]
#[test]
fn argon2_disabled_when_only_bcrypt_enabled() {
    let err = hash_with("x", Backend::Argon2).expect_err("should be disabled");
    assert!(matches!(err, HashError::Argon2Disabled), "got: {err}");
    let err = verify_with(
        "x",
        "$argon2id$v=19$m=19456,t=2,p=1$xxx$yyy",
        Backend::Argon2,
    )
    .expect_err("should be disabled");
    assert!(matches!(err, HashError::Argon2Disabled), "got: {err}");
}

#[cfg(all(test, feature = "password-argon2", not(feature = "password-bcrypt")))]
#[test]
fn bcrypt_disabled_when_only_argon2_enabled() {
    let err = hash_with("x", Backend::Bcrypt).expect_err("should be disabled");
    assert!(matches!(err, HashError::BcryptDisabled), "got: {err}");
    let err = verify_with("x", "$2b$12$xxx", Backend::Bcrypt).expect_err("should be disabled");
    assert!(matches!(err, HashError::BcryptDisabled), "got: {err}");
}

#[test]
fn unknown_prefix_error_includes_prefix() {
    let err = verify_any("x", "plaintext-not-a-hash").expect_err("should reject");
    let msg = err.to_string();
    assert!(
        msg.contains("plaintext-not") || msg.contains("plaintext"),
        "error must include prefix: {msg}"
    );
}

#[test]
fn backend_default_is_argon2() {
    assert_eq!(Backend::default(), Backend::Argon2);
}

#[test]
fn backend_display() {
    assert_eq!(Backend::Bcrypt.to_string(), "bcrypt");
    assert_eq!(Backend::Argon2.to_string(), "argon2id");
}
