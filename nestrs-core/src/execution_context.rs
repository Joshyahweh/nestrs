use axum::http::request::Parts;
use std::any::TypeId;

/// Active host kind (NestJS [`ArgumentsHost#getType`](https://docs.nestjs.com/fundamentals/execution-context)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostType {
    Http,
    Rpc,
    Ws,
    Unknown,
}

/// Cross-cutting context similar to NestJS [`ExecutionContext`](https://docs.nestjs.com/fundamentals/execution-context)
/// / `ArgumentsHost` for HTTP handlers (RPC/WS can set [`HostType`] when wired manually).
///
/// NestJS 12 parity: optional controller/handler type ids and names so guards
/// and interceptors can mirror `getClass()` / `getHandler()` without reflection.
#[derive(Clone, Debug)]
pub struct ExecutionContext {
    host_type: HostType,
    pub method: axum::http::Method,
    pub path_and_query: String,
    controller_type_id: Option<TypeId>,
    controller_name: Option<&'static str>,
    handler_type_id: Option<TypeId>,
    handler_name: Option<&'static str>,
}

impl ExecutionContext {
    pub fn from_http_parts(parts: &Parts) -> Self {
        let path_and_query = parts
            .uri
            .path_and_query()
            .map(|pq| pq.as_str().to_owned())
            .unwrap_or_else(|| parts.uri.path().to_owned());
        Self {
            host_type: HostType::Http,
            method: parts.method.clone(),
            path_and_query,
            controller_type_id: None,
            controller_name: None,
            handler_type_id: None,
            handler_name: None,
        }
    }

    /// Attach Nest-style class/handler identity (set by the route pipeline).
    pub fn with_handler_meta(
        mut self,
        controller_name: Option<&'static str>,
        controller_type_id: Option<TypeId>,
        handler_name: Option<&'static str>,
        handler_type_id: Option<TypeId>,
    ) -> Self {
        self.controller_name = controller_name;
        self.controller_type_id = controller_type_id;
        self.handler_name = handler_name;
        self.handler_type_id = handler_type_id;
        self
    }

    pub fn get_type(&self) -> HostType {
        self.host_type
    }

    /// Nest `getClass()` analogue — TypeId of the controller type when known.
    pub fn get_class(&self) -> Option<TypeId> {
        self.controller_type_id
    }

    /// Nest `getHandler()` analogue — TypeId of the handler fn item when known.
    pub fn get_handler(&self) -> Option<TypeId> {
        self.handler_type_id
    }

    /// Stable controller path string (module::Type) when registered.
    pub fn get_class_name(&self) -> Option<&'static str> {
        self.controller_name
    }

    /// Stable handler path string (module::Type::method) when registered.
    pub fn get_handler_name(&self) -> Option<&'static str> {
        self.handler_name
    }

    /// Override host type for RPC/WS entrypoints.
    pub fn with_host_type(mut self, host_type: HostType) -> Self {
        self.host_type = host_type;
        self
    }

    /// 32-hex W3C trace id from the ambient trace context, when one was
    /// installed (see `nestrs::core::with_trace_context`).
    pub fn trace_id(&self) -> Option<String> {
        crate::current_trace_context().map(|t| t.trace_id_hex())
    }

    /// 16-hex W3C span id from the ambient trace context.
    pub fn span_id(&self) -> Option<String> {
        crate::current_trace_context().map(|t| t.span_id_hex())
    }

    /// 2-hex W3C trace flags from the ambient trace context.
    pub fn trace_flags(&self) -> Option<String> {
        crate::current_trace_context().map(|t| t.trace_flags_hex())
    }

    pub fn switch_to_http(&self) -> HttpExecutionArguments<'_> {
        HttpExecutionArguments { ctx: self }
    }

    pub fn switch_to_rpc(&self) -> RpcExecutionArguments<'_> {
        RpcExecutionArguments { ctx: self }
    }

    pub fn switch_to_ws(&self) -> WsExecutionArguments<'_> {
        WsExecutionArguments { ctx: self }
    }
}

/// NestJS `switchToHttp().getRequest()`-style view without storing the full Axum request type.
pub struct HttpExecutionArguments<'a> {
    ctx: &'a ExecutionContext,
}

impl<'a> HttpExecutionArguments<'a> {
    pub fn method(&self) -> &'a axum::http::Method {
        &self.ctx.method
    }

    pub fn path_and_query(&self) -> &'a str {
        self.ctx.path_and_query.as_str()
    }
}

/// NestJS `switchToRpc()`-style view.
pub struct RpcExecutionArguments<'a> {
    ctx: &'a ExecutionContext,
}

impl<'a> RpcExecutionArguments<'a> {
    pub fn pattern(&self) -> &'a str {
        self.ctx.path_and_query.as_str()
    }

    pub fn handler_name(&self) -> Option<&'static str> {
        self.ctx.handler_name
    }
}

/// NestJS `switchToWs()`-style view.
pub struct WsExecutionArguments<'a> {
    ctx: &'a ExecutionContext,
}

impl<'a> WsExecutionArguments<'a> {
    pub fn path_and_query(&self) -> &'a str {
        self.ctx.path_and_query.as_str()
    }

    pub fn handler_name(&self) -> Option<&'static str> {
        self.ctx.handler_name
    }
}
