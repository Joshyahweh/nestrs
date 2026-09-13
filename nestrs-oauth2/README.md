# nestrs-oauth2

**OAuth2 client + JWKS-backed resource server + social providers + `OAuth2Guard`** for [nestrs](https://crates.io/crates/nestrs).

As a **client** it drives the authorization-code grant (with PKCE S256), the client-credentials grant, and the refresh-token grant, plus RFC 7009 token revocation. As a **resource server** it verifies incoming bearer JWTs against your IdP's JWKS endpoint with algorithm pinning and automatic key rotation. Thin **social provider wrappers** (Google / GitHub / Microsoft / Apple) pin the provider endpoints, and **`OAuth2Guard`** + `install_oauth2_middleware` turn a verified token into route protection.

**Docs:** [docs.rs/nestrs-oauth2](https://docs.rs/nestrs-oauth2) · **Guide:** [oauth2 guide](https://nestrs.mintlify.site/guides/oauth2) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

All features are **off by default** — opt in to the surface you need:

```toml
[dependencies]
nestrs-oauth2 = { version = "1.0.0", features = ["guard"] }
```

From the umbrella crate (enables `client` + `resource-server` + `guard`, re-exports as `nestrs::oauth2`):

```toml
nestrs = { version = "1.0.0", features = ["oauth2"] }
```

## Example: authorization code + PKCE (client)

```rust
use nestrs_oauth2::client::generate_pkce;
use nestrs_oauth2::{OAuth2Client, OAuth2Options};
use url::Url;

let options = OAuth2Options::new(
    "my-client-id",
    std::env::var("OAUTH2_CLIENT_SECRET")?,
    Url::parse("https://idp.example.com/authorize")?,
    Url::parse("https://idp.example.com/token")?,
    Url::parse("https://app.example.com/auth/callback")?,
);
let client = OAuth2Client::new(options)?;

// 1. Generate the PKCE pair; keep the verifier server-side.
let (challenge, verifier) = generate_pkce();

// 2. Send the user to the IdP.
let auth = client.authorize_url(&["openid", "email", "profile"], Some(&challenge), None);
// → redirect the browser to auth.url; persist auth.state + the verifier.

// 3. On the callback (after checking `state`), spend the code.
let token_set = client.exchange_code(&query.code, Some(verifier)).await?;
println!("access token: {}", token_set.access_token);
```

`exchange_code` takes the verifier **by value** — a `PkceCodeVerifier` can't be `Clone`d, so it can only be spent once. `OAuth2Options::public_client` omits the secret for PKCE-only flows (RFC 8252 §7.2). Every grant returns the same `TokenSet` shape, including the raw `id_token` / `scope` fields the underlying `oauth2` crate drops.

## Example: verify bearer JWTs (resource server)

```rust
use nestrs_oauth2::{JwtVerifier, ValidationConfig};
use url::Url;

let validation = ValidationConfig::new(jsonwebtoken::Algorithm::RS256)
    .with_issuer("https://idp.example.com")
    .with_audience("my-api")
    .with_leeway(60);

let verifier = JwtVerifier::from_url(
    Url::parse("https://idp.example.com/.well-known/jwks.json")?,
    validation,
)
.await?;

let data = verifier.verify(bearer_token).await?; // TokenData { header, claims }
```

Key rotation is handled per `kid`: lock-free cache reads on the hot path, **single-flight** refresh on a miss, `OAuth2Error::UnknownKid` only after the re-fetch. Algorithm pinning happens before key lookup, blocking the `alg=none` downgrade and cross-algorithm confusion.

## Protect nestrs routes

With the `guard` feature: `install_oauth2_middleware` verifies every incoming bearer token and stores an `OAuth2Identity` in request extensions; `#[use_guards(OAuth2Guard)]` then rejects anonymous callers with HTTP 401. The `OAuth2Principal` extractor (and the `NestApplication::use_oauth2` builder) live in the umbrella crate's `oauth2` feature, which re-exports this whole crate as `nestrs::oauth2`.

Umbrella-crate usage:

```rust
use nestrs::oauth2::OAuth2Guard;

#[controller(prefix = "me")]
#[use_guards(OAuth2Guard)]
struct MeController;

#[get("/")]
async fn me(principal: nestrs::OAuth2Principal) -> String {
    principal.subject
}
```

`OAuth2Module::register` wires the client (and optionally the verifier) into the DI registry via `useValue` — injectable as `Arc<OAuth2Client>` / `Arc<JwtVerifier>`.

## Social providers

Each wrapper pins the provider's endpoints and default scopes; `.client()` gives you the underlying `OAuth2Client`. `GoogleUser` / `GitHubUser` / `MicrosoftUser` / `AppleUser` model the userinfo shapes.

| Provider | Constructor | Default scopes |
|---|---|---|
| Google | `Google::new(client_id, client_secret, redirect_uri)` | `openid email profile` |
| GitHub | `GitHub::new(client_id, client_secret, redirect_uri)` | `read:user user:email` |
| Microsoft | `Microsoft::new(client_id, client_secret, redirect_uri, tenant)` | `openid email profile offline_access` |
| Apple | `Apple::new(client_id, client_secret_jwt, redirect_uri)` | `openid email name` |

(GitHub is plain OAuth2 — no `id_token` and no JWKS endpoint, so don't point `JwtVerifier` at it.)

## Features

| Feature | Purpose |
|---|---|
| `client` | `OAuth2Client` — authorization code (+ PKCE), client-credentials, refresh grants, revocation |
| `resource-server` | `JwksCache` + `JwtVerifier` for verifying incoming JWTs against an IdP JWKS |
| `social` | `Google` / `GitHub` / `Microsoft` / `Apple` wrappers over `OAuth2Client` |
| `guard` | `OAuth2Guard`, `install_oauth2_middleware`, `OAuth2Module` (implies `client` + `resource-server`) |
| `all` | Everything above |

## License

MIT OR Apache-2.0.
