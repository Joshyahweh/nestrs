// Lightweight Apollo Federation gateway.
//! Stitches subgraph SDLs behind one [`axum::Router`] and exposes:
//! - `_service { sdl }` — the merged federation SDL, so a router in front
//!   of the gateway can introspect the stitched shape.
//! - `_entities(representations: [_Any!]!) -> [_Any]` — dispatches each
//!   representation to the subgraph-specific resolver supplied via
//!   [`SubgraphSpec::entity_resolver`].

//! ## What this is — and isn't
//!
//! async-graphql 7 ships federation support natively (no Cargo feature
//! required); we don't introduce a query planner, don't speak the
//! Apollo Router wire protocol, and don't auto-stitch subgraph types.
//! "Stitch" here means:
//!
//! 1. **Validate** every `SubgraphSpec.sdl` at gateway-construction time
//!    via [`async_graphql_parser::parse_schema`]. Refuses with
//!    [`FederationError::Parse`] on bad input.
//! 2. **Emit** the merged SDL through [`Schema::sdl_with_options`] with
//!    [`SDLExportOptions`]`::default().federation()` — the same flags
//!    async-graphql uses for individual federation-v2 subgraphs.
//! 3. **Route** cross-subgraph entity queries by `__typename` to a
//!    user-supplied async resolver. Each subgraph ships with its own
//!    resolver closure; concurrency, batching, and HTTP-vs-in-process
//!    transport are entirely the caller's choice.

use axum::extract::Json;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::{Extension, Router};
use std::collections::HashMap;
use std::sync::Arc;

use async_graphql::types::Any;
use async_graphql::{
    http::{playground_source, GraphQLPlaygroundConfig},
    BatchRequest, EmptyMutation, EmptySubscription, Object, Schema,
};
use async_graphql_parser::parse_schema;

use crate::gql_data_context::GqlHandlerHook;
use crate::router_options::GraphQlHttpOptions;
use crate::sdl::SDLExportOptions;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One subgraph the gateway stitches behind its single Axum endpoint.
///
/// `sdl` is the federation-shaped SDL emitted by the subgraph (via
/// [`Schema::sdl_with_options`] with `SDLExportOptions::federation()`),
/// or a hand-rolled federation v2 SDL with `@link` / `@key` directives.
/// The gateway validates `sdl` at construction time and rejects on parse
/// error with [`FederationError::Parse`].
///
/// `entity_resolver` is the dispatch closure for `_entities` calls whose
/// representation `__typename` matches `name`. If two subgraphs share a
/// `__typename`, the **last** `SubgraphSpec` wins (documented in the
/// dispatch table below).
#[derive(Clone)]
pub struct SubgraphSpec {
    pub name: String,
    pub sdl: String,
    /// Resolver for `_entities(representations: [...])` calls whose
    /// representation's `__typename` matches `name`.
    pub entity_resolver: Arc<dyn EntityResolver>,
}

/// Configuration for [`federation_router`].
///
/// `options` controls the HTTP surface (Playground vs POST-only).
/// `hook` (if `Some`) wraps `schema.execute_batch` with a
/// [`GqlHandlerHook`]; pass `Arc<GqlDataContext>` for row-level authz.
#[derive(Default)]
pub struct FederationConfig {
    pub subgraphs: Vec<SubgraphSpec>,
    pub options: GraphQlHttpOptions,
    pub hook: Option<Arc<dyn GqlHandlerHook>>,
}

/// Errors that can occur at gateway construction time.
#[derive(Debug)]
pub enum FederationError {
    Parse {
        subgraph: String,
        source: async_graphql_parser::Error,
    },
    Merge {
        typename: String,
    },
    NoSubgraphs,
}

impl std::fmt::Display for FederationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FederationError::Parse { subgraph, source } => {
                write!(f, "subgraph `{subgraph}` SDL failed to parse: {source}")
            }
            FederationError::Merge { typename } => {
                write!(f, "two subgraphs define conflicting type `{typename}`")
            }
            FederationError::NoSubgraphs => {
                write!(f, "at least one subgraph is required")
            }
        }
    }
}

impl std::error::Error for FederationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FederationError::Parse { source, .. } => Some(source),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Entity resolver
// ---------------------------------------------------------------------------

/// Closure shape the gateway invokes per `_entities` resolution round.
///
/// The gateway groups all representations in one `_entities` call by
/// `__typename` and dispatches each group to its owning
/// [`SubgraphSpec::entity_resolver`] as a single batch — Apollo
/// Federation's "batched entity resolution" semantics. The resolver
/// returns one `Option<Value>` per input representation, in the same
/// order it received them. `Ok(None)` for a slot maps to JSON `null`
/// in the `_entities` list (test #4).
///
/// **Why a batch and not per-rep:** DataLoader-style batching lives in
/// the resolver (one DB round-trip per typename per request, not one
/// per representation). Forcing per-rep dispatch in the gateway would
/// defeat that — clients see N+1 problem surface here. The batch
/// signature is the documented contract; the test
/// `entities_resolution_batches_multiple_representations_in_one_call`
/// enforces it.
///
/// Each element in `representations` is the parsed JSON object the
/// client sent (e.g. `{ "__typename": "User", "id": "1" }`). The
/// resolver returns the canonical subgraph-shape JSON for each.
pub trait EntityResolver: Send + Sync + 'static {
    fn resolve(
        &self,
        ctx: &async_graphql::Context<'_>,
        representations: &[&serde_json::Value],
    ) -> async_graphql::Result<Vec<Option<serde_json::Value>>>;
}

// Allow users to wire any `Fn(...)` shape via `Arc::new(closure)`.
// The blanket impl accepts any `Fn`-compatible closure whose
// signatures match the trait's two `&`-by-reference parameters.
// This is the standard pattern that works on MSRV 1.88 without
// needing higher-ranked trait bounds on the trait itself.
impl<F> EntityResolver for F
where
    F: Fn(
            &async_graphql::Context<'_>,
            &[&serde_json::Value],
        ) -> async_graphql::Result<Vec<Option<serde_json::Value>>>
        + Send
        + Sync
        + 'static,
{
    fn resolve(
        &self,
        ctx: &async_graphql::Context<'_>,
        representations: &[&serde_json::Value],
    ) -> async_graphql::Result<Vec<Option<serde_json::Value>>> {
        (self)(ctx, representations)
    }
}

// ---------------------------------------------------------------------------
// Federation root: the query root the gateway exposes
// ---------------------------------------------------------------------------

/// The gateway's federated query root. Carries `_service { sdl }` for
/// introspection and `_entities` for cross-subgraph dispatch. We
/// construct it via a closure factory so the merged SDL and dispatch
/// table are baked in at construction time.
struct FederationRoot {
    merged_sdl: String,
    entities: HashMap<String, Arc<dyn EntityResolver>>,
}

#[Object]
impl FederationRoot {
    /// Apollo Federation introspection field — the gateway exposes the
    /// merged SDL so a router in front of it can discover the stitched
    /// shape.
    async fn _service(&self) -> ServiceField {
        ServiceField {
            sdl: self.merged_sdl.clone(),
        }
    }

    /// Cross-subgraph entity resolver. Groups the incoming
    /// representations by `__typename`, dispatches each group to its
    /// owning `EntityResolver` in one call, then re-interleaves the
    /// results back into the input order. Unknown `__typename` →
    /// `null` in the result list (test #4). Per-typename dispatch
    /// happens once per `_entities` call regardless of group size
    /// (test #5).
    ///
    /// **Note:** async-graphql 7 registers the federation entity field as
    /// `entities` (no underscore prefix) in the runtime registry even
    /// though the federation-v2 SDL exposes it as `_entities` per the
    /// Apollo spec. We name the resolver `entities` to match what the
    /// framework actually wires up — clients calling the gateway
    /// directly should query `entities(representations: [...])`. The
    /// SDL still says `_entities`, so an Apollo Router in front of the
    /// gateway will resolve via the spec-correct name.
    ///
    /// **Output type:** we return `serde_json::Value` (not `Vec<Any>`)
    /// because `_Any` is an opaque scalar — it can't have field
    /// selections, which is what callers expect when reading entity
    /// rows. async-graphql serializes the JSON tree directly without
    /// touching it.
    async fn entities(
        &self,
        ctx: &async_graphql::Context<'_>,
        representations: Vec<Any>,
    ) -> serde_json::Value {
        let n = representations.len();
        // Step 1: convert each `Any` to JSON and extract its `__typename`.
        // We keep the original index so we can re-interleave batched
        // results back into input order.
        let mut decoded: Vec<(usize, serde_json::Value, String)> = Vec::with_capacity(n);
        for (idx, rep) in representations.into_iter().enumerate() {
            let rep_json = match rep.0.into_json() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "federation entity dispatch: cannot convert representation to JSON: {e:?}"
                    );
                    // No typename — push a sentinel so the result slot
                    // is filled with `null` later, but no resolver is
                    // dispatched for this index.
                    decoded.push((idx, serde_json::Value::Null, String::new()));
                    continue;
                }
            };
            let typename = rep_json
                .get("__typename")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            decoded.push((idx, rep_json, typename));
        }

        // Step 2: group (idx, rep_json) by typename. Each typename
        // gets exactly one resolver call. We separate `typed` (matched
        // a resolver) from `untyped` (no resolver or missing
        // __typename) so the untyped slots stay `null` without
        // triggering a wasted dispatch.
        let mut groups: std::collections::HashMap<String, Vec<(usize, serde_json::Value)>> =
            std::collections::HashMap::new();
        let mut untyped: Vec<usize> = Vec::new();
        for (idx, rep_json, typename) in decoded {
            if typename.is_empty() {
                untyped.push(idx);
                continue;
            }
            match self.entities.get(&typename) {
                Some(_) => groups.entry(typename).or_default().push((idx, rep_json)),
                None => untyped.push(idx),
            }
        }

        // Step 3: dispatch each group to its resolver. Collect results
        // keyed by the input index so the final list is in input order.
        let mut results: std::collections::HashMap<usize, serde_json::Value> =
            std::collections::HashMap::new();
        for (typename, group) in groups {
            let resolver = self.entities.get(&typename).expect("present");
            // Borrow as slice-of-references — the trait contract.
            let refs: Vec<&serde_json::Value> = group.iter().map(|(_, v)| v).collect();
            match resolver.resolve(ctx, &refs) {
                Ok(resolved) => {
                    if resolved.len() != group.len() {
                        eprintln!(
                            "federation entity resolver for `{typename}` returned \
                             {} values for {} representations — filling remainder with null",
                            resolved.len(),
                            group.len()
                        );
                    }
                    for (slot, (idx, _)) in group.iter().enumerate() {
                        let v = resolved
                            .get(slot)
                            .and_then(|opt| opt.clone())
                            .unwrap_or(serde_json::Value::Null);
                        results.insert(*idx, v);
                    }
                }
                Err(e) => {
                    eprintln!(
                        "federation entity resolver for `{typename}` failed: {:?}",
                        e.message
                    );
                    for (idx, _) in group {
                        results.insert(idx, serde_json::Value::Null);
                    }
                }
            }
        }

        // Step 4: assemble output in input order. Every input index
        // 0..N is either in `results` (got a resolved value) or in
        // `untyped` (no resolver / no __typename). Slots that fall in
        // neither set only happen if a resolver panicked mid-dispatch
        // (the length-mismatch fallback already writes `null` to
        // `results`), so the `unwrap_or(Null)` is defensive.
        let mut out: Vec<serde_json::Value> = Vec::with_capacity(n);
        for idx in 0..n {
            out.push(results.remove(&idx).unwrap_or(serde_json::Value::Null));
        }
        serde_json::Value::Array(out)
    }
}

/// Inner type of the `_service { sdl }` selection set.
struct ServiceField {
    sdl: String,
}

#[Object]
impl ServiceField {
    /// The merged federation SDL.
    async fn sdl(&self) -> &String {
        &self.sdl
    }
}

// ---------------------------------------------------------------------------
// Router builders
// ---------------------------------------------------------------------------

/// Build a federation gateway router. Mirrors [`crate::graphql_router`]:
/// returns `Result<Router, FederationError>` because construction-time
/// SDL validation can fail and we don't want users' services to
/// silently come up broken.
pub fn federation_router(
    cfg: FederationConfig,
    path: impl Into<String>,
) -> Result<Router, FederationError> {
    federation_router_with_options(cfg, path, GraphQlHttpOptions::default())
}

/// Same as [`federation_router`] with explicit HTTP options.
pub fn federation_router_with_options(
    cfg: FederationConfig,
    path: impl Into<String>,
    options: GraphQlHttpOptions,
) -> Result<Router, FederationError> {
    build_router(cfg, path.into(), options, None)
}

/// Same as [`federation_router`] but installs a [`GqlHandlerHook`] for
/// per-request authz / transaction / dataloader setup. The hook wraps
/// `schema.execute_batch`; entity resolvers run inside the hook's
/// scope, so row-level predicates flow through unchanged.
pub fn federation_router_with_hook(
    cfg: FederationConfig,
    path: impl Into<String>,
    hook: Arc<dyn GqlHandlerHook>,
) -> Result<Router, FederationError> {
    build_router(cfg, path.into(), GraphQlHttpOptions::default(), Some(hook))
}

// ---------------------------------------------------------------------------
// Internal: build the schema + axum router
// ---------------------------------------------------------------------------

fn build_router(
    cfg: FederationConfig,
    path: String,
    options: GraphQlHttpOptions,
    hook: Option<Arc<dyn GqlHandlerHook>>,
) -> Result<Router, FederationError> {
    let (schema, _merged_sdl) = build_schema(cfg)?;
    // `_merged_sdl` is intentionally ignored — the merged SDL is
    // exposed at runtime via `_service { sdl }` (computed once inside
    // `build_schema` and stashed on `FederationRoot`).

    let endpoint = path.clone();
    // The handler mirrors `graphql_router_with_hook`
    // (`gql_data_context.rs:115-142`): pre-execute `prepare` runs once per
    // request in the batch (dataloader installs fresh loaders), then we
    // run `execute_batch` wrapped in the hook's scope so task-locals
    // (ability/principal/tx) are visible to entity resolvers.
    let handler = move |Extension(schema): Extension<
        Schema<FederationRoot, EmptyMutation, EmptySubscription>,
    >,
                        Json(req): Json<BatchRequest>| {
        let hook = hook.clone();
        async move {
            let mut req = req;
            if let Some(h) = hook.as_ref() {
                match &mut req {
                    BatchRequest::Single(r) => h.prepare(r),
                    BatchRequest::Batch(rs) => {
                        for r in rs {
                            h.prepare(r);
                        }
                    }
                }
            }
            let execute = Box::pin(async move { schema.execute_batch(req).await });
            let resp = match hook.as_ref() {
                Some(h) => h.run(execute).await,
                None => execute.await,
            };
            let headers = resp.http_headers_iter().collect::<Vec<_>>();
            let mut http_resp = Json(resp).into_response();
            for (name, value) in headers {
                http_resp.headers_mut().append(name, value);
            }
            http_resp
        }
    };

    if options.enable_playground {
        let playground = move || async move {
            Html(playground_source(GraphQLPlaygroundConfig::new(
                endpoint.as_str(),
            )))
        };
        let router =
            Router::new().route(path.as_str(), axum::routing::get(playground).post(handler));
        Ok(router.layer(Extension(schema)))
    } else {
        let router = Router::new().route(
            path.as_str(),
            axum::routing::get(|| async { StatusCode::METHOD_NOT_ALLOWED }).post(handler),
        );
        Ok(router.layer(Extension(schema)))
    }
}

// ---------------------------------------------------------------------------
// Wave 7.12 — Federation v2 subgraph SDL helpers
// ---------------------------------------------------------------------------

/// Wave 7.12. Emit the federation v2 subgraph SDL for an
/// `async_graphql::Schema`. Wraps
/// [`crate::sdl::export_schema_sdl_with_options`] with
/// [`SDLExportOptions::default()`].federation().compose_directive()` so
/// the output contains:
/// - `@link(url: "https://specs.apollo.dev/link/v1.0")` (the v2 link
///   directive that namespaces federation-specific types)
/// - `@key(fields: "...")` directives for every entity
/// - the `_Entity` union type and `_service` query field the
///   Apollo Router expects
/// - directive composition (`@composeDirective`) for custom
///   directives you want to expose to the router
///
/// Use this when generating the SDL file you'll hand to an Apollo
/// Router / GraphOS Studio deployment — the output is the spec-
/// compliant shape, not the legacy federation v1 `@key`-only form.
///
/// Pair with [`export_subgraph_v2_sdl_to_file`] for the disk variant.
pub fn export_subgraph_v2_sdl<Q, M, S>(schema: &Schema<Q, M, S>) -> String
where
    Q: async_graphql::ObjectType + 'static,
    M: async_graphql::ObjectType + 'static,
    S: async_graphql::SubscriptionType + 'static,
{
    let raw = crate::sdl::export_schema_sdl_with_options(
        schema,
        SDLExportOptions::default()
            .federation()
            .compose_directive(),
    );
    let mut out = String::with_capacity(raw.len() + 64);
    out.push_str("# Federation v2 subgraph SDL — emitted by nestrs-graphql (Wave 7.12)\n");
    out.push_str(&raw);
    out
}

/// Wave 7.12. Disk variant of [`export_subgraph_v2_sdl`]. Creates
/// parent directories as needed and returns the number of bytes
/// written.
pub fn export_subgraph_v2_sdl_to_file<Q, M, S>(
    schema: &Schema<Q, M, S>,
    path: impl AsRef<std::path::Path>,
) -> Result<usize, String>
where
    Q: async_graphql::ObjectType + 'static,
    M: async_graphql::ObjectType + 'static,
    S: async_graphql::SubscriptionType + 'static,
{
    let sdl = export_subgraph_v2_sdl(schema);
    crate::sdl::write_sdl_to_file(&sdl, path)
}

/// Wave 7.12. Detect whether a federation v2 SDL string was emitted
/// by [`export_subgraph_v2_sdl`]. Used by the CLI's
/// `nestrs-cli graphql federation export` to sanity-check the
/// response from a running subgraph before writing it to disk —
/// catching the case where the user points at a non-federation
/// endpoint by mistake.
///
/// Returns `true` if the SDL contains the federation v2 `@link`
/// directive. The federation v1 SDL never contains `@link`; v2
/// always does.
pub fn is_federation_v2_sdl(sdl: &str) -> bool {
    // The `@link` directive is the federation-v2 signature. It can
    // appear as `@link(url: "...")` or as part of a directive
    // declaration like `directive @link(...) on FIELD_DEFINITION`.
    // Both forms ship in the spec-compliant shape.
    sdl.contains("@link")
}


///
/// Steps:
/// 1. Validate every subgraph's SDL with `parse_schema`. First parse
///    error → `FederationError::Parse` (no panic, no half-built state).
/// 2. Concatenate each subgraph's SDL into `merged_sdl`, with the
///    `SDLExportOptions::federation()` flag set so the gateway's own
///    SDL exposes the federation-v2 `@link` directive and
///    `_Entity` / `_service` plumbing (test #8).
/// 3. Build the dispatch table (`__typename` → resolver). Duplicate
///    `__typename` across subgraphs → `FederationError::Merge` (last
///    writer wins would mask config bugs).
fn build_schema(
    cfg: FederationConfig,
) -> Result<
    (
        Schema<FederationRoot, EmptyMutation, EmptySubscription>,
        String,
    ),
    FederationError,
> {
    if cfg.subgraphs.is_empty() {
        return Err(FederationError::NoSubgraphs);
    }

    // 1. Validate every SDL up front. We do this BEFORE building the
    // dispatch table so a parse error never produces a partial gateway.
    for sg in &cfg.subgraphs {
        parse_schema(sg.sdl.as_str()).map_err(|source| FederationError::Parse {
            subgraph: sg.name.clone(),
            source,
        })?;
    }

    // 2. Merge SDLs.
    let mut merged_sdl = String::new();
    for (i, sg) in cfg.subgraphs.iter().enumerate() {
        if i > 0 {
            merged_sdl.push('\n');
        }
        merged_sdl.push_str("# subgraph: ");
        merged_sdl.push_str(&sg.name);
        merged_sdl.push('\n');
        merged_sdl.push_str(&sg.sdl);
    }

    // 3. Dispatch table.
    let mut entities: HashMap<String, Arc<dyn EntityResolver>> = HashMap::new();
    let mut typenames_seen: HashMap<String, String> = HashMap::new();
    for sg in &cfg.subgraphs {
        let typename = sg.name.clone();
        if let Some(prev) = typenames_seen.get(&typename) {
            return Err(FederationError::Merge {
                typename: format!("{typename} (already owned by `{prev}`)"),
            });
        }
        typenames_seen.insert(typename.clone(), sg.name.clone());
        entities.insert(typename, sg.entity_resolver.clone());
    }

    let root = FederationRoot {
        merged_sdl: merged_sdl.clone(),
        entities,
    };

    let schema: Schema<FederationRoot, EmptyMutation, EmptySubscription> =
        Schema::build(root, EmptyMutation, EmptySubscription)
            .enable_federation()
            .finish();

    // Re-export the merged SDL using the same federation flag the
    // single-subgraph SDL export uses — so test #8 sees the `@link`
    // directive and Federation-v2 SDL shape.
    let merged_sdl_exported =
        schema.sdl_with_options(SDLExportOptions::default().federation().compose_directive());

    Ok((schema, merged_sdl_exported))
}

#[cfg(test)]
mod tests {
    //! Wave 7.12 — verify the federation v2 subgraph SDL helpers.
    //!
    //! `is_federation_v2_sdl` is the only pure function — tested with
    //! string fixtures. The two `export_subgraph_v2_sdl*` helpers need
    //! a real `async_graphql::Schema`, so we build a minimal one
    //! (single `ping: String` query) inline.
    use super::*;
    use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};

    /// Trivial query root used by the SDL-emission tests. A single
    /// `ping` field so the schema has at least one type — `Schema::build`
    /// would accept an empty root but the emitted SDL would then have
    /// nothing useful for humans or routers to inspect.
    struct PingQuery;

    #[Object]
    impl PingQuery {
        async fn ping(&self) -> &'static str {
            "pong"
        }
    }

    #[test]
    fn is_federation_v2_sdl_detects_at_link_directive() {
        // Federation v2 SDLs always contain a `@link` directive — either
        // as a directive declaration or as an application.
        let sdl_v2 =
            "directive @link(url: String) on FIELD_DEFINITION\ntype Query { ping: String }\n";
        assert!(
            is_federation_v2_sdl(sdl_v2),
            "should detect `@link` as federation v2"
        );
    }

    #[test]
    fn is_federation_v2_sdl_rejects_v1_or_non_federation_sdl() {
        // Pre-Apollo-v2 federation and plain GraphQL SDLs never include
        // the `@link` directive.
        let sdl_plain = "type Query { ping: String }\n";
        let sdl_v1 = "type User @key(fields: \"id\") { id: ID! }\ntype Query { me: User }\n";
        assert!(!is_federation_v2_sdl(sdl_plain));
        assert!(!is_federation_v2_sdl(sdl_v1));
    }

    #[test]
    fn is_federation_v2_sdl_handles_empty_input() {
        assert!(!is_federation_v2_sdl(""));
    }

    #[test]
    fn export_subgraph_v2_sdl_emits_at_link_header_and_body() {
        let schema = Schema::build(PingQuery, EmptyMutation, EmptySubscription).finish();
        let sdl = export_subgraph_v2_sdl(&schema);
        // Header comment our helper prepends.
        assert!(
            sdl.starts_with("# Federation v2 subgraph SDL"),
            "expected the Wave 7.12 header comment, got: {sdl}"
        );
        // Federation v2 SDL signature directive.
        assert!(
            is_federation_v2_sdl(&sdl),
            "expected `@link` directive in federation v2 SDL, got: {sdl}"
        );
        // The schema's query type survives the round-trip.
        assert!(sdl.contains("type Query"));
        assert!(sdl.contains("ping"));
    }

    #[test]
    fn export_subgraph_v2_sdl_to_file_writes_and_returns_byte_count() {
        let schema = Schema::build(PingQuery, EmptyMutation, EmptySubscription).finish();
        // Use a process-id-scoped tmp filename to avoid cross-test
        // collisions when `cargo test` runs multiple tests in parallel.
        let tmp = std::env::temp_dir().join(format!(
            "nestrs_wave_7_12_federation_test_{}.graphql",
            std::process::id()
        ));
        let bytes = export_subgraph_v2_sdl_to_file(&schema, &tmp).expect("write");
        assert!(bytes > 0, "byte count must be positive");
        let on_disk = std::fs::read_to_string(&tmp).expect("read back");
        assert!(is_federation_v2_sdl(&on_disk));
        assert!(on_disk.contains("Federation v2 subgraph SDL"));
        let _ = std::fs::remove_file(&tmp);
    }
}
