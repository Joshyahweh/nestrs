//! Wave 7.12 — `nestrs-cli graphql federation export` integration
//! tests. The `is_federation_v2_sdl` substring check is already covered
//! by unit tests inside `graphql_federation.rs`. The CLI tests cover
//! flag parsing, the v2 validation gate, the `--lenient` opt-out, and
//! end-to-end against a Python mock server that returns a federation
//! v2 SDL with the `@link` directive.

use std::fs;
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
// End-to-end — Python mock server that returns a federation v2 SDL
// (with `@link` directive) on `{_service{sdl}}`. The SDL payload is
// passed via argv[2] so each test can swap between v1 / v2 shapes.
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

fn start_mock_server(sdl: &str) -> Option<(u16, std::process::Child)> {
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

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);

    // Pass the SDL via a temp file because SDLs may contain quotes /
    // braces that would break shell-string interpolation in argv.
    let payload_path = std::env::temp_dir().join(format!(
        "nestrs-cli-fed-mock-{}-{}.graphql",
        port,
        std::process::id()
    ));
    let mut f = fs::File::create(&payload_path).expect("create payload");
    f.write_all(sdl.as_bytes()).expect("write payload");
    drop(f);

    let script = r#"
import http.server, json, sys

PORT = int(sys.argv[1])
SDL = open(sys.argv[2]).read()

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
            payload = {"data": {"_service": {"sdl": SDL}}}
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
        .arg(payload_path.display().to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn python3");

    for _ in 0..30 {
        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            std::thread::sleep(Duration::from_millis(50));
            return Some((port, child));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = fs::remove_file(&payload_path);
    Some((port, child))
}

#[test]
fn run_writes_federation_v2_sdl_when_at_link_present() {
    let Some((port, mut child)) = start_mock_server(FEDERATION_V2_SDL) else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let dir = unique_tmp_dir("v2-accept");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{port}/graphql");
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
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn run_rejects_v1_sdl_in_strict_mode() {
    let Some((port, mut child)) = start_mock_server(FEDERATION_V1_SDL) else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let dir = unique_tmp_dir("v1-reject");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{port}/graphql");
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
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn run_accepts_v1_sdl_in_lenient_mode() {
    let Some((port, mut child)) = start_mock_server(FEDERATION_V1_SDL) else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let dir = unique_tmp_dir("v1-lenient");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{port}/graphql");
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
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn run_with_bearer_token_succeeds() {
    // Bearer-token plumbing is shared with `graphql_sdl` — verifying
    // it gets passed through is enough here.
    let Some((port, mut child)) = start_mock_server(FEDERATION_V2_SDL) else {
        eprintln!("skipping: python3 not available");
        return;
    };
    let dir = unique_tmp_dir("bearer");
    let out = dir.join("user.graphql");
    let url = format!("http://127.0.0.1:{port}/graphql");
    let args: Vec<String> = vec![
        "--url".to_string(),
        url,
        "--out".to_string(),
        out.display().to_string(),
        "--bearer-token".to_string(),
        "test-token".to_string(),
    ];
    graphql_federation::run(&args).expect("run with bearer");
    let _ = child.kill();
    let _ = child.wait();
}
