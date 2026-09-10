//! Regression: `HttpService` must apply outbound timeouts.
//!
//! reqwest has **no** default timeout — before this fix, a TCP-connected but
//! unresponsive upstream hung the calling handler's future forever, and
//! concurrent hung calls accumulated. `HttpService` now defaults to a 30s
//! whole-request / 10s connect deadline, tunable via `HttpServiceOptions`
//! (raw `reqwest::Client` access stays available via `.client()`).
#![cfg(feature = "http-client")]

use nestrs::{HttpService, HttpServiceOptions, DEFAULT_CONNECT_TIMEOUT, DEFAULT_REQUEST_TIMEOUT};
use std::net::TcpListener;
use std::time::{Duration, Instant};

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

#[tokio::test]
async fn request_timeout_fires_against_hung_upstream() {
    // An upstream that accepts the TCP connection but never writes a response.
    // The kernel completes the handshake into the backlog, so reqwest's
    // connect succeeds and the request then stalls waiting for headers —
    // exactly the "connected but unresponsive" case with no built-in deadline.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    std::thread::spawn(move || {
        if let Ok((stream, _)) = listener.accept() {
            // Hold the connection open without ever responding.
            std::thread::sleep(Duration::from_secs(10));
            drop(stream);
        }
    });

    let service = HttpService::from_options(&HttpServiceOptions {
        request_timeout: Duration::from_millis(150),
        connect_timeout: Duration::from_secs(2),
    });

    let started = Instant::now();
    let result = service
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await;
    let elapsed = started.elapsed();

    let err = result.expect_err("a request against a hung upstream must fail, not hang");
    assert!(err.is_timeout(), "expected a timeout error, got: {err}");
    // The deadline (not a fast connection error) ended the request: a refused
    // or reset connection returns in ~1-5ms, which the 150ms deadline cannot.
    assert!(
        elapsed >= Duration::from_millis(100),
        "the request should have waited for the deadline; elapsed {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the deadline must fire promptly; elapsed {elapsed:?}"
    );
}
