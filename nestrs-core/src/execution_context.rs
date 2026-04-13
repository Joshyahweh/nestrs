use axum::http::request::Parts;

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
#[derive(Clone, Debug)]
pub struct ExecutionContext {
    host_type: HostType,
    pub method: axum::http::Method,
    pub path_and_query: String,
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
        }
    }

    pub fn get_type(&self) -> HostType {
        self.host_type
    }

    pub fn switch_to_http(&self) -> HttpExecutionArguments<'_> {
        HttpExecutionArguments { ctx: self }
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
