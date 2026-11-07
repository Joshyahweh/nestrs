# nestrs-ai

LLM / agent module for [nestrs](https://crates.io/crates/nestrs) — NestJS [`@nestjs/ai`](https://docs.nestjs.com/ai/overview) analogue (AI SDK–style, OpenAI-compatible patterns).

Register named `LanguageModel` implementations, call `generate_text` / `generate_object`, and optionally run an agent loop with tools. Model calls go through [`nestrs-resilience`](https://crates.io/crates/nestrs-resilience) (timeout + jittered retry).

## Install

```toml
[dependencies]
nestrs-ai = "1.7.1"
```

Or via the umbrella (also enables `resilience`):

```toml
nestrs = { version = "1.7.1", features = ["ai"] }
```

## Register models

```rust
use nestrs_ai::{AiModule, AiOptions, FakeModel, LanguageModel};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

fn ai_module() -> nestrs_core::DynamicModule {
    let fake = FakeModel::new(vec![Ok(json!({ "text": "hello" }))]);
    let mut models: HashMap<String, Arc<dyn LanguageModel>> = HashMap::new();
    models.insert("default".into(), Arc::new(fake));

    AiModule::register(AiOptions {
        models,
        defaults_timeout_ms: 60_000,
        max_steps: 10,
    })
}
```

`FakeModel` is for **tests and labs only** — script answers, never production traffic.

## Generate

```rust
use nestrs_ai::{AiService, GenerateObjectRequest, GenerateTextRequest};
use std::sync::Arc;

async fn demo(ai: Arc<AiService>) {
    let text = ai
        .generate_text(GenerateTextRequest {
            model: Some("default".into()),
            label: "greeting".into(),
            prompt: "Say hi".into(),
            instructions: None,
            tools: vec![],
        })
        .await
        .unwrap();

    // generate_object fails closed on invalid JSON or missing required fields
    let _obj = ai
        .generate_object(GenerateObjectRequest {
            model: Some("default".into()),
            label: "extract".into(),
            prompt: "…".into(),
            instructions: None,
            schema_name: "Person".into(),
            schema: schemars::schema_for!(Person),
        })
        .await;
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Person {
    name: String,
}
```

### Agent tools

Register handlers on `AgentToolRegistry` / `AgentFunction`. **Tool handlers must enforce caller authz** — the framework does not imply trust from the model.

## Surface

- `AiModule` / `AiOptions` / `AiService`
- `LanguageModel`, `FakeModel`
- `GenerateTextRequest`, `GenerateObjectRequest`, `GenerateResult`
- `AgentToolRegistry`, `AgentFunction`, `ToolCall`
- `AiError`

## Safety

| Rule | Why |
|------|-----|
| Fail closed on `generate_object` | Invalid JSON / missing fields → `AiError`, not silent defaults |
| Authz in tools | Models can invent tool calls |
| Timeouts via resilience | Avoid hung providers |
| No secrets in prompts/logs | Treat model I/O as untrusted |

## Docs

- Guide: [AI](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/ecosystem/ai.mdx)
- Hub: [Reliability](https://github.com/Joshyahweh/nestrs/blob/main/mintlify-docs/guides/reliability.mdx)
- rustdoc: <https://docs.rs/nestrs-ai/1.7.1>
- Lab: `cargo run -p lab --bin lab9_nestjs12`

## License

MIT OR Apache-2.0.
