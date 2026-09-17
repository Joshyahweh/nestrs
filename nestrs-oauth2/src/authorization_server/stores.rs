//! Pluggable persistence for the authorization server.
//!
//! Everything the server needs to remember — registered clients,
//! authorization codes, refresh tokens, revoked access-token ids — lives
//! behind these four small traits. Ship the in-memory implementations
//! below for dev, tests, and small single-process deployments; back them
//! with Postgres/Redis (or any shared store) for multi-instance
//! production. The traits are `async_trait`-based, so a networked store
//! fits without blocking the request path.
//!
//! Two invariants every implementation MUST uphold, because the
//! security model leans on them:
//!
//! 1. **Hashed keys.** Codes and refresh tokens are always stored
//!    **keyed by the SHA-256 hex of the plaintext** — the server never
//!    hands a store a raw credential. A store compromise then yields
//!    unredeemable digests, not tokens.
//! 2. **Atomic consumption.** `consume` (codes and refresh) must
//!    transition fresh→used in one atomic step; two concurrent
//!    redemptions of the same credential must produce exactly one
//!    `Fresh` outcome.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use super::model::{OAuth2ClientRecord, StoredAuthorizationCode, StoredRefreshToken};

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Registered-client lookup.
#[async_trait]
pub trait ClientStore: Send + Sync {
    /// The record for `client_id`, or `None` when unknown.
    async fn find(&self, client_id: &str) -> Option<OAuth2ClientRecord>;
}

/// Outcome of redeeming an authorization code at the token endpoint.
#[derive(Debug)]
pub enum ConsumeCode {
    /// First redemption — the caller proceeds (and the store has
    /// already marked the code `used`).
    Fresh(StoredAuthorizationCode),
    /// The code was already used — a replay. Carries the `family_id`
    /// the code seeded so the caller can revoke it.
    Reused { family_id: String },
    /// Unknown or pruned (expired) code.
    Missing,
}

/// Single-use authorization-code storage, keyed by `sha256_hex(code)`.
#[async_trait]
pub trait CodeStore: Send + Sync {
    /// Store a freshly minted code (always `used: false`).
    async fn save(&self, code_hash: &str, code: StoredAuthorizationCode);

    /// Atomically redeem: fresh → mark used + return `Fresh`; already
    /// used → `Reused` (replay); absent/expired → `Missing`.
    async fn consume(&self, code_hash: &str) -> ConsumeCode;
}

/// Outcome of consuming a refresh token (rotation).
#[derive(Debug)]
pub enum ConsumeRefresh {
    /// The token was fresh and is now marked rotated — the caller may
    /// issue a successor in the same family.
    Fresh(StoredRefreshToken),
    /// The token was already rotated or revoked — **reuse detected**.
    /// Carries the family so the caller can revoke it.
    Reused { family_id: String },
    /// Unknown or pruned token.
    Missing,
}

/// Refresh-token storage, keyed by `sha256_hex(token)`.
#[async_trait]
pub trait RefreshTokenStore: Send + Sync {
    /// Store a freshly minted refresh token (`rotated: false`,
    /// `revoked: false`).
    async fn save(&self, token_hash: &str, token: StoredRefreshToken);

    /// Non-consuming lookup — used by introspection. Whether a
    /// rotated/revoked/expired record is returned is implementation
    /// detail; the caller applies the flags.
    async fn find(&self, token_hash: &str) -> Option<StoredRefreshToken>;

    /// Atomically consume for rotation: fresh → mark rotated + return
    /// `Fresh`; rotated/revoked → `Reused` (reuse detection); absent →
    /// `Missing`.
    async fn consume(&self, token_hash: &str) -> ConsumeRefresh;

    /// Revoke an entire rotation family (all tokens with `family_id`).
    /// Returns the records revoked in this call — the caller feeds
    /// their `access_jti`/`access_exp` into the revocation list so
    /// outstanding access JWTs die with the family.
    async fn revoke_family(&self, family_id: &str) -> Vec<StoredRefreshToken>;
}

/// Revocation list for issued access-token `jti`s (RFC 7009 support).
#[async_trait]
pub trait AccessTokenRevocationList: Send + Sync {
    /// Record `jti` as revoked until `expires_at` (entries may be
    /// pruned after the token would have expired anyway).
    async fn revoke(&self, jti: &str, expires_at: i64);

    /// Whether `jti` is revoked.
    async fn is_revoked(&self, jti: &str) -> bool;
}

/// The full set of stores the server runs on.
#[derive(Clone)]
pub struct AuthorizationServerStores {
    pub clients: Arc<dyn ClientStore>,
    pub codes: Arc<dyn CodeStore>,
    pub refresh_tokens: Arc<dyn RefreshTokenStore>,
    pub revocations: Arc<dyn AccessTokenRevocationList>,
}

impl AuthorizationServerStores {
    /// All four in-memory stores, seeded with `clients`.
    ///
    /// The in-memory stores prune expired entries on write, so memory
    /// is bounded by the active set — but state lives and dies with the
    /// process. Use for dev, tests, and single-instance deployments;
    /// point the fields at DB-backed stores for production.
    pub fn in_memory(clients: Vec<OAuth2ClientRecord>) -> Self {
        Self {
            clients: Arc::new(InMemoryClientStore::from(clients)),
            codes: Arc::new(InMemoryCodeStore::default()),
            refresh_tokens: Arc::new(InMemoryRefreshTokenStore::default()),
            revocations: Arc::new(InMemoryRevocationList::default()),
        }
    }
}

// Manual Debug on every store: a derived one would dump stored records
// into any `%{:?}` log line. Entry counts are safe to show.

/// In-memory [`ClientStore`] — a lookup table of pre-registered records.
pub struct InMemoryClientStore {
    clients: RwLock<HashMap<String, OAuth2ClientRecord>>,
}

impl std::fmt::Debug for InMemoryClientStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryClientStore")
            .field("entries", &self.clients.read().len())
            .finish()
    }
}

impl From<Vec<OAuth2ClientRecord>> for InMemoryClientStore {
    fn from(clients: Vec<OAuth2ClientRecord>) -> Self {
        let map = clients
            .into_iter()
            .map(|c| (c.client_id.clone(), c))
            .collect();
        Self {
            clients: RwLock::new(map),
        }
    }
}

#[async_trait]
impl ClientStore for InMemoryClientStore {
    async fn find(&self, client_id: &str) -> Option<OAuth2ClientRecord> {
        self.clients.read().get(client_id).cloned()
    }
}

/// In-memory [`CodeStore`]. Expired entries (used or not) are pruned on
/// every `save` — after expiry a code is unconditionally `invalid_grant`
/// either way, and its family is long dead.
pub struct InMemoryCodeStore {
    codes: RwLock<HashMap<String, StoredAuthorizationCode>>,
}

impl std::fmt::Debug for InMemoryCodeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryCodeStore")
            .field("entries", &self.codes.read().len())
            .finish()
    }
}

impl Default for InMemoryCodeStore {
    fn default() -> Self {
        Self {
            codes: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl CodeStore for InMemoryCodeStore {
    async fn save(&self, code_hash: &str, code: StoredAuthorizationCode) {
        let mut codes = self.codes.write();
        let now = now_epoch();
        codes.retain(|_, c| c.expires_at > now);
        codes.insert(code_hash.to_string(), code);
    }

    async fn consume(&self, code_hash: &str) -> ConsumeCode {
        let mut codes = self.codes.write();
        match codes.get_mut(code_hash) {
            None => ConsumeCode::Missing,
            Some(code) => {
                if code.used {
                    ConsumeCode::Reused {
                        family_id: code.family_id.clone(),
                    }
                } else if code.expires_at <= now_epoch() {
                    ConsumeCode::Missing
                } else {
                    code.used = true;
                    ConsumeCode::Fresh(code.clone())
                }
            }
        }
    }
}

/// In-memory [`RefreshTokenStore`]. Expired entries are pruned on
/// `save`; revoked-but-unexpired entries stay (they carry the family
/// bookkeeping reuse detection needs).
pub struct InMemoryRefreshTokenStore {
    tokens: RwLock<HashMap<String, StoredRefreshToken>>,
}

impl std::fmt::Debug for InMemoryRefreshTokenStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryRefreshTokenStore")
            .field("entries", &self.tokens.read().len())
            .finish()
    }
}

impl Default for InMemoryRefreshTokenStore {
    fn default() -> Self {
        Self {
            tokens: RwLock::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl RefreshTokenStore for InMemoryRefreshTokenStore {
    async fn save(&self, token_hash: &str, token: StoredRefreshToken) {
        let mut tokens = self.tokens.write();
        let now = now_epoch();
        tokens.retain(|_, t| t.expires_at > now);
        tokens.insert(token_hash.to_string(), token);
    }

    async fn find(&self, token_hash: &str) -> Option<StoredRefreshToken> {
        self.tokens.read().get(token_hash).cloned()
    }

    async fn consume(&self, token_hash: &str) -> ConsumeRefresh {
        let mut tokens = self.tokens.write();
        match tokens.get_mut(token_hash) {
            None => ConsumeRefresh::Missing,
            Some(token) => {
                if token.rotated || token.revoked {
                    ConsumeRefresh::Reused {
                        family_id: token.family_id.clone(),
                    }
                } else if token.expires_at <= now_epoch() {
                    ConsumeRefresh::Missing
                } else {
                    token.rotated = true;
                    ConsumeRefresh::Fresh(token.clone())
                }
            }
        }
    }

    async fn revoke_family(&self, family_id: &str) -> Vec<StoredRefreshToken> {
        let mut tokens = self.tokens.write();
        let mut revoked = Vec::new();
        for token in tokens.values_mut() {
            if token.family_id == family_id && !token.revoked {
                token.revoked = true;
                revoked.push(token.clone());
            }
        }
        revoked
    }
}

/// In-memory [`AccessTokenRevocationList`] — `jti` → `exp`, pruned
/// once entries outlive their token.
pub struct InMemoryRevocationList {
    revoked: RwLock<HashMap<String, i64>>,
}

impl std::fmt::Debug for InMemoryRevocationList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryRevocationList")
            .field("entries", &self.revoked.read().len())
            .finish()
    }
}

impl Default for InMemoryRevocationList {
    fn default() -> Self {
        Self {
            revoked: RwLock::new(HashMap::new()),
        }
    }
}

const REVOCATION_PRUNE_THRESHOLD: usize = 4096;

#[async_trait]
impl AccessTokenRevocationList for InMemoryRevocationList {
    async fn revoke(&self, jti: &str, expires_at: i64) {
        let mut revoked = self.revoked.write();
        revoked.insert(jti.to_string(), expires_at);
        if revoked.len() > REVOCATION_PRUNE_THRESHOLD {
            let now = now_epoch();
            revoked.retain(|_, exp| *exp > now);
        }
    }

    async fn is_revoked(&self, jti: &str) -> bool {
        let revoked = self.revoked.read();
        matches!(revoked.get(jti), Some(exp) if *exp > now_epoch())
    }
}
