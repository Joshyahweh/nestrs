//! NestJS **`HttpModule` / `HttpService`** analogue (feature: **`http-client`**).
//!
//! Thin re-export shim over the `nestrs-http` workspace member. Real
//! implementation lives in `nestrs_http::*`; the umbrella keeps the same
//! paths so existing code (`nestrs::HttpService`, `HttpServiceOptions`,
//! `HttpModule`, `DEFAULT_REQUEST_TIMEOUT`, `DEFAULT_CONNECT_TIMEOUT`)
//! keeps compiling unchanged.

#[cfg(feature = "http-client")]
pub use nestrs_http::{
    HttpModule, HttpService, HttpServiceOptions, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT,
};