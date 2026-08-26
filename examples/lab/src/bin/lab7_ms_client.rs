//! Lab 7b — microservice client: fires live Redis-transport RPCs at lab7_ms_server
//! and prints each result (happy path, error detail mapping, guard-free event emit).
//!
//! Run after the server is up: `cargo run -p lab --bin lab7_ms_client`

use std::sync::Arc;

#[derive(serde::Serialize)]
pub struct Req {
    id: i64,
}

#[derive(serde::Deserialize)]
pub struct Res {
    name: String,
}

#[tokio::main]
async fn main() {
    let transport = nestrs::microservices::RedisTransport::new(
        nestrs::microservices::RedisTransportOptions::new("redis://127.0.0.1:6380")
            .with_prefix("lab7"),
    );
    let proxy = nestrs::microservices::ClientProxy::new(Arc::new(transport));

    // 1. Happy-path request/response.
    match proxy.send::<Req, Res>("user.get", &Req { id: 7 }).await {
        Ok(res) => println!("RPC OK   user.get(7)      -> {}", res.name),
        Err(e) => println!("RPC FAIL user.get(7): {e:?}"),
    }

    // 2. Several sequential calls — proves reply channels are per-call, not reused.
    for id in [1, 2, 3] {
        match proxy.send::<Req, Res>("user.get", &Req { id }).await {
            Ok(res) => println!("RPC OK   user.get({id})     -> {}", res.name),
            Err(e) => println!("RPC FAIL user.get({id}): {e:?}"),
        }
    }

    // 3. HttpException on the server maps into TransportError details.
    match proxy.send::<Req, Res>("user.get", &Req { id: 0 }).await {
        Ok(_) => println!("RPC UNEXPECTED user.get(0) succeeded"),
        Err(e) => println!(
            "RPC ERR  user.get(0)       -> message={:?} details={:?}",
            e.message,
            e.details.as_ref().map(|d| &d["statusCode"])
        ),
    }

    match proxy.send::<Req, Res>("user.fail", &Req { id: 9 }).await {
        Ok(_) => println!("RPC UNEXPECTED user.fail succeeded"),
        Err(e) => println!(
            "RPC ERR  user.fail         -> message={:?} details={:?}",
            e.message,
            e.details.as_ref().map(|d| &d["statusCode"])
        ),
    }

    // 4. Fire-and-forget event.
    proxy
        .emit("user.created", &serde_json::json!({ "id": 99 }))
        .await
        .expect("emit ok");
    println!("EMIT     user.created(99)  -> sent");

    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    println!("LAB7 CLIENT DONE");
}
