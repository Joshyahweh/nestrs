//! Wave 7.13 — integration tests for `nestrs-oauth2::password`.
//!
//! Unit tests for the bcrypt / argon2 backends, the `verify_any`
//! prefix detector, and the disabled-backend error paths already
//! live inside `src/password.rs` (gate-conditioned on the
//! per-backend features). This file adds integration coverage
//! that touches the public re-exports and the `#[derive(HashOnNew)]`
//! macro expansion end-to-end.

#![cfg(any(feature = "password-bcrypt", feature = "password-argon2"))]

use nestrs_oauth2::{
    hash, hash_with, verify, verify_any, verify_with, Argon2Hasher, Backend, BcryptHasher,
    HashError, PasswordHasher,
};

// ---------------------------------------------------------------------------
// Round-trip — confirm each backend produces a verifiable hash.
// ---------------------------------------------------------------------------

#[cfg(feature = "password-bcrypt")]
#[test]
fn bcrypt_round_trip_via_public_api() {
    let h = hash_with("hunter2", Backend::Bcrypt).expect("bcrypt hash");
    assert!(h.starts_with("$2"), "bcrypt hash must start with $2: {h}");
    assert!(verify_with("hunter2", &h, Backend::Bcrypt).expect("verify ok"));
    assert!(!verify_with("wrong", &h, Backend::Bcrypt).expect("verify ok"));
}

#[cfg(feature = "password-argon2")]
#[test]
fn argon2_round_trip_via_public_api() {
    let h = hash_with("hunter2", Backend::Argon2).expect("argon2 hash");
    assert!(
        h.starts_with("$argon2"),
        "argon2 hash must start with $argon2: {h}"
    );
    assert!(verify_with("hunter2", &h, Backend::Argon2).expect("verify ok"));
    assert!(!verify_with("wrong", &h, Backend::Argon2).expect("verify ok"));
}

#[cfg(feature = "password")]
#[test]
fn default_hash_uses_argon2() {
    let h = hash("hunter2").expect("default hash");
    assert!(
        h.starts_with("$argon2"),
        "default backend must be argon2: {h}"
    );
}

// ---------------------------------------------------------------------------
// `verify_any` prefix dispatch — public-API form.
// ---------------------------------------------------------------------------

#[cfg(feature = "password")]
#[test]
fn verify_any_dispatches_on_prefix() {
    let bcrypt = hash_with("hunter2", Backend::Bcrypt).expect("bcrypt");
    let argon2 = hash_with("hunter2", Backend::Argon2).expect("argon2");
    assert!(verify_any("hunter2", &bcrypt).expect("verify ok"));
    assert!(verify_any("hunter2", &argon2).expect("verify ok"));
    assert!(!verify_any("wrong", &bcrypt).expect("verify ok"));
    assert!(!verify_any("wrong", &argon2).expect("verify ok"));
}

#[cfg(feature = "password")]
#[test]
fn verify_any_unknown_prefix_returns_error() {
    let err = verify("hunter2", "not-a-real-hash").expect_err("should reject");
    assert!(matches!(err, HashError::UnknownPrefix(_)), "got: {err}");
}

// ---------------------------------------------------------------------------
// Disabled-backend error paths. These only run when only ONE backend
// is compiled in — the dual-backend build always exercises both,
// so we don't repeat the disabled-path tests in the full build.
// ---------------------------------------------------------------------------

#[cfg(all(feature = "password-bcrypt", not(feature = "password-argon2")))]
#[test]
fn argon2_disabled_when_only_bcrypt_enabled() {
    let err = hash_with("x", Backend::Argon2).expect_err("should be disabled");
    assert!(matches!(err, HashError::Argon2Disabled), "got: {err}");
}

#[cfg(all(feature = "password-argon2", not(feature = "password-bcrypt")))]
#[test]
fn bcrypt_disabled_when_only_argon2_enabled() {
    let err = hash_with("x", Backend::Bcrypt).expect_err("should be disabled");
    assert!(matches!(err, HashError::BcryptDisabled), "got: {err}");
}

// ---------------------------------------------------------------------------
// `PasswordHasher` trait — exercise both concrete impls and the trait
// object form. Only runs when both backends are compiled in (the
// `password` umbrella feature).
// ---------------------------------------------------------------------------

#[cfg(feature = "password")]
#[test]
fn password_hasher_trait_round_trip_for_each_backend() {
    let bcrypt: Box<dyn PasswordHasher> = Box::new(BcryptHasher);
    let argon2: Box<dyn PasswordHasher> = Box::new(Argon2Hasher);
    for hasher in [&bcrypt, &argon2] {
        let h = hasher.hash("hunter2").expect("hash");
        assert!(hasher.verify("hunter2", &h).expect("verify ok"));
        assert!(!hasher.verify("wrong", &h).expect("verify ok"));
    }
}

// ---------------------------------------------------------------------------
// `#[derive(HashOnNew)]` — end-to-end macro expansion. The macro
// generates `new_with_hashed(...)` that calls
// `nestrs_oauth2::password::hash(...)` on each `#[hash]` field.
// We exercise it through the re-exported derive and confirm the
// stored field is a verifiable hash, not the plaintext.
// ---------------------------------------------------------------------------

#[cfg(feature = "password")]
#[test]
fn hash_on_new_derive_hashes_marked_fields() {
    use nestrs_oauth2::HashOnNew;

    #[derive(HashOnNew)]
    pub struct UserRow {
        pub email: String,
        #[hash]
        pub password: String,
        pub role: String,
    }

    let row = UserRow::new_with_hashed(
        "alice@example.com".to_string(),
        "hunter2".to_string(),
        "admin".to_string(),
    );

    // Plain fields pass through verbatim.
    assert_eq!(row.email, "alice@example.com");
    assert_eq!(row.role, "admin");

    // Hashed field is stored as a real hash, not the plaintext.
    assert_ne!(
        row.password, "hunter2",
        "HashOnNew must NOT store plaintext"
    );
    assert!(
        row.password.starts_with("$argon2"),
        "HashOnNew must use argon2 by default: {}",
        row.password
    );

    // The stored hash verifies back to the original plaintext.
    assert!(verify("hunter2", &row.password).expect("verify ok"));
    assert!(!verify("wrong", &row.password).expect("verify ok"));
}

#[cfg(feature = "password")]
#[test]
fn hash_on_new_derive_supports_multiple_hashed_fields() {
    use nestrs_oauth2::HashOnNew;

    #[derive(HashOnNew)]
    pub struct Creds {
        #[hash]
        pub password: String,
        #[hash]
        pub api_token: String,
    }

    let creds = Creds::new_with_hashed("hunter2".to_string(), "tok_abc".to_string());
    assert_ne!(creds.password, "hunter2");
    assert_ne!(creds.api_token, "tok_abc");
    assert!(creds.password.starts_with("$argon2"));
    assert!(creds.api_token.starts_with("$argon2"));
    assert!(verify("hunter2", &creds.password).unwrap());
    assert!(verify("tok_abc", &creds.api_token).unwrap());
}

#[cfg(feature = "password")]
#[test]
fn hash_on_new_derive_preserves_struct_field_order() {
    use nestrs_oauth2::HashOnNew;

    // Field ordering must match the constructor parameter order.
    // If the derive accidentally reorders, this test catches it.
    #[derive(HashOnNew)]
    pub struct Ordered {
        pub a: String,
        #[hash]
        pub b: String,
        pub c: String,
        #[hash]
        pub d: String,
    }

    let row = Ordered::new_with_hashed(
        "A".to_string(),
        "B-plain".to_string(),
        "C".to_string(),
        "D-plain".to_string(),
    );
    assert_eq!(row.a, "A");
    assert!(
        row.b.starts_with("$argon2"),
        "b should be hashed: {}",
        row.b
    );
    assert_eq!(row.c, "C");
    assert!(
        row.d.starts_with("$argon2"),
        "d should be hashed: {}",
        row.d
    );
}
