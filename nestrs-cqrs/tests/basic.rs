use async_trait::async_trait;
use nestrs_cqrs::{Command, CommandBus, CommandHandler, Query, QueryBus, QueryHandler};
use std::sync::Arc;

struct Ping;

impl Command for Ping {
    type Response = String;
}

struct PingHandler;

#[async_trait]
impl CommandHandler<Ping> for PingHandler {
    async fn execute(
        &self,
        _command: Ping,
    ) -> Result<<Ping as Command>::Response, nestrs_cqrs::CqrsError> {
        Ok("pong".to_string())
    }
}

struct GetAnswer;

impl Query for GetAnswer {
    type Response = u32;
}

struct GetAnswerHandler;

#[async_trait]
impl QueryHandler<GetAnswer> for GetAnswerHandler {
    async fn execute(
        &self,
        _query: GetAnswer,
    ) -> Result<<GetAnswer as Query>::Response, nestrs_cqrs::CqrsError> {
        Ok(42)
    }
}

#[tokio::test]
async fn command_bus_register_and_execute() {
    let bus = CommandBus::new();
    bus.register::<Ping, _>(Arc::new(PingHandler)).await;
    let res = bus.execute(Ping).await.expect("execute should succeed");
    assert_eq!(res, "pong");
}

#[tokio::test]
async fn query_bus_register_and_execute() {
    let bus = QueryBus::new();
    bus.register::<GetAnswer, _>(Arc::new(GetAnswerHandler))
        .await;
    let res = bus
        .execute(GetAnswer)
        .await
        .expect("execute should succeed");
    assert_eq!(res, 42);
}
