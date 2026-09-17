//! Regression: `HttpService` must apply outbound timeouts.
//!
//! reqwest has **no** default timeout — before this fix, a TCP-connected but
//! unresponsive upstream hung the calling handler's future forever, and
//! concurrent hung calls accumulated. `HttpService` now defaults to a 30s
//! whole-request / 10s connect deadline, tunable via `HttpServiceOptions`
//! (raw `reqwest::Client` access stays available via `.client()`).

use nestrs_http::{
    HttpService, HttpServiceOptions, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT,
};
use std::time::Duration;

#[test]
fn default_options_carry_sane_timeouts() {
    assert_eq!(
        DEFAULT_REQUEST_TIMEOUT,
        Duration::from_secs(30),
        "the whole-request default must stay finite and bounded"
    );
    assert_eq!(
        DEFAULT_CONNECT_TIMEOUT,
        Duration::from_secs(10),
        "the connect default must stay finite and bounded"
    );
    let options = HttpServiceOptions::default();
    assert_eq!(options.request_timeout, DEFAULT_REQUEST_TIMEOUT);
    assert_eq!(options.connect_timeout, DEFAULT_CONNECT_TIMEOUT);
}

#[test]
fn from_options_produces_a_working_client() {
    let svc = HttpService::from_options(&HttpServiceOptions::default());
    // Borrow the underlying client — the typed wrapper just hands back the
    // reqwest handle, so any successful reqwest operation is reachable.
    let _ = svc.client();
}

#[test]
fn request_builder_helpers_return_typed_builders() {
    let svc = HttpService::from_options(&HttpServiceOptions::default());
    let _ = svc.get("https://example.com");
    let _ = svc.post("https://example.com");
    let _ = svc.put("https://example.com");
    let _ = svc.patch("https://example.com");
    let _ = svc.delete("https://example.com");
}
