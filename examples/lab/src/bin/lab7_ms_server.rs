//! Lab 7a — microservice server over the Redis transport (live pub/sub RPC).
//!
//! Handles `user.get` (request/response), `user.fail` (error mapping), and
//! `user.created` (fire-and-forget event) over redis://127.0.0.1:6380 with the
//! `lab7` channel prefix.
//!
//! Run: `cargo run -p lab --bin lab7_ms_server`

use nestrs::prelude::*;

#[dto]
pub struct GetUserReq {
    #[validate(range(min = 0))]
    id: i64,
}

#[dto]
pub struct UserRes {
    #[IsString]
    name: String,
}

#[dto]
pub struct UserCreatedEvent {
    id: i64,
}

use std::sync::atomic::{AtomicU64, Ordering};

static EVENT_HITS: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
#[injectable]
struct UserHandler;

#[micro_routes]
impl UserHandler {
    #[message_pattern("user.get")]
    async fn get_user(&self, req: GetUserReq) -> Result<UserRes, HttpException> {
        if req.id == 0 {
            return Err(BadRequestException::new("id must be non-zero"));
        }
        Ok(UserRes {
            name: format!("user-{}", req.id),
        })
    }

    #[message_pattern("user.fail")]
    async fn fail_always(&self, _req: GetUserReq) -> Result<UserRes, HttpException> {
        Err(InternalServerErrorException::new("deliberate failure"))
    }

    #[event_pattern("user.created")]
    async fn on_user_created(&self, evt: UserCreatedEvent) {
        let n = EVENT_HITS.fetch_add(1, Ordering::Relaxed) + 1;
        eprintln!("[lab7-server] user.created #{n} for id={}", evt.id);
    }
}

#[module(providers = [UserHandler], microservices = [UserHandler])]
pub struct MsModule;

#[tokio::main]
async fn main() {
    NestFactory::create_microservice_redis::<MsModule>(
        nestrs::microservices::RedisMicroserviceOptions::new("redis://127.0.0.1:6380")
            .with_prefix("lab7"),
    )
    .listen()
    .await;
}
