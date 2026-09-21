//! Wave 7.11 — `nestrs-cli graphql sdl` integration tests. Tests the
//! pure parsing path (`parse_sdl_body`) and the file-write path
//! (`write_sdl`) directly — the `curl`-based `fetch_sdl` path is
//! covered by a smoke test against an in-process mock HTTP server.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nestrs_scaffold::graphql_sdl;

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
    fs::create_dir_all(&dir).expect("mkdir");
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
    let args: Vec<String> = vec![
        "--url".to_string(),
        "http://localhost:3000/graphql".to_string(),
    ];
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
// End-to-end — in-process mock (listener stays bound; no Python / port race).
// ---------------------------------------------------------------------------

struct MockServer {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn start_mock_server() -> MockServer {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_worker = Arc::clone(&stop);
    let payload = serde_json::json!({
        "data": {
            "_service": {
                "sdl": "type User @key(fields: \"id\") { id: ID! }"
            }
        }
    })
    .to_string();

    std::thread::spawn(move || {
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
fn fetch_sdl_against_mock_server() {
    let server = start_mock_server();
    let url = format!("http://127.0.0.1:{}/graphql", server.port);
    let sdl = graphql_sdl::fetch_sdl(&url, None, true).expect("fetch");
    assert!(sdl.contains("type User"));
    assert!(sdl.contains("@key"));
}

#[test]
fn run_full_subcommand_writes_file() {
    let server = start_mock_server();
    let dir = unique_tmp_dir("run-full");
    let out = dir.join("schema.graphql");
    let url = format!("http://127.0.0.1:{}/graphql", server.port);
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
}

#[test]
fn run_full_subcommand_with_bearer_token() {
    let server = start_mock_server();
    let dir = unique_tmp_dir("run-bearer");
    let out = dir.join("schema.graphql");
    let url = format!("http://127.0.0.1:{}/graphql", server.port);
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
}
