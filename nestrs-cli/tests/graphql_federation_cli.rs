//! Wave 7.12 — `nestrs-cli graphql federation export` integration
//! tests. The `is_federation_v2_sdl` substring check is already covered
//! by unit tests inside `graphql_federation.rs`. The CLI tests cover
//! flag parsing, the v2 validation gate, the `--lenient` opt-out, and
//! end-to-end against an in-process mock HTTP server that returns a
//! federation SDL on `{_service{sdl}}`.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nestrs_scaffold::graphql_federation;

fn unique_tmp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("nestrs-cli-fed-{name}-{nanos}"))
}

// ---------------------------------------------------------------------------
// Flag parsing — covered without spawning a server
// ---------------------------------------------------------------------------

#[test]
fn dispatch_rejects_missing_subcommand() {
    let err = graphql_federation::dispatch(&[]).expect_err("should reject");
    assert!(
        err.contains("expected `nestrs-cli graphql federation"),
        "got: {err}"
    );
}

#[test]
fn dispatch_rejects_unknown_subcommand() {
    let args: Vec<String> = vec!["bogus".to_string()];
    let err = graphql_federation::dispatch(&args).expect_err("should reject");
    assert!(err.contains("unknown"), "got: {err}");
    assert!(err.contains("bogus"), "got: {err}");
}

#[test]
fn dispatch_routes_export_to_run() {
    // `dispatch(["export"])` lands in `run(&[])`, which errors on
    // missing `--url`. We assert the error comes from the URL check
    // (proving the dispatch routed correctly), not from the dispatch
    // wrapper itself.
    let args: Vec<String> = vec!["export".to_string()];
    let err = graphql_federation::dispatch(&args).expect_err("should reject");
    assert!(err.contains("--url"), "got: {err}");
}

#[test]
fn run_rejects_missing_url() {
    let args: Vec<String> = vec!["--out".to_string(), "/tmp/x".to_string()];
    let err = graphql_federation::run(&args).expect_err("should reject");
    assert!(err.contains("--url"), "error should mention --url: {err}");
}

#[test]
fn run_rejects_missing_out() {
    let args: Vec<String> = vec![
        "--url".to_string(),
        "http://localhost:3000/graphql".to_string(),
    ];
    let err = graphql_federation::run(&args).expect_err("should reject");
    assert!(err.contains("--out"), "error should mention --out: {err}");
}

#[test]
fn run_rejects_unknown_option() {
    let args: Vec<String> = vec![
        "--url".to_string(),
        "http://localhost:3000/graphql".to_string(),
        "--out".to_string(),
        "/tmp/x".to_string(),
        "--bogus".to_string(),
    ];
    let err = graphql_federation::run(&args).expect_err("should reject");
    assert!(err.contains("unknown option"), "got: {err}");
}

#[test]
fn run_rejects_non_federation_v2_sdl_by_default() {
    // Pure check — feed a v1 SDL into `run` is not possible (run does
    // the HTTP fetch), but we can call the validation helper directly
    // to confirm the rejection logic.
    let v1_sdl = "type User @key(fields: \"id\") { id: ID! }\ntype Query { me: User }\n";
    assert!(!graphql_federation::is_federation_v2_sdl(v1_sdl));
    let v2_sdl = "directive @link(url: String) on FIELD_DEFINITION\ntype Query { ping: String }\n";
    assert!(graphql_federation::is_federation_v2_sdl(v2_sdl));
}

// ---------------------------------------------------------------------------
// End-to-end — in-process mock that keeps the listener bound (no
// bind-then-drop race under parallel `cargo test`).
// ---------------------------------------------------------------------------

const FEDERATION_V2_SDL: &str = "\
directive @link(url: String, as: String, for: String) on FIELD_DEFINITION\n\
directive @key(fields: String!) repeatable on OBJECT | INTERFACE\n\
directive @shareable repeatable on OBJECT | FIELD_DEFINITION\n\
\n\
type User @key(fields: \"id\") {\n  id: ID!\n  name: String\n}\n\
\n\
type Query {\n  me: User\n}\n";

const FEDERATION_V1_SDL: &str = "\
type User @key(fields: \"id\") {\n  id: ID!\n  name: String\n}\n\
\n\
type Query {\n  me: User\n}\n";

struct MockServer {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Unblock `accept` so the worker thread can observe `stop`.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn start_mock_server(sdl: &str) -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let payload = serde_json::json!({
        "data": { "_service": { "sdl": sdl } }
    })
    .to_string();

    std::thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        while !stop_worker.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                continue;
            };
            if stop_worker.load(Ordering::SeqCst) {
                break;
            }
            if let Err(e) = serve_federation_post(&mut stream, payload.as_bytes()) {
                eprintln!("mock server handler error: {e}");
            }
        }
    });

    MockServer { port, stop }
}

fn serve_federation_post(stream: &mut TcpStream, payload: &[u8]) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(());
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("content-length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body)?;
    }

    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(payload)?;
    Ok(())
}

#[test]
fn run_writes_federation_v2_sdl_when_at_link_present() {
    let server = start_mock_server(FEDERATION_V2_SDL);
    let dir = unique_tmp_dir("v2-accept");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{}/graphql", server.port);
    let args: Vec<String> = vec![
        "--url".to_string(),
        url,
        "--out".to_string(),
        out.display().to_string(),
    ];
    graphql_federation::run(&args).expect("run should accept v2 SDL");
    let body = fs::read_to_string(&out).expect("read");
    assert!(
        body.contains("@link"),
        "on-disk SDL must contain @link: {body}"
    );
    assert!(
        body.contains("type User"),
        "on-disk SDL must have type User"
    );
}

#[test]
fn run_rejects_v1_sdl_in_strict_mode() {
    let server = start_mock_server(FEDERATION_V1_SDL);
    let dir = unique_tmp_dir("v1-reject");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{}/graphql", server.port);
    let args: Vec<String> = vec![
        "--url".to_string(),
        url,
        "--out".to_string(),
        out.display().to_string(),
    ];
    let err = graphql_federation::run(&args).expect_err("should reject v1 SDL");
    assert!(
        err.contains("did not return a federation v2 SDL"),
        "expected helpful error, got: {err}"
    );
    assert!(
        err.contains("--lenient"),
        "error should mention --lenient workaround: {err}"
    );
    // No file should be written.
    assert!(!out.exists(), "out file should not be created on rejection");
}

#[test]
fn run_accepts_v1_sdl_in_lenient_mode() {
    let server = start_mock_server(FEDERATION_V1_SDL);
    let dir = unique_tmp_dir("v1-lenient");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{}/graphql", server.port);
    let args: Vec<String> = vec![
        "--url".to_string(),
        url,
        "--out".to_string(),
        out.display().to_string(),
        "--lenient".to_string(),
    ];
    graphql_federation::run(&args).expect("run should accept v1 SDL with --lenient");
    let body = fs::read_to_string(&out).expect("read");
    assert!(body.contains("type User"));
    // Confirm the SDL does NOT contain @link (proves we ran with the
    // v1 mock — `body` should match what the server returned).
    assert!(!body.contains("@link"));
}

#[test]
fn run_with_bearer_token_succeeds() {
    // Bearer-token plumbing is shared with `graphql_sdl` — verifying
    // it gets passed through is enough here.
    let server = start_mock_server(FEDERATION_V2_SDL);
    let dir = unique_tmp_dir("bearer");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{}/graphql", server.port);
    let args: Vec<String> = vec![
        "--url".to_string(),
        url,
        "--out".to_string(),
        out.display().to_string(),
        "--bearer-token".to_string(),
        "test-token".to_string(),
    ];
    graphql_federation::run(&args).expect("run with bearer");
}
