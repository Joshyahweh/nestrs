# nestrs-workflows

Durable multi-step workflows for [nestrs](https://crates.io/crates/nestrs) — NestJS `@nestjs/workflows` analogue.

Persist step results, resume after process restart, and park for **human approval**. Combine with [`nestrs-locks`](https://crates.io/crates/nestrs-locks) for single-runner steppers and [`nestrs-outbox`](https://crates.io/crates/nestrs-outbox) for side effects.

## Install

```toml
[dependencies]
nestrs-workflows = "1.7.1"
```

Or via the umbrella (also enables `outbox` + `locks`):

```toml
nestrs = { version = "1.7.1", features = ["workflows"] }
```

## Register

```rust
use nestrs_workflows::{WorkflowsModule, WorkflowsOptions};

#[module(imports = [WorkflowsModule::register(WorkflowsOptions::default())])]
struct AppModule;
```

## Lifecycle

```text
start → complete_step → … → wait_for_approval → approve → complete_step → …
```

```rust
use nestrs_workflows::WorkflowsService;
use serde_json::json;
use std::sync::Arc;

async fn order_flow(svc: Arc<WorkflowsService>) {
    let inst = svc.start("order.fulfill", json!({ "id": 1 })).await;
    svc.complete_step(&inst.id, "charge", json!({ "ok": true }))
        .await
        .unwrap();

    svc.wait_for_approval(&inst.id).await.unwrap();
    // later, from an admin handler:
    svc.approve(&inst.id).await.unwrap();

    svc.complete_step(&inst.id, "ship", json!({ "carrier": "DHL" }))
        .await
        .unwrap();
}
```

A new `WorkflowsService` sharing the same `WorkflowStore` resumes from persisted state after a logical restart.

### Status

| `WorkflowStatus` | Meaning |
|------------------|---------|
| `Running` | Active steps |
| `WaitingApproval` | Parked until `approve` |
| `Completed` | Finished |
| `Failed` | Terminal failure |

## Surface

- `WorkflowsModule` / `WorkflowsOptions` / `WorkflowsService`
- `WorkflowInstance`, `WorkflowStatus`
- `InMemoryWorkflowStore` / `WorkflowStore` trait

## Docs

- Guide: [Workflows](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/workflows.mdx)
- Hub: [Reliability](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/guides/reliability.mdx)
- rustdoc: <https://docs.rs/nestrs-workflows/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
