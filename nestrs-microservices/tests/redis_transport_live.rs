//! Live Redis transport tests — the connection-churn regression suite.
//!
//! Opt-in (no broker in CI): set `NESTRS_TEST_REDIS_URL` (e.g.
//! `redis://127.0.0.1:6379`) and run with `--features redis`.
//!
//! The churn finding: `RedisTransport::send_json` used to dial a fresh pubsub
//! connection *and* a fresh command connection per RPC, the server dialed a
//! fresh command connection per reply, and `emit_json` dialed one per emit.
//! These tests pin the fixed topology — a fixed set of long-lived connections
//! — by asserting `INFO clients`'s `connected_clients` is stable across
//! sequential *and* concurrent RPCs, emits, and error replies.

#![cfg(feature = "redis")]

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use nestrs_microservices::{
    MicroserviceHandler, RedisMicroserviceOptions, RedisMicroserviceServer, RedisTransport,
    RedisTransportOptions, Transport, TransportError,
};
use serde_json::{json, Value};

fn live_url() -> Option<String> {
    std::env::var("NESTRS_TEST_REDIS_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// Answers `audit.ping`, records `audit.tick` events, and deliberately has no
/// handler for `audit.missing` so error replies can be exercised too.
struct RecordingHandler {
    emits: Arc<StdMutex<Vec<(String, Value)>>>,
}

#[async_trait]
impl MicroserviceHandler for RecordingHandler {
    async fn handle_message(
        &self,
        pattern: &str,
        payload: Value,
    ) -> Option<Result<Value, TransportError>> {
        match pattern {
            "audit.ping" => Some(Ok(json!({"pong": payload.get("n")}))),
            // Exercise the server's error-reply path through the shared
            // manager connection too.
            "audit.fail" => Some(Err(TransportError::new("deliberate failure"))),
            _ => None,
        }
    }

    async fn handle_event(&self, pattern: &str, payload: Value) -> bool {
        if pattern == "audit.tick" {
            self.emits.lock().expect("emits lock").push((pattern.to_string(), payload));
            true
        } else {
            false
        }
    }
}

/// Spawns the server and returns (transport, join handle, kill sender). The
/// transport is separate from the server so both sides' connections are under
/// test.
async fn start_fixture(
    url: &str,
    emits: Arc<StdMutex<Vec<(String, Value)>>>,
) -> (
    RedisTransport,
    tokio::task::JoinHandle<Result<(), TransportError>>,
    tokio::sync::oneshot::Sender<()>,
) {
    let handler = Arc::new(RecordingHandler { emits });
    let server = RedisMicroserviceServer::new(
        RedisMicroserviceOptions::new(url).with_prefix("nestrs.audit.live"),
        vec![handler],
    );
    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        server
            .listen_with_shutdown(async move {
                let _ = kill_rx.await;
            })
            .await
    });

    let mut opts = RedisTransportOptions::new(url).with_prefix("nestrs.audit.live");
    opts.request_timeout = Duration::from_secs(5);
    let transport = RedisTransport::new(opts);
    (transport, task, kill_tx)
}

/// Retries `audit.ping` until the server's wildcard subscription is live
/// (listening is async; there is no ready signal on the transport).
async fn await_ready(transport: &RedisTransport) {
    for _ in 0..50 {
        if transport
            .send_json("audit.ping", json!({"n": 0}))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("redis server fixture never became ready");
}

/// `INFO clients` → `connected_clients`. Uses its own long-lived manager so
/// measuring itself never changes the count.
async fn connected_clients(url: &str) -> u64 {
    let client = redis::Client::open(url).expect("info client");
    let mut conn = client
        .get_connection_manager()
        .await
        .expect("info manager");
    let info: String = redis::cmd("INFO")
        .arg("clients")
        .query_async(&mut conn)
        .await
        .expect("INFO clients");
    let line = info
        .lines()
        .find(|l| l.starts_with("connected_clients:"))
        .expect("connected_clients in INFO");
    line.trim_start_matches("connected_clients:")
        .trim()
        .parse()
        .expect("connected_clients integer")
}

#[tokio::test]
async fn rpcs_and_emits_run_on_a_fixed_set_of_connections() {
    let Some(url) = live_url() else {
        // Opt-in: set NESTRS_TEST_REDIS_URL to run this suite.
        return;
    };

    let emits = Arc::new(StdMutex::new(Vec::new()));
    let (transport, server_task, kill_tx) = start_fixture(&url, emits.clone()).await;
    await_ready(&transport).await;

    // Warm up: transport manager, transport pubsub pump, server manager and
    // server pubsub are all up by the time the first pong returns.
    for n in 1..=2 {
        let res = transport.send_json("audit.ping", json!({"n": n})).await;
        assert_eq!(res.expect("warmup rpc")["pong"], n, "warmup RPC {n}");
    }

    let baseline = connected_clients(&url).await;

    // Sequential RPCs (including an unhandled-pattern error and a deliberate
    // handler failure — error replies must use the shared manager too).
    for n in 1..=3 {
        let res = transport
            .send_json("audit.ping", json!({"n": n}))
            .await
            .expect("sequential rpc");
        assert_eq!(res["pong"], n);
    }
    let missing = transport
        .send_json("audit.missing", json!({}))
        .await
        .expect_err("unhandled pattern must error");
    assert!(missing.message.contains("no microservice handler"), "{missing:?}");
    let failing = transport
        .send_json("audit.fail", json!({}))
        .await
        .expect_err("deliberate failure must error");
    assert_eq!(failing.message, "deliberate failure");

    // Concurrent RPCs — the reply-router registry must deliver each reply to
    // its own waiter across one shared pubsub connection.
    let rounds: Vec<_> = (1..=8)
        .map(|n| {
            let t = transport.clone();
            async move {
                let res = t.send_json("audit.ping", json!({"n": n})).await;
                (n, res)
            }
        })
        .collect();
    let joined = futures_util::future::join_all(rounds).await;
    for (n, res) in joined {
        assert_eq!(res.expect("concurrent rpc")["pong"], n, "concurrent RPC {n}");
    }

    // Emits ride the shared manager connection too.
    transport
        .emit_json("audit.tick", json!({"seq": 1}))
        .await
        .expect("emit");
    transport
        .emit_json("audit.tick", json!({"seq": 2}))
        .await
        .expect("emit");
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let count = emits.lock().expect("emits lock").len();
        if count >= 2 || std::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let (first_seq, second_seq, emit_count) = {
        let recorded = emits.lock().expect("emits lock");
        (recorded[0].1["seq"].clone(), recorded[1].1["seq"].clone(), recorded.len())
    };
    assert_eq!(emit_count, 2, "both emits delivered");
    assert_eq!(first_seq, 1);
    assert_eq!(second_seq, 2);

    // THE churn assertion: everything above ran on the connections that
    // already existed at `baseline`.
    let after = connected_clients(&url).await;
    assert_eq!(
        after, baseline,
        "connection churn: RPCs/emits dialed new connections (baseline {baseline}, after {after})"
    );

    // Clean shutdown; the listener exits its select loop.
    let _ = kill_tx.send(());
    server_task
        .await
        .expect("server task join")
        .expect("listener exits cleanly");
}
