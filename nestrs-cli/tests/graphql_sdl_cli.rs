//! Wave 7.11 — `nestrs-cli graphql sdl` integration tests. Tests the
//! pure parsing path (`parse_sdl_body`) and the file-write path
//! (`write_sdl`) directly — the `curl`-based `fetch_sdl` path is
//! covered by a smoke test that uses a local Python HTTP server.

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use nestrs_cli::graphql_sdl;

fn unique_tmp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("nestrs-cli-gql-{name}-{nanos}"))
}

#[test]
fn parse_sdl_body_extracts_federation_sdl_field() {
    let body = r#"{"data":{"_service":{"sdl":"type User @key(fields: \"id\") { id: ID! }"}}}"#;
    let sdl = graphql_sdl::parse_sdl_body(body).expect("parse");
    assert!(sdl.contains("type User"));
    assert!(sdl.contains("@key"));
}

#[test]
fn parse_sdl_body_rejects_missing_service_field() {
    let body = r#"{"data":null,"errors":[{"message":"not enabled"}]}"#;
    let err = graphql_sdl::parse_sdl_body(body).expect_err("should fail");
    assert!(err.contains("not contain"));
}

#[test]
fn write_sdl_creates_parent_dirs_and_writes_bytes() {
    let dir = unique_tmp_dir("write-sdl");
    let nested = dir.join("schemas").join("v1").join("user.graphql");
    let sdl = "type User { id: ID! }";
    graphql_sdl::write_sdl(&nested, sdl).expect("write");
    let body = fs::read_to_string(&nested).expect("read");
    assert_eq!(body, sdl);
}

#[test]
fn write_sdl_overwrites_existing_file() {
    let dir = unique_tmp_dir("write-sdl-overwrite");
    let path = dir.join("schema.graphql");
    fs::write(&path, "old content").expect("seed");
    graphql_sdl::write_sdl(&path, "new content").expect("write");
    let body = fs::read_to_string(&path).expect("read");
    assert_eq!(body, "new content");
}

#[test]
fn run_rejects_missing_url() {
    let args: Vec<String> = vec!["--out".to_string(), "/tmp/x".to_string()];
    let err = graphql_sdl::run(&args).expect_err("should reject");
    assert!(err.contains("--url"), "error should mention --url: {err}");
}

#[test]
fn run_rejects_missing_out() {
    let args: Vec<String> = vec!["--url".to_string(), "http://localhost:3000/graphql".to_string()];
    let err = graphql_sdl::run(&args).expect_err("should reject");
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
    let err = graphql_sdl::run(&args).expect_err("should reject");
    assert!(err.contains("unknown option"), "got: {err}");
}

// ---------------------------------------------------------------------------
// End-to-end smoke test — spin up a Python HTTP server that responds
// to the federation SDL query, then run `fetch_sdl` against it. We
// use Python because every Mac has it; no extra dep.
// ---------------------------------------------------------------------------

fn start_mock_server() -> Option<(u16, std::process::Child)> {
    if !Command::new("python3")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return None;
    }

    // Pick a free port.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    let script = r#"
import http.server, json, sys

PORT = int(sys.argv[1])

class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        ln = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(ln).decode()
        try:
            req = json.loads(body)
        except Exception:
            req = {}
        q = req.get("query", "")
        if "_service" in q and "sdl" in q:
            payload = {"data": {"_service": {"sdl": "type User @key(fields: \"id\") { id: ID! }"}}}
        else:
            payload = {"errors": [{"message": "unsupported"}]}
        out = json.dumps(payload).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(out)))
        self.end_headers()
        self.wfile.write(out)
    def log_message(self, *args, **kwargs):
        pass

http.server.HTTPServer(("127.0.0.1", PORT), H).serve_forever()
"#;

    let child = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(port.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn python3");

    // Wait for the server to start.
    for _ in 0..30 {
        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            // bind failed = port in use = server is up
            std::thread::sleep(Duration::from_millis(50));
            return Some((port, child));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Some((port, child))
}

#[test]
fn fetch_sdl_against_mock_server() {
    let Some((port, mut child)) = start_mock_server() else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let url = format!("http://127.0.0.1:{port}/graphql");
    let sdl = graphql_sdl::fetch_sdl(&url, None, true).expect("fetch");
    assert!(sdl.contains("type User"));
    assert!(sdl.contains("@key"));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn run_full_subcommand_writes_file() {
    let Some((port, mut child)) = start_mock_server() else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let dir = unique_tmp_dir("run-full");
    let out = dir.join("schema.graphql");
    let url = format!("http://127.0.0.1:{port}/graphql");
    let args: Vec<String> = vec![
        "--url".to_string(),
        url,
        "--out".to_string(),
        out.display().to_string(),
        "--federation".to_string(),
    ];
    graphql_sdl::run(&args).expect("run");
    let body = fs::read_to_string(&out).expect("read");
    assert!(body.contains("type User"));
    assert!(body.contains("@key"));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn run_full_subcommand_with_bearer_token() {
    let Some((port, mut child)) = start_mock_server() else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let dir = unique_tmp_dir("run-bearer");
    let out = dir.join("schema.graphql");
    let url = format!("http://127.0.0.1:{port}/graphql");
    let args: Vec<String> = vec![
        "--url".to_string(),
        url,
        "--out".to_string(),
        out.display().to_string(),
        "--federation".to_string(),
        "--bearer-token".to_string(),
        "test-token".to_string(),
    ];
    graphql_sdl::run(&args).expect("run");
    let _ = child.kill();
    let _ = child.wait();
}