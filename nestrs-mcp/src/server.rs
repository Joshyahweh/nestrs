//! Top-level MCP server. Aggregates the four tool routers
//! (introspection, runtime, scaffold, docs) into a single
//! `ServerHandler` so the binary can serve one `--stdio` or `--http`
//! endpoint with all tools registered.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::ServerHandler;
use rmcp::model::*;
#[cfg(feature = "elicitation")]
use rmcp::service::ElicitationMode;
use rmcp::service::RequestContext;
use rmcp::task_manager::{TaskContext, TaskFuture, TaskManager, TaskOptions};
use rmcp::RoleServer;
use rmcp::{tool_handler, tool_router};

use crate::surfaces::McpSurfaces;
use crate::tools::{
    docs::DocsTools, introspection::IntrospectionTools, runtime::RuntimeTools,
    scaffold::ScaffoldTools,
};

#[cfg(feature = "authz")]
use crate::mcp_data_context::{
    commit_or_rollback, mask_response, run_with_mcp_scopes, McpDataContext,
};
#[cfg(feature = "authz")]
use nestrs::TransactionSlot;

/// A list of tools the model can call. Merges all four sub-routers so
/// we can serve a single tool-list to the client.
pub struct NestrsMcpServer {
    pub tool_router: ToolRouter<Self>,
    /// Optional per-server data context. When `Some`, the
    /// `call_tool` override (under `feature = "authz"`) installs the
    /// per-task ability + transaction scopes and post-masks the
    /// response. When `None`, behaviour is unchanged.
    #[cfg(feature = "authz")]
    pub data_context: McpDataContext,
    /// Prompts, resources, resource templates, and the
    /// completion handler. Default is empty (the protocol
    /// methods return "no such surface"). Use
    /// [`Self::with_surfaces`] to register any of them.
    pub surfaces: McpSurfaces,
    /// SEP-2663 task store. Always present so `spawn_task` works
    /// even without the capability advertised; the `tasks/get` /
    /// `tasks/update` / `tasks/cancel` RPCs only resolve when the
    /// extension is advertised (see [`Self::with_task_support`]).
    tasks: TaskManager,
    /// Whether the SEP-1724 `io.modelcontextprotocol/tasks`
    /// extension is advertised in `get_info`. Opt-in — a server
    /// that never spawns tasks shouldn't tell clients it supports
    /// them.
    task_support: bool,
}

impl Default for NestrsMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

// `TaskManager` doesn't implement `Debug`, so the derive can't be
// used. Summarize the store as its running-task count instead of
// dragging the whole inner state into the output.
impl std::fmt::Debug for NestrsMcpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("NestrsMcpServer");
        s.field("tool_router", &self.tool_router)
            .field("surfaces", &self.surfaces)
            .field("tasks", &self.tasks.running_task_count())
            .field("task_support", &self.task_support);
        // The data-context field only exists under `feature = "authz"`.
        #[cfg(feature = "authz")]
        s.field("data_context", &self.data_context);
        s.finish()
    }
}

impl NestrsMcpServer {
    pub fn new() -> Self {
        // We don't aggregate sub-routers here because the rmcp router
        // type is invariant in `Self`. Each `*Tools` struct is meant to
        // be served as its own handler when the user wants one area;
        // for the all-tools case, the binary spawns one server per
        // area and the MCP client multiplexes. The top-level
        // `NestrsMcpServer` exists so embedders (e.g. `nestrs-cli`) can
        // mount a single default handler without picking an area.
        Self {
            tool_router: ToolRouter::<Self>::new(),
            #[cfg(feature = "authz")]
            data_context: McpDataContext::new(),
            surfaces: McpSurfaces::new(),
            tasks: TaskManager::new(),
            task_support: false,
        }
    }

    /// Attach a data context that controls the per-tool-call ambient
    /// ability + transaction + outbound masking. Opt-in — the
    /// returned server behaves identically to the default until a
    /// tool is called, at which point `current_mcp_ability` /
    /// `current_mcp_transaction` are visible inside the tool body
    /// and the response is post-masked.
    #[cfg(feature = "authz")]
    pub fn with_data_context(mut self, ctx: McpDataContext) -> Self {
        self.data_context = ctx;
        self
    }

    /// Attach the protocol surfaces (prompts / resources /
    /// templates / completion). Pass the same `McpSurfaces`
    /// you've been building up; the server clones the
    /// `Arc` inside, so registrations added after `with_surfaces`
    /// are visible on the server.
    pub fn with_surfaces(mut self, surfaces: McpSurfaces) -> Self {
        self.surfaces = surfaces;
        self
    }

    /// Advertise the SEP-2663 tasks extension (`tasks/get`,
    /// `tasks/update`, `tasks/cancel`) in `get_info`. Opt-in.
    /// The [`TaskManager`] exists either way — this only controls
    /// whether the capability is declared to clients, which also
    /// controls whether the SDK accepts task RPCs from them
    /// (undeclared → `-32601`).
    pub fn with_task_support(mut self) -> Self {
        self.task_support = true;
        self
    }

    /// The task store backing the SEP-2663 extension. Use it (or
    /// the [`Self::spawn_task`] shortcut) from inside a tool body
    /// to run long-running work asynchronously: return
    /// `CallToolResponse::Task(CreateTaskResult::new(task))` from
    /// the tool, then the client polls with `tasks/get`.
    pub fn tasks(&self) -> &TaskManager {
        &self.tasks
    }

    /// Spawn an operation as a task and return its seed [`Task`]
    /// state for a `CreateTaskResult`. Thin passthrough to
    /// [`TaskManager::spawn`] — the future must be `'static`, so
    /// move owned data in rather than borrowing `self`.
    pub fn spawn_task(
        &self,
        options: TaskOptions,
        make_future: impl FnOnce(TaskContext) -> TaskFuture,
    ) -> Task {
        self.tasks.spawn(options, make_future)
    }

    /// Elicit a response from the client during a tool call
    /// (SEP-1034). Returns the client's `ElicitResult` (Accept /
    /// Decline / Cancel). If the client didn't declare
    /// `elicitation` as a capability, returns a `Cancel`
    /// result rather than erroring — the spec lets servers
    /// choose, and `Cancel` is the kinder shape for the
    /// client's UI.
    ///
    /// Only available with the `elicitation` feature. Without
    /// it the function isn't compiled in; tools that want
    /// MRTR-for-tools via the lower-level `elicit_input`
    /// helper (in `surfaces.rs`) don't need it.
    #[cfg(feature = "elicitation")]
    pub async fn elicit(
        &self,
        request: ElicitRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ElicitResult, rmcp::ErrorData> {
        // Capability probe. `supported_elicitation_modes`
        // returns an empty set when the client didn't
        // advertise any elicitation capability (form or url).
        if !context
            .peer
            .supported_elicitation_modes()
            .contains(&ElicitationMode::Form)
        {
            return Ok(ElicitResult::new(ElicitationAction::Cancel));
        }
        // Hand off to the user-supplied handler.
        let handler = self
            .surfaces
            .elicitation_handler()
            .ok_or_else(|| ErrorData::invalid_params("no elicitation handler registered", None))?;
        handler(request, context)
            .map_err(|e| ErrorData::internal_error(format!("elicitation failed: {e}"), None))
    }

    /// All four sub-servers. Each is a fully-formed `ServerHandler`
    /// that can be served on its own transport (so the binary can
    /// spawn four in parallel if the MCP client expects a multi-handler
    /// model). The `merge` helper here exists for symmetry with future
    /// `crate::tools::*` composition.
    pub fn sub_servers() -> SubServers {
        SubServers {
            introspection: IntrospectionTools,
            runtime: RuntimeTools,
            scaffold: ScaffoldTools,
            docs: DocsTools,
        }
    }
}

/// The four area-specific servers. Each can be served on its own
/// transport. The top-level `NestrsMcpServer` is a thin shim.
#[derive(Debug)]
pub struct SubServers {
    pub introspection: IntrospectionTools,
    pub runtime: RuntimeTools,
    pub scaffold: ScaffoldTools,
    pub docs: DocsTools,
}

#[tool_router]
impl NestrsMcpServer {
    // No tools here — `NestrsMcpServer` is just the wrapper. Use
    // `SubServers` to get the actual tool-bearing handlers.
}

#[tool_handler]
impl ServerHandler for NestrsMcpServer {
    fn get_info(&self) -> ServerInfo {
        let mut caps = ServerCapabilities::default();
        // Only advertise `resources.subscribe` if the user
        // marked at least one URI as subscribable. Otherwise
        // the protocol client would happily try to subscribe
        // to nothing and we'd just respond with an empty
        // accepted filter. Cheap to gate; saves a round trip.
        if self.surfaces.has_subscribable_resources() {
            caps.resources.get_or_insert_default().subscribe = Some(true);
        }
        // SEP-1724 extension map: the SEP-2663 tasks extension is
        // an entry in `extensions`, not a top-level capability.
        // Same wire result as `ServerCapabilities::builder().enable_tasks()`
        // but composes with the manual capability building above.
        if self.task_support {
            caps.extensions
                .get_or_insert_default()
                .insert(TASKS_EXTENSION_ID.to_string(), JsonObject::new());
        }
        InitializeResult::new(caps)
            .with_server_info(
                Implementation::new("nestrs-mcp", env!("CARGO_PKG_VERSION"))
                    .with_title("nestrs Model Context Protocol server")
                    .with_description(
                        "Introspection, live runtime, scaffolding, and docs search for nestrs.",
                    )
                    .with_website_url("https://github.com/Joshyahweh/nestrs/tree/main/nestrs-mcp"),
            )
            .with_instructions(
                "nestrs-mcp exposes nestrs project structure, live runtime, \
                 scaffolding actions, and docs search. Each tool takes a \
                 `workspace_path` for source-level operations; runtime tools \
                 take a `base_url` and optional `token`.",
            )
    }
    async fn initialize(
        &self,
        _request: InitializeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, rmcp::ErrorData> {
        Ok(self.get_info())
    }

    /// Per-tool-call dispatch with the optional ambient ability +
    /// transaction + outbound masking. Without `feature = "authz"`,
    /// the macro-generated default fires (plain
    /// `self.tool_router.call(tcc).await`). With the feature on, the
    /// override installs the per-task scopes, dispatches, then
    /// post-masks + commits/rolls back.
    #[cfg(feature = "authz")]
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, rmcp::ErrorData> {
        // Snapshot data context before the async move.
        let ctx = self.data_context.clone();
        // Capture the ability for the post-mask step. None means
        // "no masking" — we still want to commit/rollback based on
        // the response.
        let ability_for_mask = ctx.ability.clone();
        // Pre-open a tx so we can decide commit/rollback after the
        // dispatch returns. The slot is also installed into the
        // per-task scope inside `run_with_mcp_scopes` so the tool
        // body can read it via `current_mcp_transaction`.
        let slot: Option<std::sync::Arc<TransactionSlot>> = if let Some(pool) = &ctx.pool {
            let pool = pool.clone();
            match pool.begin().await {
                Ok(tx) => Some(std::sync::Arc::new(TransactionSlot::new(tx))),
                Err(e) => {
                    tracing::warn!(
                        target: "nestrs::mcp_data_context",
                        "failed to open per-tool-call transaction: {e}"
                    );
                    None
                }
            }
        } else {
            None
        };
        let slot_for_post = slot.clone();

        // Inner dispatch: install scopes, run the tool router, mask
        // the response. The `&self` reference here is fine because
        // `run_with_mcp_scopes` takes a future that's polled on the
        // current task and the response future just borrows `self`
        // for the duration of the dispatch.
        let dispatch_result = run_with_mcp_scopes(&ctx, slot, async {
            let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
            self.tool_router.call(tcc).await
        })
        .await;

        // Match on the dispatch outcome:
        // - `Err(e)`: tool wasn't found / routing failed / RPC-level
        //   error. The slot's `Drop` would roll back, but be explicit
        //   so the warning log surfaces on a stale connection.
        // - `Ok(resp)`: post-mask, then commit/rollback based on
        //   whether the tool itself flagged `is_error`.
        let mut response = match dispatch_result {
            Ok(r) => r,
            Err(e) => {
                commit_or_rollback(slot_for_post, &tool_error_marker()).await;
                return Err(e);
            }
        };

        // Post-mask the response in place if we have an ability.
        if let Some(ability) = ability_for_mask.as_ref() {
            mask_response(&mut response, ability);
        }
        // Commit on success, roll back on tool-level error. The
        // slot's `Drop` would also roll back, but explicit commit
        // surfaces the success path and the warning log on commit
        // failure (e.g. broken connection).
        commit_or_rollback(slot_for_post, &response).await;
        Ok(response)
    }

    // -------------------------------------------------------------
    // Protocol surfaces beyond tools. Each override reads from
    // `self.surfaces`; the default (empty) `McpSurfaces` produces
    // empty lists and "not found" errors, which is the correct
    // MCP-flavoured "this server doesn't expose this surface" shape.
    // -------------------------------------------------------------

    fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListPromptsResult, rmcp::ErrorData>> + Send + '_
    {
        let supported = protocol_supports_cache_hints(&context);
        let hints = self.surfaces.cache_hints();
        let prompts = self.surfaces.list_prompts();
        let result = crate::surfaces::attach_cache_hints(
            ListPromptsResult::with_all_items(prompts),
            hints,
            supported,
        );
        async move { Ok(result) }
    }

    fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetPromptResponse, rmcp::ErrorData>> + Send + '_
    {
        let result = self.surfaces.get_prompt(&request);
        async move { result }
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourcesResult, rmcp::ErrorData>> + Send + '_
    {
        let supported = protocol_supports_cache_hints(&context);
        let hints = self.surfaces.cache_hints();
        let resources = self.surfaces.list_resources();
        let result = crate::surfaces::attach_cache_hints(
            ListResourcesResult::with_all_items(resources),
            hints,
            supported,
        );
        async move { Ok(result) }
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListResourceTemplatesResult, rmcp::ErrorData>>
           + Send
           + '_ {
        let supported = protocol_supports_cache_hints(&context);
        let hints = self.surfaces.cache_hints();
        let templates = self.surfaces.list_resource_templates();
        let result = crate::surfaces::attach_cache_hints(
            ListResourceTemplatesResult::with_all_items(templates),
            hints,
            supported,
        );
        async move { Ok(result) }
    }

    fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ReadResourceResponse, rmcp::ErrorData>> + Send + '_
    {
        let supported = protocol_supports_cache_hints(&context);
        let hints = self.surfaces.cache_hints();
        let result = self.surfaces.read_resource(&request);
        async move {
            result.map(|response| match response {
                ReadResourceResponse::Complete(inner) => ReadResourceResponse::Complete(
                    crate::surfaces::attach_cache_hints(inner, hints, supported),
                ),
                other => other,
            })
        }
    }

    fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CompleteResult, rmcp::ErrorData>> + Send + '_
    {
        let result = self.surfaces.complete(&request);
        async move { result }
    }

    // -------------------------------------------------------------
    // Subscriptions (SEP-1696 / MCP 2026-07-28). The `listen`
    // override parks on `SubscriptionContext::cancelled` so the
    // sink stays alive for the duration of the subscription.
    // The `accepted_subscription_filter` override returns
    // whatever URIs the user has marked subscribable that
    // intersect with what the client asked for. Legacy
    // `subscribe` / `unsubscribe` overrides below keep
    // pre-2026-07-28 peers happy (the SDK auto-routes to the
    // deprecated methods for those versions).
    // -------------------------------------------------------------

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        // Empty filter (no subscribable resources registered
        // and the client asked for nothing) — return `None`
        // to signal "subscriptions/listen not implemented."
        if !self.surfaces.has_subscribable_resources()
            && requested
                .resource_subscriptions
                .as_ref()
                .map_or(true, |v| v.is_empty())
        {
            return None;
        }
        let accepted = self.surfaces.accepted_filter_for(requested);
        // If the accepted filter has nothing the client
        // asked for, also return `None` to skip the
        // acknowledge round trip.
        if accepted
            .resource_subscriptions
            .as_ref()
            .map_or(true, |v| v.is_empty())
        {
            return None;
        }
        Some(accepted)
    }

    fn listen(
        &self,
        context: rmcp::service::SubscriptionContext,
    ) -> impl std::future::Future<Output = Result<(), rmcp::ErrorData>> + Send + '_ {
        // The SDK hands us a fresh sink per subscription. We
        // clone it into `McpSurfaces` so the resource-write
        // path can fan out updates, then park on `cancelled`
        // so the subscription stays alive.
        //
        // `SubscriptionContext` doesn't expose a stable id, so
        // we generate one. The id only needs to be unique
        // within this `McpSurfaces` (the HashMap key) and is
        // used to remove the sink on cancel.
        let id = new_subscription_id();
        self.surfaces
            .add_subscription_sink(id.clone(), context.sink().clone());
        async move {
            context.cancelled().await;
            self.surfaces.remove_subscription_sink(&id);
            Ok(())
        }
    }

    // -------------------------------------------------------------
    // Tasks (SEP-2663, `io.modelcontextprotocol/tasks`). All three
    // RPCs delegate to the `TaskManager`; the SDK rejects them
    // with `-32601` before reaching these overrides unless the
    // extension was advertised via `with_task_support`.
    // -------------------------------------------------------------

    fn get_task(
        &self,
        request: GetTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<GetTaskResult, rmcp::ErrorData>> + Send + '_ {
        // `TaskManager::get_task` yields the `DetailedTask`; the
        // wire result wraps it (`resultType: "complete"` + the
        // flattened task/payload fields).
        let result = self
            .tasks
            .get_task(&request.task_id)
            .map(GetTaskResult::new);
        async move { result }
    }

    fn update_task(
        &self,
        request: UpdateTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), rmcp::ErrorData>> + Send + '_ {
        let result = self
            .tasks
            .update_task(&request.task_id, request.input_responses);
        async move { result }
    }

    fn cancel_task(
        &self,
        request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<(), rmcp::ErrorData>> + Send + '_ {
        let result = self.tasks.cancel_task(&request.task_id);
        async move { result }
    }
}

/// SEP-2549 cache hints are only understood by peers on protocol
/// version 2026-07-28 or later. Older peers get results without
/// the `ttlMs` / `cacheScope` fields.
fn protocol_supports_cache_hints(context: &RequestContext<RoleServer>) -> bool {
    context
        .protocol_version()
        .is_some_and(|v| v >= ProtocolVersion::V_2026_07_28)
}

/// Monotonic counter used to mint subscription ids. We need
/// stable-ish ids per `listen` call so the
/// `remove_subscription_sink` call on cancel finds the right
/// entry. The id has no protocol meaning; it's purely a
/// `McpSurfaces` HashMap key.
fn new_subscription_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("sub-{n}")
}

/// Marker response used to drive the commit/rollback policy on the
/// dispatch-error path. Treated as a tool-level error so any
/// pre-opened transaction is rolled back.
#[cfg(feature = "authz")]
fn tool_error_marker() -> rmcp::model::CallToolResponse {
    rmcp::model::CallToolResult::error(vec![rmcp::model::ContentBlock::text("dispatch error")])
        .into()
}
