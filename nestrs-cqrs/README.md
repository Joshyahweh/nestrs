# nestrs-cqrs

**CQRS-style command and query buses** for [nestrs](https://crates.io/crates/nestrs): type-safe `Command` / `Query` traits, async handlers, and a small **`CqrsModule`** that registers `CommandBus` + `QueryBus` in the DI container.

Handlers are registered **imperatively** (e.g. in `on_module_init` or bootstrap); auto-discovery can be layered in your app if needed.

**Docs:** [docs.rs/nestrs-cqrs](https://docs.rs/nestrs-cqrs) · **Repo:** [github.com/Joshyahweh/nestrs](https://github.com/Joshyahweh/nestrs)

## Install

```toml
[dependencies]
nestrs-cqrs = "0.3.4"
async-trait = "0.1"
```

## Example

```rust
use async_trait::async_trait;
use nestrs_cqrs::{Command, CommandBus, CommandHandler, CqrsError, Query, QueryBus, QueryHandler};
use std::sync::Arc;

pub struct CreateUser {
    pub email: String,
}

impl Command for CreateUser {
    type Response = u64;
}

pub struct GetUser {
    pub id: u64,
}

impl Query for GetUser {
    type Response = String;
}

pub struct Handlers;

#[async_trait]
impl CommandHandler<CreateUser> for Handlers {
    async fn execute(&self, cmd: CreateUser) -> Result<u64, CqrsError> {
        Ok(cmd.email.len() as u64)
    }
}

#[async_trait]
impl QueryHandler<GetUser> for Handlers {
    async fn execute(&self, q: GetUser) -> Result<String, CqrsError> {
        Ok(format!("user-{}", q.id))
    }
}

#[tokio::main]
async fn main() -> Result<(), CqrsError> {
    let commands = CommandBus::new();
    let queries = QueryBus::new();
    let handlers = Arc::new(Handlers);

    commands.register::<CreateUser, _>(handlers.clone()).await;
    queries.register::<GetUser, _>(handlers).await;

    let id = commands.execute(CreateUser { email: "a@b.com".into() }).await?;
    assert_eq!(id, 11);

    let name = queries.execute(GetUser { id }).await?;
    assert_eq!(name, "user-11");
    Ok(())
}
```

In a **nestrs** app, import **`CqrsModule`** in your `#[module]` graph so `CommandBus` and `QueryBus` are constructed via DI.

Also includes **`Saga`** / **`SagaDefinition`** traits as extension points for long-running workflows.

## License

MIT OR Apache-2.0.
