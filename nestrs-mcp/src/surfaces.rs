//! MCP protocol surfaces beyond tools: prompts, resources,
//! resource templates, and completion.
//!
//! The rmcp 3.1 SDK ships a built-in `Router` that aggregates
//! `Tool` + `Prompt` routes, but the rest of the protocol
//! surfaces (resources, resource templates, completion) need to
//! be wired by hand. This module is the hand-wiring.
//!
//! The shape is a single `McpSurfaces` value the
//! `NestrsMcpServer` carries in a `Mutex`. Each `ServerHandler`
//! override (`list_prompts`, `get_prompt`, `list_resources`,
//! `list_resource_templates`, `read_resource`, `complete`)
//! reads the corresponding field. Tests construct an
//! `McpSurfaces`, install it on a `NestrsMcpServer`, and call
//! the overrides directly through the public re-exports.
//!
//! ## Why a `Mutex` not `RwLock`
//!
//! Reads (every MCP request is a read) dominate writes
//! (`register_prompt` happens once at startup). The lock is
//! never held across an `.await`, so the contention is bounded
//! to the short critical section of "clone the relevant vec out
//! and return it." A `Mutex<Vec<T>>` is the smallest thing that
//! works and avoids the write-preference foot-gun of `RwLock`
//! under bursty read traffic.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

#[cfg(feature = "elicitation")]
use rmcp::model::ElicitResult;
use rmcp::model::{
    CacheScope, CallToolRequestParams, CompleteRequestParams, CompleteResult, CompletionInfo,
    ContentBlock, ElicitRequestParams, ElicitationSchema, GetPromptRequestParams,
    GetPromptResponse, GetPromptResult, InputRequest, InputRequests, InputRequiredResult,
    ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, Prompt, PromptArgument,
    PromptMessage, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Request,
    Resource, ResourceContents, ResourceTemplate, Role, SubscriptionFilter,
};
use rmcp::service::SubscriptionSink;
use rmcp::ErrorData;

/// A user-supplied handler for one prompt. The argument map is
/// the JSON object the client sent (`{"topic": "rust", ...}`).
/// Returning `Err(_)` becomes a 500-style MCP error.
pub type PromptHandler = Box<
    dyn Fn(serde_json::Map<String, serde_json::Value>) -> Result<Vec<PromptMessage>, String>
        + Send
        + Sync
        + 'static,
>;

/// A user-supplied handler for one resource. The URI is the
/// raw string the client sent (e.g. `"file:///x"` or
/// `"nestrs://docs/router"`). The handler returns the contents
/// to ship back.
pub type ResourceHandler =
    Box<dyn Fn(&str) -> Result<Vec<ResourceContents>, String> + Send + Sync + 'static>;

/// A user-supplied handler for one resource template. The URI
/// is the *expanded* URI (e.g. `"nestrs://docs/router"` after
/// the client filled in the template's `{name}`). The handler
/// returns the contents.
pub type ResourceTemplateHandler =
    Box<dyn Fn(&str) -> Result<Vec<ResourceContents>, String> + Send + Sync + 'static>;

/// A user-supplied completion handler. Receives the partial
/// argument value the client is typing, plus the full request
/// (which carries the `ref` so the handler knows which
/// prompt/resource is being completed).
///
/// The return is the completion list. The MCP spec limits
/// completions to 100 items; we don't enforce that here — the
/// SDK or transport layer does, if it cares.
pub type CompleteHandler = Box<
    dyn Fn(&str, &CompleteRequestParams) -> Result<Vec<String>, String> + Send + Sync + 'static,
>;

/// A user-supplied elicitation handler. Receives the
/// `ElicitRequestParams` the server wants to send to the client
/// plus the full `RequestContext<RoleServer>` (so the handler can
/// inspect capabilities, forward to another service, or call
/// `context.peer.elicit::<T>(...)` to actually do the
/// hand-off). Returns the `ElicitResult` from the client.
///
/// Wrapped in `Arc` so `McpSurfaces` (and therefore
/// `NestrsMcpServer`) can be `Clone` — `Box<dyn Fn>` isn't.
///
/// The handler is sync — the `NestrsMcpServer::elicit` wrapper
/// sits in front and is async. Keeping the handler type sync
/// matches the existing surfaces pattern.
#[cfg(feature = "elicitation")]
pub type ElicitationHandler = std::sync::Arc<
    dyn Fn(
            ElicitRequestParams,
            rmcp::service::RequestContext<rmcp::service::RoleServer>,
        ) -> Result<ElicitResult, String>
        + Send
        + Sync
        + 'static,
>;

/// Cache hints attached to list/read results (SEP-2549). The hints
/// travel as top-level `ttlMs` / `cacheScope` fields on the wire —
/// not inside `_meta` — so a client can skip re-fetching a surface
/// that hasn't changed.
///
/// Opt-in and global: set once via [`McpSurfaces::with_cache_hints`]
/// and every `list_*` / `read_resource` response carries them. Only
/// emitted to peers on protocol ≥ 2026-07-28 (the server override
/// does that gating, since it needs the `RequestContext`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheHints {
    /// How long the client may cache the response, in milliseconds.
    pub ttl_ms: u64,
    /// Who may cache it. `Public` (default) allows intermediaries;
    /// `Private` restricts to the requesting user's client.
    pub cache_scope: CacheScope,
}

impl CacheHints {
    pub fn new(ttl_ms: u64, cache_scope: CacheScope) -> Self {
        Self {
            ttl_ms,
            cache_scope,
        }
    }
}

/// The `.with_ttl_ms` / `.with_cache_scope` builder pair is stamped
/// by the `paginated_result!` macro onto every list result and is
/// also hand-written on `ReadResourceResult`. This trait lets one
/// helper attach hints to all of them.
pub(crate) trait WithCacheHints: Sized {
    fn with_hints(self, hints: &CacheHints) -> Self;
}

macro_rules! impl_with_cache_hints {
    ($($ty:ty),* $(,)?) => {
        $(
            impl WithCacheHints for $ty {
                fn with_hints(self, hints: &CacheHints) -> Self {
                    self.with_ttl_ms(hints.ttl_ms)
                        .with_cache_scope(hints.cache_scope)
                }
            }
        )*
    };
}

impl_with_cache_hints!(
    ListPromptsResult,
    ListResourcesResult,
    ListResourceTemplatesResult,
    ReadResourceResult,
);

/// Attach `hints` to `result` if the user configured any and the
/// peer's protocol version supports cache hints (≥ 2026-07-28).
/// The protocol gate is passed as a bool because it comes from the
/// `RequestContext` in the `ServerHandler` override, which this
/// helper (and its tests) can't construct.
pub(crate) fn attach_cache_hints<R: WithCacheHints>(
    result: R,
    hints: Option<CacheHints>,
    protocol_supported: bool,
) -> R {
    match (hints, protocol_supported) {
        (Some(h), true) => result.with_hints(&h),
        _ => result,
    }
}

/// One registered prompt: metadata + handler.
struct PromptEntry {
    prompt: Prompt,
    handler: PromptHandler,
}

/// One registered resource: metadata + handler.
struct ResourceEntry {
    resource: Resource,
    handler: ResourceHandler,
}

/// One registered resource template: metadata + handler. The
/// handler sees the expanded URI.
struct ResourceTemplateEntry {
    template: ResourceTemplate,
    handler: ResourceTemplateHandler,
}

/// The bundle of protocol surfaces attached to a
/// `NestrsMcpServer`. Clone-cheap (just an `Arc` bump).
#[derive(Default, Clone)]
pub struct McpSurfaces {
    inner: std::sync::Arc<Mutex<SurfacesInner>>,
}

#[derive(Default)]
struct SurfacesInner {
    prompts: HashMap<String, PromptEntry>,
    resources: HashMap<String, ResourceEntry>,
    /// The first resource template registered under a given
    /// URI-template string wins. (A more sophisticated server
    /// could keep all and dispatch by longest match; for v1
    /// the simple "last write wins" / "first write wins" is
    /// fine — the user is expected to register each template
    /// once.)
    resource_templates: HashMap<String, ResourceTemplateEntry>,
    /// The complete handler. Only one is supported (the
    /// protocol has one `completion/complete` endpoint, not
    /// one per surface). The handler reads `CompleteRequestParams::ref_`
    /// to know which prompt or resource is being completed.
    complete: Option<CompleteHandler>,
    /// The set of resources any client may ask to subscribe to
    /// (via `subscriptions/listen` on protocol ≥ 2026-07-28, or
    /// the legacy `resources/subscribe` on older peers). Empty
    /// means "this server has no subscribable resources." The
    /// `accepted_subscription_filter` override intersects the
    /// client's requested filter against this set.
    subscribable_resources: HashSet<String>,
    /// Live `SubscriptionSink`s, keyed by subscription id. The
    /// `listen` override inserts on entry, removes on cancel
    /// (via `SubscriptionContext::cancelled()`). The
    /// resource-write path can fan out an update by calling
    /// `subscription_sinks_for(uri)` and invoking
    /// `sink.notify_resource_updated(uri)` on each.
    subscription_sinks: HashMap<String, SubscriptionSink>,
    /// Cache hints (SEP-2549) attached to every list/read result
    /// when `Some`. Gated per-request on the peer's protocol
    /// version by the `ServerHandler` overrides.
    cache_hints: Option<CacheHints>,
    /// Elicitation handler (SEP-1034). Only one is supported
    /// per `NestrsMcpServer` — a server typically asks one
    /// question at a time, and the protocol doesn't expose
    /// per-tool handlers. The handler receives the
    /// `ElicitRequestParams` and a `RequestContext<RoleServer>`
    /// so it can call `context.peer.elicit::<T>(...)`.
    #[cfg(feature = "elicitation")]
    elicitation_handler: Option<ElicitationHandler>,
}

impl std::fmt::Debug for McpSurfaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        f.debug_struct("McpSurfaces")
            .field("prompts", &g.prompts.keys().collect::<Vec<_>>())
            .field("resources", &g.resources.keys().collect::<Vec<_>>())
            .field(
                "resource_templates",
                &g.resource_templates.keys().collect::<Vec<_>>(),
            )
            .field("complete", &g.complete.as_ref().map(|_| "Some(...)"))
            .field(
                "subscribable_resources",
                &g.subscribable_resources.iter().collect::<Vec<_>>(),
            )
            .field("subscription_sinks", &g.subscription_sinks.len())
            .field("cache_hints", &g.cache_hints)
            .finish()
    }
}

impl McpSurfaces {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a prompt. `name` is the prompt's identifier
    /// (used by `prompts/get`). `description` is optional.
    /// `arguments` is the argument schema (clients use this to
    /// prompt the model). `handler` is the body.
    pub fn register_prompt<H>(
        self,
        name: impl Into<String>,
        description: Option<String>,
        arguments: Vec<PromptArgument>,
        handler: H,
    ) -> Self
    where
        H: Fn(serde_json::Map<String, serde_json::Value>) -> Result<Vec<PromptMessage>, String>
            + Send
            + Sync
            + 'static,
    {
        let name = name.into();
        let mut prompt = Prompt::new(
            name.clone(),
            description,
            if arguments.is_empty() {
                None
            } else {
                Some(arguments)
            },
        );
        // The `Prompt::new` constructor takes Option<D> for
        // description; we want a more ergonomic builder that
        // also sets `title` to a sane default if not provided.
        if prompt.title.is_none() {
            prompt.title = Some(name.clone());
        }
        let entry = PromptEntry {
            prompt,
            handler: Box::new(handler),
        };
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .prompts
            .insert(name, entry);
        self
    }

    /// Register a resource. `uri` is the resource's identifier
    /// (e.g. `"file:///Cargo.toml"`). `handler` is the body.
    pub fn register_resource<H>(self, resource: Resource, handler: H) -> Self
    where
        H: Fn(&str) -> Result<Vec<ResourceContents>, String> + Send + Sync + 'static,
    {
        let uri = resource.uri.clone();
        let entry = ResourceEntry {
            resource,
            handler: Box::new(handler),
        };
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .resources
            .insert(uri, entry);
        self
    }

    /// Register a resource template. `uri_template` is an
    /// RFC 6570 URI template (e.g.
    /// `"nestrs://docs/{name}"`). The handler receives the
    /// *expanded* URI — the same shape a `register_resource`
    /// handler would see. We don't expand templates
    /// ourselves; the client (per spec) is expected to
    /// expand and then call `resources/read` on the
    /// resulting URI, and the server side either matches a
    /// static `register_resource` or a `register_template`
    /// in that order. Since we override `read_resource` to
    /// check both, callers that only register templates
    /// never see the static resource list.
    pub fn register_resource_template<H>(self, template: ResourceTemplate, handler: H) -> Self
    where
        H: Fn(&str) -> Result<Vec<ResourceContents>, String> + Send + Sync + 'static,
    {
        let key = template.uri_template.clone();
        let entry = ResourceTemplateEntry {
            template,
            handler: Box::new(handler),
        };
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .resource_templates
            .insert(key, entry);
        self
    }

    /// Register the completion handler. Only one — the spec
    /// exposes a single `completion/complete` endpoint that
    /// takes a `ref` to disambiguate.
    pub fn register_complete<H>(self, handler: H) -> Self
    where
        H: Fn(&str, &CompleteRequestParams) -> Result<Vec<String>, String> + Send + Sync + 'static,
    {
        self.inner.lock().expect("McpSurfaces poisoned").complete = Some(Box::new(handler));
        self
    }

    /// Mark a resource URI as eligible for subscription. The
    /// `accepted_subscription_filter` override will intersect
    /// the client's requested filter against this set. The
    /// user is expected to also register the resource with
    /// `register_resource` (or via a `register_resource_template`)
    /// so the client can `resources/read` it; this call only
    /// controls the subscription side.
    pub fn register_subscribable_resource(self, uri: impl Into<String>) -> Self {
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .subscribable_resources
            .insert(uri.into());
        self
    }

    /// Register the elicitation handler (SEP-1034). Only one
    /// is supported. The handler receives the request the
    /// server wants to send to the client and a
    /// `RequestContext<RoleServer>`, and returns the client's
    /// response (or an error string). Available only with
    /// the `elicitation` feature.
    #[cfg(feature = "elicitation")]
    pub fn register_elicitation<H>(self, handler: H) -> Self
    where
        H: Fn(
                ElicitRequestParams,
                rmcp::service::RequestContext<rmcp::service::RoleServer>,
            ) -> Result<ElicitResult, String>
            + Send
            + Sync
            + 'static,
    {
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .elicitation_handler = Some(std::sync::Arc::new(handler));
        self
    }

    /// Attach cache hints (SEP-2549) to every list/read result
    /// served by this surfaces bundle. The hints are stamped on
    /// `list_prompts` / `list_resources` / `list_resource_templates`
    /// / `read_resource` responses as top-level `ttlMs` /
    /// `cacheScope` fields, but only for peers on protocol
    /// ≥ 2026-07-28 (the override gates on the request's protocol
    /// version).
    pub fn with_cache_hints(self, hints: CacheHints) -> Self {
        self.inner.lock().expect("McpSurfaces poisoned").cache_hints = Some(hints);
        self
    }

    // -- read paths used by the ServerHandler overrides --------

    pub(crate) fn cache_hints(&self) -> Option<CacheHints> {
        self.inner.lock().expect("McpSurfaces poisoned").cache_hints
    }

    pub(crate) fn list_prompts(&self) -> Vec<Prompt> {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        g.prompts.values().map(|e| e.prompt.clone()).collect()
    }

    pub(crate) fn get_prompt(
        &self,
        request: &GetPromptRequestParams,
    ) -> Result<GetPromptResponse, ErrorData> {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        let entry = g.prompts.get(&request.name).ok_or_else(|| {
            ErrorData::invalid_params(format!("prompt `{}` not found", request.name), None)
        })?;
        let args = request.arguments.clone().unwrap_or_default();
        let messages = (entry.handler)(args).map_err(|e| {
            ErrorData::internal_error(format!("prompt `{}` failed: {e}", request.name), None)
        })?;
        let mut result = GetPromptResult::new(messages);
        result.description = entry.prompt.description.clone();
        Ok(GetPromptResponse::Complete(result))
    }

    pub(crate) fn list_resources(&self) -> Vec<Resource> {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        g.resources.values().map(|e| e.resource.clone()).collect()
    }

    pub(crate) fn list_resource_templates(&self) -> Vec<ResourceTemplate> {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        g.resource_templates
            .values()
            .map(|e| e.template.clone())
            .collect()
    }

    pub(crate) fn read_resource(
        &self,
        request: &ReadResourceRequestParams,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        // 1) Try the static resource list first.
        if let Some(entry) = g.resources.get(&request.uri) {
            let contents = (entry.handler)(&request.uri).map_err(|e| {
                ErrorData::internal_error(format!("resource `{}` failed: {e}", request.uri), None)
            })?;
            return Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
                contents,
            )));
        }
        // 2) Otherwise, see if any registered template's
        //    uri_template is a *prefix* match. The spec uses
        //    RFC 6570, but for v1 we accept templates whose
        //    static prefix (everything before the first
        //    `{`) matches. This is enough for the common
        //    "nestrs://docs/{name}" shape.
        for entry in g.resource_templates.values() {
            if let Some(prefix) = template_static_prefix(&entry.template.uri_template) {
                if request.uri.starts_with(&prefix) {
                    let contents = (entry.handler)(&request.uri).map_err(|e| {
                        ErrorData::internal_error(
                            format!(
                                "resource template `{}` failed: {e}",
                                entry.template.uri_template
                            ),
                            None,
                        )
                    })?;
                    return Ok(ReadResourceResponse::Complete(ReadResourceResult::new(
                        contents,
                    )));
                }
            }
        }
        Err(ErrorData::invalid_params(
            format!("resource `{}` not found", request.uri),
            None,
        ))
    }

    pub(crate) fn complete(
        &self,
        request: &CompleteRequestParams,
    ) -> Result<CompleteResult, ErrorData> {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        let handler = g
            .complete
            .as_ref()
            .ok_or_else(|| ErrorData::invalid_params("no completion handler registered", None))?;
        let arg = &request.argument.value;
        let values = handler(arg, request)
            .map_err(|e| ErrorData::internal_error(format!("completion failed: {e}"), None))?;
        // `with_all_values` stamps the `total` and `has_more`
        // fields the spec wants (vs. `new` which leaves them
        // `None`). The spec lets servers omit them; we set
        // them so callers that do `result.completion.total`
        // get a useful number without a follow-up
        // length().
        let info = CompletionInfo::with_all_values(values)
            .map_err(|e| ErrorData::internal_error(format!("completion invalid: {e}"), None))?;
        Ok(CompleteResult::new(info))
    }

    // -- subscription reads (used by the ServerHandler overrides) --

    /// Intersect a client-requested subscription filter against
    /// the URIs the user has marked subscribable. The result is
    /// what the SDK's `accepted_subscription_filter` returns.
    /// An empty `requested.resource_subscriptions` (or no
    /// registered URIs) yields an empty
    /// `resource_subscriptions` field, which the SDK treats as
    /// "the server has no subscribable resources" — the
    /// subscription is acknowledged with no events.
    pub(crate) fn accepted_filter_for(&self, requested: &SubscriptionFilter) -> SubscriptionFilter {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        let mut accepted = SubscriptionFilter::new();
        // We don't claim any of the list-changed booleans
        // for now — those are for tool/prompt/resource
        // discovery, which nestrs-mcp doesn't drive
        // automatically. Subscriptions are URI-specific.
        if let Some(uris) = requested.resource_subscriptions.as_ref() {
            let keep: Vec<String> = uris
                .iter()
                .filter(|u| g.subscribable_resources.contains(*u))
                .cloned()
                .collect();
            if !keep.is_empty() {
                accepted.resource_subscriptions = Some(keep);
            }
        }
        accepted
    }

    /// Insert a live `SubscriptionSink` for an established
    /// subscription. The id is whatever the SDK stamps
    /// (`SubscriptionContext::subscription_id()`); the
    /// `listen` override provides it.
    pub(crate) fn add_subscription_sink(&self, id: String, sink: SubscriptionSink) {
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .subscription_sinks
            .insert(id, sink);
    }

    /// Remove a `SubscriptionSink` by id (on cancel or graceful
    /// close). Returns the removed sink so the caller can
    /// inspect the cause if it cares.
    pub(crate) fn remove_subscription_sink(&self, id: &str) -> Option<SubscriptionSink> {
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .subscription_sinks
            .remove(id)
    }

    /// Return all sinks whose subscription includes `uri`.
    /// This is the fan-out point for the resource-write path:
    /// call `sinks_for(uri)` and then
    /// `sink.notify_resource_updated(uri).await` on each.
    /// The set may be empty (nobody subscribed, or the
    /// subscription has already torn down).
    pub(crate) fn subscription_sinks_for(&self, uri: &str) -> Vec<SubscriptionSink> {
        let g = self.inner.lock().expect("McpSurfaces poisoned");
        if !g.subscribable_resources.contains(uri) {
            return Vec::new();
        }
        g.subscription_sinks.values().cloned().collect()
    }

    /// Whether any URIs have been marked subscribable. Used
    /// by the `get_info` override to decide whether to
    /// advertise the `resources.subscribe` capability.
    pub(crate) fn has_subscribable_resources(&self) -> bool {
        !self
            .inner
            .lock()
            .expect("McpSurfaces poisoned")
            .subscribable_resources
            .is_empty()
    }

    // -- elicitation reads (used by NestrsMcpServer::elicit) --

    /// Borrow the user-supplied elicitation handler, if any.
    /// Returns `None` if no handler was registered.
    #[cfg(feature = "elicitation")]
    pub(crate) fn elicitation_handler(&self) -> Option<ElicitationHandler> {
        self.inner
            .lock()
            .expect("McpSurfaces poisoned")
            .elicitation_handler
            .as_ref()
            .map(|a| a.clone())
    }
}

/// If `template` has a `{` placeholder, return the substring
/// before the first `{`. Used as a cheap prefix match in
/// `read_resource`. Returns `None` when the template has no
/// placeholders (callers should fall back to the exact
/// resource map).
fn template_static_prefix(template: &str) -> Option<String> {
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Trim a trailing `/` so "nestrs://docs/{name}"
            // → "nestrs://docs/" (a real path prefix) and
            // "nestrs://{thing}" → "nestrs://" (which the
            // caller should not match, but the read path
            // is fine either way).
            let prefix = &template[..i];
            let trimmed = prefix.trim_end_matches('/');
            return Some(trimmed.to_string());
        }
        i += 1;
    }
    None
}

// -- builder helpers for common shapes ------------------------------

/// Build a text `PromptMessage` for the user role. The
/// `content` is the text the model will see.
pub fn user_text(text: impl Into<String>) -> PromptMessage {
    PromptMessage::new(Role::User, ContentBlock::text(text.into()))
}

/// Build a text `PromptMessage` for the assistant role.
pub fn assistant_text(text: impl Into<String>) -> PromptMessage {
    PromptMessage::new(Role::Assistant, ContentBlock::text(text.into()))
}

/// Build a `ResourceContents` carrying plain text. Most file
/// resources use this shape; the `text/markdown` mime type is
/// the most common fit.
pub fn text_contents(
    uri: impl Into<String>,
    mime_type: Option<impl Into<String>>,
    text: impl Into<String>,
) -> ResourceContents {
    ResourceContents::TextResourceContents {
        uri: uri.into(),
        mime_type: mime_type.map(Into::into),
        text: text.into(),
        meta: None,
    }
}

// -- MRTR-for-tools helpers (SEP-2322) ----------------------------

/// Build a `CallToolResponse::InputRequired` from a structured
/// "ask the user" shape. A tool that wants to ask a question
/// mid-flight returns this from its body:
///
/// ```ignore
/// #[tool(description = "pick a colour")]
/// async fn pick(&self) -> Result<InputRequiredResult, McpError> {
///     let mut props = BTreeMap::new();
///     props.insert("colour".into(), PrimitiveSchemaDefinition::String(StringSchema {
///         title: Some("Colour".into()),
///         description: Some("Pick one".into()),
///         ..Default::default()
///     }));
///     Ok(elicit_input("Pick a colour", ElicitationSchema::new(props), None))
/// }
/// ```
///
/// The returned `InputRequiredResult` carries the elicitation
/// under a single server-assigned key (the request name) so
/// the client can echo it back unchanged. `request_state` is
/// optional — pass `Some(opaque_state)` if the tool needs to
/// carry server-side state across MRTR rounds.
pub fn elicit_input(
    message: impl Into<String>,
    schema: ElicitationSchema,
    request_state: Option<String>,
) -> InputRequiredResult {
    let elicit = ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: message.into(),
        requested_schema: schema,
    };
    let mut requests: InputRequests = std::collections::BTreeMap::new();
    // One server-assigned key per call. The client echoes
    // responses back under the same key per SEP-2322.
    requests.insert(
        "input".to_string(),
        InputRequest::Elicitation(Request::new(elicit)),
    );
    InputRequiredResult::new(Some(requests), request_state)
}

/// Extract the `inputResponses` map the client sent back on a
/// `tools/call` retry. Returns `None` when the request has no
/// `inputResponses` field — the caller should treat that the
/// same as "no prior round."
///
/// Per SEP-2322 the client echoes each `InputRequest`'s
/// response under the same key the server used. Values are
/// opaque JSON because the response union is heterogeneous.
pub fn input_responses(
    request: &CallToolRequestParams,
) -> Option<std::collections::BTreeMap<String, serde_json::Value>> {
    request.input_responses.clone()
}

// -- a sealed trait for the "async-returns-pinned" return shape ---
// (None. The rmcp ServerHandler trait methods return
// `Pin<Box<dyn Future>>`; the overrides in `server.rs` spell
// out the type at each site. We don't add a public helper
// here because it would obscure rather than illuminate.)

#[cfg(test)]
mod tests {
    //! Unit tests for `McpSurfaces`. The reader methods are
    //! `pub(crate)` because the rmcp `RequestContext` is too
    //! awkward to construct in an integration test (it carries
    //! a `Peer<R>` with a real mpsc channel). These tests
    //! exercise the body of the override methods directly,
    //! which is the same code the `ServerHandler` impl
    //! invokes — there's no per-call glue to test separately.

    use super::*;
    use rmcp::handler::server::tool::IntoCallToolResult;
    use rmcp::handler::server::ServerHandler;
    use rmcp::model::{
        ArgumentInfo, CallToolResponse, CallToolResult, CompleteRequestParams, CreateTaskResult,
        ErrorCode, GetPromptRequestParams, PrimitiveSchemaDefinition, PromptArgument,
        ReadResourceRequestParams, Reference, TaskPayload, TaskStatus, TASKS_EXTENSION_ID,
    };
    use rmcp::task_manager::{TaskExit, TaskManager, TaskOptions};
    use rmcp::{tool, tool_router, Json};
    use serde_json::json;

    fn arg(name: &str, required: bool) -> PromptArgument {
        let mut a = PromptArgument::default();
        a.name = name.into();
        a.required = Some(required);
        a
    }

    fn get_prompt_args(
        name: &str,
        args: serde_json::Map<String, serde_json::Value>,
    ) -> GetPromptRequestParams {
        let mut p = GetPromptRequestParams::new(name);
        p.arguments = Some(args);
        p
    }

    fn read_args(uri: &str) -> ReadResourceRequestParams {
        ReadResourceRequestParams::new(uri)
    }

    fn complete_args(value: &str) -> CompleteRequestParams {
        CompleteRequestParams::new(
            Reference::for_prompt("review"),
            ArgumentInfo::new("topic", value),
        )
    }

    fn make_resource(uri: &str, name: &str) -> Resource {
        Resource::new(uri, name).with_mime_type("text/plain")
    }

    // -- empty / default state ----------------------------------

    #[test]
    fn empty_surfaces_lists_nothing() {
        let s = McpSurfaces::new();
        assert!(s.list_prompts().is_empty());
        assert!(s.list_resources().is_empty());
        assert!(s.list_resource_templates().is_empty());
    }

    #[test]
    fn empty_surfaces_get_prompt_errors() {
        let s = McpSurfaces::new();
        let err = s
            .get_prompt(&get_prompt_args("nope", Default::default()))
            .unwrap_err();
        // The error code is what matters; the message is human-readable.
        let s = err.to_string();
        assert!(s.contains("nope"), "unexpected error: {s}");
    }

    #[test]
    fn empty_surfaces_read_resource_errors() {
        let s = McpSurfaces::new();
        let err = s.read_resource(&read_args("file:///x")).unwrap_err();
        assert!(err.to_string().contains("file:///x"));
    }

    #[test]
    fn empty_surfaces_complete_errors() {
        let s = McpSurfaces::new();
        let err = s.complete(&complete_args("")).unwrap_err();
        assert!(err.to_string().contains("completion"));
    }

    // -- prompts -----------------------------------------------

    #[test]
    fn register_prompt_appears_in_list() {
        let s = McpSurfaces::new().register_prompt(
            "review",
            Some("Code review prompt".into()),
            vec![arg("topic", true)],
            |_args| Ok(vec![user_text("hi")]),
        );
        let prompts = s.list_prompts();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].name, "review");
        assert_eq!(
            prompts[0].description.as_deref(),
            Some("Code review prompt")
        );
        assert!(prompts[0].arguments.is_some());
        assert_eq!(prompts[0].arguments.as_ref().unwrap()[0].name, "topic");
    }

    #[test]
    fn get_prompt_runs_handler_and_returns_messages() {
        let s = McpSurfaces::new().register_prompt(
            "echo",
            Some("Echo prompt".into()),
            vec![arg("text", true)],
            |mut args| {
                let text = args
                    .remove("text")
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                Ok(vec![user_text(text)])
            },
        );
        let mut input = serde_json::Map::new();
        input.insert("text".into(), json!("hello world"));
        let resp = s.get_prompt(&get_prompt_args("echo", input)).unwrap();
        let GetPromptResponse::Complete(result) = resp else {
            panic!("expected Complete response")
        };
        assert_eq!(result.messages.len(), 1);
        match &result.messages[0].content {
            ContentBlock::Text(t) => assert_eq!(t.text, "hello world"),
            _ => panic!("expected text block"),
        }
    }

    #[test]
    fn get_prompt_handler_error_propagates() {
        let s = McpSurfaces::new()
            .register_prompt("boom", None, vec![], |_args| Err("handler says no".into()));
        let err = s
            .get_prompt(&get_prompt_args("boom", Default::default()))
            .unwrap_err();
        assert!(err.to_string().contains("handler says no"));
    }

    #[test]
    fn get_prompt_with_missing_args_passes_empty_map() {
        // `arguments: None` (not Some(empty)) — the handler should
        // still receive an empty map and run.
        let s = McpSurfaces::new().register_prompt("noargs", None, vec![], |args| {
            assert!(args.is_empty());
            Ok(vec![assistant_text("ack")])
        });
        let mut req = get_prompt_args("noargs", Default::default());
        req.arguments = None;
        let resp = s.get_prompt(&req).unwrap();
        let GetPromptResponse::Complete(r) = resp else {
            panic!()
        };
        assert_eq!(r.messages.len(), 1);
    }

    #[test]
    fn register_two_prompts_both_listed() {
        let s = McpSurfaces::new()
            .register_prompt("a", None, vec![], |_| Ok(vec![user_text("a")]))
            .register_prompt("b", None, vec![], |_| Ok(vec![user_text("b")]));
        let names: Vec<String> = s.list_prompts().into_iter().map(|p| p.name).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"a".to_string()));
        assert!(names.contains(&"b".to_string()));
    }

    // -- resources ---------------------------------------------

    #[test]
    fn register_resource_appears_in_list_and_reads_back() {
        let s =
            McpSurfaces::new().register_resource(make_resource("file:///a.txt", "alpha"), |uri| {
                assert_eq!(uri, "file:///a.txt");
                Ok(vec![ResourceContents::text("alpha body", uri)])
            });
        let r = s.list_resources();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].uri, "file:///a.txt");
        let resp = s.read_resource(&read_args("file:///a.txt")).unwrap();
        let ReadResourceResponse::Complete(result) = resp else {
            panic!()
        };
        assert_eq!(result.contents.len(), 1);
        match &result.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => assert_eq!(text, "alpha body"),
            _ => panic!("expected text"),
        }
    }

    #[test]
    fn read_resource_falls_back_to_template() {
        // No static resource matches; the template prefix
        // does.
        let s = McpSurfaces::new().register_resource_template(
            ResourceTemplate::new("docs://{name}", "doc_tmpl"),
            |uri| Ok(vec![ResourceContents::text("rendered", uri)]),
        );
        let resp = s
            .read_resource(&read_args("docs://router/getting-started"))
            .unwrap();
        let ReadResourceResponse::Complete(r) = resp else {
            panic!()
        };
        assert_eq!(r.contents.len(), 1);
    }

    #[test]
    fn static_resource_wins_over_template() {
        // If a static resource is registered for a URI that
        // also matches a template, the static one wins.
        let s = McpSurfaces::new()
            .register_resource(make_resource("docs://router", "router-doc"), |uri| {
                Ok(vec![ResourceContents::text("static", uri)])
            })
            .register_resource_template(
                ResourceTemplate::new("docs://{name}", "doc_tmpl"),
                |uri| Ok(vec![ResourceContents::text("template", uri)]),
            );
        let resp = s.read_resource(&read_args("docs://router")).unwrap();
        let ReadResourceResponse::Complete(r) = resp else {
            panic!()
        };
        match &r.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => {
                assert_eq!(text, "static");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn read_resource_unknown_uri_errors() {
        let s = McpSurfaces::new();
        let err = s.read_resource(&read_args("unknown://x")).unwrap_err();
        assert!(err.to_string().contains("unknown://x"));
    }

    // -- resource templates -----------------------------------

    #[test]
    fn register_template_appears_in_list() {
        let s = McpSurfaces::new().register_resource_template(
            ResourceTemplate::new("docs://{name}", "doc_tmpl"),
            |_uri| Ok(vec![]),
        );
        let templates = s.list_resource_templates();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].uri_template, "docs://{name}");
        assert_eq!(templates[0].name, "doc_tmpl");
    }

    #[test]
    fn template_static_prefix_strips_trailing_slash() {
        // The prefix helper trims a trailing `/` so a
        // "docs://{name}" template's prefix is "docs:" (no
        // double slash).
        assert_eq!(
            template_static_prefix("docs://{name}").as_deref(),
            Some("docs:")
        );
    }

    #[test]
    fn template_without_placeholder_returns_none() {
        // No `{` — caller should fall back to the exact
        // resource map.
        assert_eq!(template_static_prefix("static.txt"), None);
    }

    // -- completion -------------------------------------------

    #[test]
    fn complete_returns_user_supplied_values() {
        let s = McpSurfaces::new().register_complete(|value, _req| {
            Ok(vec![format!("{value}.alpha"), format!("{value}.beta")])
        });
        let result = s.complete(&complete_args("rus")).unwrap();
        assert_eq!(result.completion.values, vec!["rus.alpha", "rus.beta"]);
        assert_eq!(result.completion.total, Some(2));
    }

    #[test]
    fn complete_handler_error_propagates() {
        let s = McpSurfaces::new().register_complete(|_, _| Err("oops".into()));
        let err = s.complete(&complete_args("x")).unwrap_err();
        assert!(err.to_string().contains("oops"));
    }

    // -- user_text / assistant_text / text_contents helpers ----

    #[test]
    fn user_text_builder_produces_user_role() {
        let m = user_text("hi");
        assert!(matches!(m.role, Role::User));
    }

    #[test]
    fn assistant_text_builder_produces_assistant_role() {
        let m = assistant_text("ok");
        assert!(matches!(m.role, Role::Assistant));
    }

    #[test]
    fn text_contents_builder_round_trips() {
        let c = text_contents("file:///x", Some::<&str>("text/markdown"), "body");
        match c {
            ResourceContents::TextResourceContents {
                uri,
                mime_type,
                text,
                ..
            } => {
                assert_eq!(uri, "file:///x");
                assert_eq!(mime_type.as_deref(), Some("text/markdown"));
                assert_eq!(text, "body");
            }
            _ => panic!("expected text contents"),
        }
    }

    // -- surfacing through NestrsMcpServer -------------------

    #[test]
    fn nestrs_mcp_server_default_has_empty_surfaces() {
        let s = crate::server::NestrsMcpServer::new();
        assert!(s.surfaces.list_prompts().is_empty());
        assert!(s.surfaces.list_resources().is_empty());
        assert!(s.surfaces.list_resource_templates().is_empty());
    }

    #[test]
    fn nestrs_mcp_server_with_surfaces_attaches_them() {
        let surfaces =
            McpSurfaces::new().register_prompt("hi", None, vec![], |_| Ok(vec![user_text("hi")]));
        let s = crate::server::NestrsMcpServer::new().with_surfaces(surfaces);
        assert_eq!(s.surfaces.list_prompts().len(), 1);
    }

    #[test]
    fn surfaces_inner_default_is_empty() {
        let s = McpSurfaces::new();
        // We can't reach the inner directly (it's private), but
        // every read path returns empty — that's the contract.
        assert!(s.list_prompts().is_empty());
        assert!(s.list_resources().is_empty());
        assert!(s.list_resource_templates().is_empty());
    }

    // -- silence the unused-import warning for ArgumentInfo ---
    // (ArgumentInfo is part of the `complete` request shape and
    // is used inside `complete_args` above; we re-import it
    // here in case a follow-up test wants to build an
    // ArgumentInfo directly.)
    #[allow(dead_code)]
    fn _argument_info_silencer() -> ArgumentInfo {
        ArgumentInfo::new("topic", "x")
    }

    // -- subscriptions ----------------------------------------

    #[test]
    fn empty_surfaces_has_no_subscribable_resources() {
        let s = McpSurfaces::new();
        assert!(!s.has_subscribable_resources());
        let req = SubscriptionFilter::new();
        let accepted = s.accepted_filter_for(&req);
        assert!(accepted
            .resource_subscriptions
            .as_ref()
            .map_or(true, |v| v.is_empty()));
    }

    #[test]
    fn register_subscribable_resource_appears_in_filter() {
        let s = McpSurfaces::new().register_subscribable_resource("file:///a.txt");
        assert!(s.has_subscribable_resources());
        let mut req = SubscriptionFilter::new();
        req.resource_subscriptions = Some(vec!["file:///a.txt".to_string()]);
        let accepted = s.accepted_filter_for(&req);
        let got = accepted.resource_subscriptions.expect("Some");
        assert_eq!(got, vec!["file:///a.txt".to_string()]);
    }

    #[test]
    fn accepted_filter_intersects_requested_with_registered() {
        // The intersection is the registered URIs that the
        // client also asked for. The client asked for two;
        // only one is registered; the result is the one.
        let s = McpSurfaces::new().register_subscribable_resource("file:///a.txt");
        let mut req = SubscriptionFilter::new();
        req.resource_subscriptions = Some(vec![
            "file:///a.txt".to_string(),
            "file:///b.txt".to_string(),
        ]);
        let accepted = s.accepted_filter_for(&req);
        let got = accepted.resource_subscriptions.unwrap();
        assert_eq!(got, vec!["file:///a.txt".to_string()]);
    }

    #[test]
    fn accepted_filter_drops_unregistered_requested_uris() {
        // No registered subscribables — the filter should
        // return an empty accepted list, not the requested
        // list verbatim.
        let s = McpSurfaces::new();
        let mut req = SubscriptionFilter::new();
        req.resource_subscriptions = Some(vec!["file:///a.txt".to_string()]);
        let accepted = s.accepted_filter_for(&req);
        assert!(accepted
            .resource_subscriptions
            .as_ref()
            .map_or(true, |v| v.is_empty()));
    }

    #[test]
    fn accepted_filter_does_not_advertise_list_changed_booleans() {
        // nestrs-mcp doesn't drive automatic list-changed
        // notifications. The boolean flags on the filter
        // should always come back as None.
        let s = McpSurfaces::new().register_subscribable_resource("file:///a.txt");
        let mut req = SubscriptionFilter::new();
        req.tools_list_changed = Some(true);
        req.prompts_list_changed = Some(true);
        req.resources_list_changed = Some(true);
        req.resource_subscriptions = Some(vec!["file:///a.txt".to_string()]);
        let accepted = s.accepted_filter_for(&req);
        assert_eq!(accepted.tools_list_changed, None);
        assert_eq!(accepted.prompts_list_changed, None);
        assert_eq!(accepted.resources_list_changed, None);
        assert!(accepted.resource_subscriptions.is_some());
    }

    #[test]
    fn subscription_sinks_for_unknown_uri_returns_empty() {
        // Direct fan-out: a URI that isn't in the
        // subscribable set should return no sinks even if
        // sinks are registered. This is the negative
        // path the resource-write code would exercise.
        let s = McpSurfaces::new().register_subscribable_resource("file:///a.txt");
        // No sinks inserted, so the result is empty.
        assert!(s.subscription_sinks_for("file:///a.txt").is_empty());
    }

    #[test]
    fn subscription_sinks_for_subscribable_uri_returns_empty_when_no_sinks() {
        // The URI is subscribable but no client has
        // established a subscription — fan-out yields
        // nothing (and a `notify_resource_updated` would
        // be a no-op for everyone).
        let s = McpSurfaces::new().register_subscribable_resource("file:///a.txt");
        let result = s.subscription_sinks_for("file:///a.txt");
        assert!(result.is_empty());
    }

    #[test]
    fn nestrs_mcp_server_get_info_does_not_advertise_subscribe_when_empty() {
        // No subscribable resources — the capability
        // shouldn't be on. (If it were, a client would
        // happily try to subscribe to nothing.)
        let s = crate::server::NestrsMcpServer::new();
        let info = s.get_info();
        let resources_cap = info.capabilities.resources.as_ref();
        // Either the resources capability is absent or its
        // `subscribe` is None / false.
        let subscribe_advertised = resources_cap.and_then(|c| c.subscribe).unwrap_or(false);
        assert!(!subscribe_advertised);
    }

    #[test]
    fn nestrs_mcp_server_default_accepted_filter_is_none() {
        // No subscribable resources registered, client
        // asked for nothing — the override returns `None`
        // to signal "subscriptions/listen not implemented"
        // and skip the acknowledge round trip.
        let s = crate::server::NestrsMcpServer::new();
        let req = SubscriptionFilter::new();
        let accepted = s.accepted_subscription_filter(&req);
        assert!(accepted.is_none());
    }

    #[test]
    fn nestrs_mcp_server_accepted_filter_intersects_with_requested() {
        // One subscribable registered, client asked for
        // two — only the registered one comes back. The
        // override returns `Some(filter)` with the
        // intersection; the SDK ships the `Some` to the
        // client to acknowledge what we'll actually
        // deliver.
        let surfaces = McpSurfaces::new().register_subscribable_resource("file:///a.txt");
        let s = crate::server::NestrsMcpServer::new().with_surfaces(surfaces);
        let mut req = SubscriptionFilter::new();
        req.resource_subscriptions = Some(vec![
            "file:///a.txt".to_string(),
            "file:///b.txt".to_string(),
        ]);
        let accepted = s.accepted_subscription_filter(&req).expect("Some");
        let got = accepted.resource_subscriptions.expect("Some");
        assert_eq!(got, vec!["file:///a.txt".to_string()]);
    }

    #[test]
    fn nestrs_mcp_server_accepted_filter_is_none_when_nothing_matches() {
        // Client asked for one URI, nothing is registered
        // — the override returns `None` (no acknowledge
        // round trip needed). This is the "client asked
        // for things we don't have" path.
        let s = crate::server::NestrsMcpServer::new();
        let mut req = SubscriptionFilter::new();
        req.resource_subscriptions = Some(vec!["file:///a.txt".to_string()]);
        let accepted = s.accepted_subscription_filter(&req);
        assert!(accepted.is_none());
    }

    // -- MRTR-for-tools (SEP-2322) ----------------------------

    #[test]
    fn elicit_input_builds_input_required_result() {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "colour".to_string(),
            PrimitiveSchemaDefinition::String(rmcp::model::StringSchema::default()),
        );
        let result = elicit_input("Pick a colour", ElicitationSchema::new(props), None);
        // Result type is INPUT_REQUIRED.
        assert_eq!(result.result_type, rmcp::model::ResultType::INPUT_REQUIRED);
        // We have one input request, under the "input" key.
        let requests = result.input_requests.expect("input_requests");
        assert_eq!(requests.len(), 1);
        assert!(requests.contains_key("input"));
    }

    #[test]
    fn elicit_input_with_state_includes_request_state() {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "x".to_string(),
            PrimitiveSchemaDefinition::String(rmcp::model::StringSchema::default()),
        );
        let result = elicit_input("go", ElicitationSchema::new(props), Some("opaque".into()));
        assert_eq!(result.request_state.as_deref(), Some("opaque"));
    }

    #[test]
    fn elicit_input_without_state_omits_request_state() {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "x".to_string(),
            PrimitiveSchemaDefinition::String(rmcp::model::StringSchema::default()),
        );
        let result = elicit_input("go", ElicitationSchema::new(props), None);
        assert!(result.request_state.is_none());
    }

    #[test]
    fn elicit_input_serializes_with_result_type() {
        let mut props = std::collections::BTreeMap::new();
        props.insert(
            "x".to_string(),
            PrimitiveSchemaDefinition::String(rmcp::model::StringSchema::default()),
        );
        let result = elicit_input("go", ElicitationSchema::new(props), None);
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["resultType"], "input_required");
        assert!(json["inputRequests"]["input"].is_object());
    }

    #[test]
    fn input_responses_returns_none_when_absent() {
        // No input_responses at all → None.
        let req = CallToolRequestParams::new("anything");
        assert!(input_responses(&req).is_none());
    }

    #[test]
    fn input_responses_extracts_responses() {
        let mut responses = std::collections::BTreeMap::new();
        responses.insert("input".to_string(), serde_json::json!({"colour": "blue"}));
        let req = CallToolRequestParams::new("anything").with_input_responses(responses);
        let got = input_responses(&req).expect("Some");
        assert_eq!(got.get("input").unwrap()["colour"], "blue");
    }

    #[test]
    fn input_responses_round_trips_through_serde() {
        // Wire-level round-trip: serialize the request
        // with `input_responses` and confirm we can pull
        // them back out via the helper. This is the path
        // the SDK takes — the JSON-RPC message contains
        // `inputResponses` at the top level of
        // `CallToolRequestParams`.
        let mut responses = std::collections::BTreeMap::new();
        responses.insert("input".to_string(), serde_json::json!("accepted"));
        let req = CallToolRequestParams::new("anything").with_input_responses(responses);
        let json = serde_json::to_value(&req).unwrap();
        assert!(json["inputResponses"].is_object());
        let back: CallToolRequestParams = serde_json::from_value(json).unwrap();
        let got = input_responses(&back).expect("Some");
        assert_eq!(got.get("input").unwrap(), "accepted");
    }

    // -- elicitation (only compiled with the feature) --------

    #[cfg(feature = "elicitation")]
    #[test]
    fn empty_surfaces_elicitation_handler_returns_none() {
        let s = McpSurfaces::new();
        assert!(s.elicitation_handler().is_none());
    }

    #[cfg(feature = "elicitation")]
    #[test]
    fn register_elicitation_handler_attaches() {
        let s = McpSurfaces::new().register_elicitation(|_req, _ctx| {
            Ok(rmcp::model::ElicitResult::new(
                rmcp::model::ElicitationAction::Cancel,
            ))
        });
        let handler = s.elicitation_handler();
        assert!(handler.is_some());
    }

    // -- cache hints (SEP-2549) -------------------------------

    #[test]
    fn with_cache_hints_default_none() {
        // Without opting in, no hints are attached — the wire
        // result carries neither `ttlMs` nor `cacheScope`.
        let s = McpSurfaces::new();
        assert!(s.cache_hints().is_none());
        let result = attach_cache_hints(
            ListResourcesResult::with_all_items(vec![]),
            s.cache_hints(),
            true,
        );
        assert_eq!(result.ttl_ms, None);
        assert_eq!(result.cache_scope, None);
    }

    #[test]
    fn attach_hints_to_list_resources() {
        let hints = CacheHints::new(60_000, CacheScope::Public);
        let result = attach_cache_hints(
            ListResourcesResult::with_all_items(vec![]),
            Some(hints),
            true,
        );
        assert_eq!(result.ttl_ms, Some(60_000));
        assert_eq!(result.cache_scope, Some(CacheScope::Public));
    }

    #[test]
    fn attach_hints_to_read_resource() {
        let hints = CacheHints::new(30_000, CacheScope::Private);
        let result = attach_cache_hints(ReadResourceResult::new(vec![]), Some(hints), true);
        assert_eq!(result.ttl_ms, Some(30_000));
        assert_eq!(result.cache_scope, Some(CacheScope::Private));
    }

    #[test]
    fn attach_hints_suppressed_for_old_protocol() {
        // Pre-2026-07-28 peers don't know the fields — the
        // override gates on the protocol version, so the
        // helper must leave the result untouched.
        let hints = CacheHints::new(60_000, CacheScope::Public);
        let result = attach_cache_hints(
            ListResourcesResult::with_all_items(vec![]),
            Some(hints),
            false,
        );
        assert_eq!(result.ttl_ms, None);
        assert_eq!(result.cache_scope, None);
    }

    #[test]
    fn attach_hints_private_scope_round_trips() {
        // Wire shape: `cacheScope` serializes lowercase
        // ("private") and `ttlMs` is a top-level camelCase
        // field — the SEP-2549 layout.
        let hints = CacheHints::new(5_000, CacheScope::Private);
        let result = attach_cache_hints(ReadResourceResult::new(vec![]), Some(hints), true);
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["ttlMs"], 5_000);
        assert_eq!(json["cacheScope"], "private");
    }

    #[test]
    fn negative_ttl_normalizes_to_zero() {
        // SEP-2549: `ttlMs` MUST be >= 0; a negative value from a
        // (misbehaving) server deserializes to 0 rather than
        // erroring. Our types inherit that SDK serde behaviour —
        // this pins it so a rmcp upgrade that changes it fails
        // here instead of in a user's client.
        let back: ListResourcesResult =
            serde_json::from_value(json!({"resources": [], "ttlMs": -5})).unwrap();
        assert_eq!(back.ttl_ms, Some(0));
    }

    #[test]
    fn hints_apply_to_prompts_and_templates() {
        // The same helper covers all three list results — the
        // trait impls are generated by macro over the identical
        // builder pairs, so one test per remaining type is
        // enough to catch a signature drift.
        let hints = CacheHints::new(1_000, CacheScope::Public);
        let prompts =
            attach_cache_hints(ListPromptsResult::with_all_items(vec![]), Some(hints), true);
        assert_eq!(prompts.ttl_ms, Some(1_000));
        let templates = attach_cache_hints(
            ListResourceTemplatesResult::with_all_items(vec![]),
            Some(hints),
            true,
        );
        assert_eq!(templates.ttl_ms, Some(1_000));
    }

    // -- structuredContent ------------------------------------

    // Fixture for the `Json<T>` tool path. The `#[tool_router]`
    // macro generates the output schema from the `Json<TestOut>`
    // return type; the router's `list_all()` is what a
    // `tools/list` would surface.
    struct JsonToolFixture {
        tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
    }

    impl Default for JsonToolFixture {
        fn default() -> Self {
            // The macro generates `Self::tool_router()`; the
            // derived `Default` would leave the field empty.
            Self {
                tool_router: Self::tool_router(),
            }
        }
    }

    #[derive(Debug, serde::Serialize, schemars::JsonSchema)]
    struct TestOut {
        answer: u32,
    }

    #[tool_router]
    impl JsonToolFixture {
        #[tool(description = "echoes a typed payload")]
        async fn echo(&self) -> Json<TestOut> {
            Json(TestOut { answer: 42 })
        }
    }

    #[test]
    fn json_tool_result_has_structured_content() {
        // The `Json<T>` wrapper's `IntoCallToolResult` impl calls
        // `CallToolResult::structured` — the typed JSON lands in
        // `structuredContent`, not just in the text content.
        let response = Json(TestOut { answer: 42 })
            .into_call_tool_result()
            .unwrap();
        let CallToolResponse::Complete(result) = response else {
            panic!("expected Complete")
        };
        let structured = result.structured_content.expect("Some");
        assert_eq!(structured["answer"], 42);
    }

    #[test]
    fn json_tool_result_also_has_text_fallback() {
        // `structured()` appends a text block alongside the
        // structured payload, for clients that only render text.
        let response = Json(TestOut { answer: 42 })
            .into_call_tool_result()
            .unwrap();
        let CallToolResponse::Complete(result) = response else {
            panic!("expected Complete")
        };
        assert!(result
            .content
            .iter()
            .any(|c| matches!(c, ContentBlock::Text(t) if t.text.contains("42")),));
    }

    #[test]
    fn json_tool_declares_output_schema() {
        // The `#[tool]` macro detects the `Json<T>` return type
        // and stamps `outputSchema` on the Tool — this is what
        // makes the structuredContent machine-checkable for the
        // client.
        let fixture = JsonToolFixture::default();
        let tool = fixture.tool_router.get("echo").expect("tool registered");
        assert!(tool.output_schema.is_some());
    }

    #[test]
    fn structured_content_wire_key_is_camel_case() {
        // Pin the wire key: `structuredContent`, top level of the
        // CallToolResult — not `structured_content`.
        let result = CallToolResult::structured(json!({"answer": 42}));
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["structuredContent"]["answer"], 42);
        assert!(json.get("structured_content").is_none());
    }

    // -- tasks (SEP-2663) -------------------------------------

    #[tokio::test]
    async fn task_spawn_then_get_reports_working() {
        // A task that hasn't finished and hasn't asked for input
        // reports `working` — the seed state a client sees right
        // after the `CreateTaskResult` round-trips.
        let manager = TaskManager::new();
        let task = manager.spawn(TaskOptions::default(), |_ctx| {
            Box::pin(std::future::pending())
        });
        let detailed = manager.get_task(&task.task_id).unwrap();
        assert_eq!(detailed.status(), TaskStatus::Working);
        manager.shutdown();
    }

    #[tokio::test]
    async fn task_completes_to_completed_status() {
        let manager = TaskManager::new();
        let task = manager.spawn(TaskOptions::default(), |_ctx| {
            Box::pin(async { Ok(CallToolResult::success(vec![ContentBlock::text("42")])) })
        });
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let detailed = manager.get_task(&task.task_id).unwrap();
            if detailed.status() == TaskStatus::Completed {
                assert!(matches!(detailed.payload, TaskPayload::Completed { .. }));
                return;
            }
        }
        panic!("task did not complete");
    }

    #[tokio::test]
    async fn task_request_input_flips_to_input_required_then_update_delivers() {
        // The MRTR-in-task loop: the operation surfaces an
        // elicitation via `request_input`, the client polls
        // `tasks/get` (sees `input_required` + the request),
        // answers via `tasks/update`, and the operation resumes.
        let manager = TaskManager::new();
        let task = manager.spawn(TaskOptions::default(), |ctx| {
            Box::pin(async move {
                let request: InputRequest = serde_json::from_value(json!({
                    "method": "elicitation/create",
                    "params": {
                        "message": "Pick one",
                        "requestedSchema": {"type": "object", "properties": {}}
                    }
                }))
                .map_err(|e| rmcp::ErrorData::internal_error(e.to_string(), None))?;
                let response = ctx.request_input("input", request).await?;
                let ok = response
                    .get("action")
                    .and_then(|v| v.as_str())
                    .is_some_and(|a| a == "accept");
                Ok(CallToolResult::success(vec![ContentBlock::text(if ok {
                    "accepted"
                } else {
                    "declined"
                })]))
            })
        });

        let mut saw_input_required = false;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let detailed = manager.get_task(&task.task_id).unwrap();
            if let TaskPayload::InputRequired { input_requests } = &detailed.payload {
                assert!(input_requests.contains_key("input"));
                saw_input_required = true;
                break;
            }
        }
        assert!(saw_input_required, "task never reached input_required");

        manager
            .update_task(
                &task.task_id,
                [("input".to_string(), json!({"action": "accept"}))],
            )
            .unwrap();

        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let detailed = manager.get_task(&task.task_id).unwrap();
            if detailed.status() == TaskStatus::Completed {
                return;
            }
        }
        panic!("task did not complete after input response");
    }

    #[tokio::test]
    async fn task_cancel_is_cooperative() {
        // `tasks/cancel` acks immediately without forcing a
        // terminal state; the operation observes it, exits with
        // `TaskExit::Cancelled`, and the task settles as
        // `cancelled`.
        let manager = TaskManager::new();
        let task = manager.spawn(TaskOptions::default(), |ctx| {
            Box::pin(async move {
                tokio::select! {
                    _ = ctx.cancelled() => Err(TaskExit::Cancelled),
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                        Ok(CallToolResult::success(vec![ContentBlock::text("never")]))
                    }
                }
            })
        });
        manager.cancel_task(&task.task_id).unwrap();
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let detailed = manager.get_task(&task.task_id).unwrap();
            if detailed.status().is_terminal() {
                assert_eq!(detailed.status(), TaskStatus::Cancelled);
                return;
            }
        }
        panic!("task did not settle after cancel");
    }

    #[tokio::test]
    async fn task_unknown_id_is_invalid_params() {
        let manager = TaskManager::new();
        let err = manager.get_task("nope").unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn nestrs_mcp_server_spawn_task_registers_and_returns_task() {
        // The embedder-facing shortcut: spawn through the server,
        // then read the task back through the same manager the
        // `tasks/get` override delegates to.
        let server = crate::server::NestrsMcpServer::new();
        let task = server.spawn_task(TaskOptions::default(), |_ctx| {
            Box::pin(std::future::pending())
        });
        let detailed = server.tasks().get_task(&task.task_id).unwrap();
        assert_eq!(detailed.status(), TaskStatus::Working);
        server.tasks().shutdown();
    }

    #[test]
    fn nestrs_mcp_server_default_does_not_advertise_tasks() {
        // Opt-in: a server that never spawns tasks shouldn't
        // declare the extension (the SDK would then accept task
        // RPCs only to have nothing behind them).
        let server = crate::server::NestrsMcpServer::new();
        let info = server.get_info();
        let declared = info
            .capabilities
            .extensions
            .is_some_and(|e| e.contains_key(TASKS_EXTENSION_ID));
        assert!(!declared);
    }

    #[test]
    fn nestrs_mcp_server_with_task_support_advertises_tasks() {
        let server = crate::server::NestrsMcpServer::new().with_task_support();
        let info = server.get_info();
        let declared = info
            .capabilities
            .extensions
            .is_some_and(|e| e.contains_key(TASKS_EXTENSION_ID));
        assert!(declared);
    }

    #[tokio::test]
    async fn create_task_result_wire_shape_has_result_type_task() {
        // `CreateTaskResult` serializes with `resultType: "task"`
        // and the task fields flattened at the top level — the
        // `tools/call` body a taskable tool returns
        // (`CallToolResponse::Task(...)` wraps exactly this).
        let manager = TaskManager::new();
        let task = manager.spawn(TaskOptions::default(), |_ctx| {
            Box::pin(std::future::pending())
        });
        let json = serde_json::to_value(CreateTaskResult::new(task)).unwrap();
        assert_eq!(json["resultType"], "task");
        assert!(json["taskId"].is_string());
        assert_eq!(json["status"], "working");
        manager.shutdown();
    }
}
