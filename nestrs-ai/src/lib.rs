//! NestJS [`@nestjs/ai`](https://docs.nestjs.com/ai/overview) analogue —
//! named [`LanguageModel`]s, [`AiService::generate_text`] / [`AiService::generate_object`],
//! agent tools via [`AgentToolRegistry`], and [`FakeModel`] for tests.
//!
//! Model calls run under [`nestrs_resilience`] (timeout + jittered retry).
//! [`AiService::generate_object`] **fails closed** on invalid JSON or missing required
//! schema fields. Tool handlers must enforce the **caller's** authorization.
//!
//! Umbrella feature: `nestrs` / `ai` (implies `resilience`) → `nestrs::ai`.
#![doc(html_root_url = "https://docs.rs/nestrs-ai/1.7.0")]

mod fake;
mod module;
mod options;
mod service;
mod tools;

pub use fake::FakeModel;
pub use module::AiModule;
pub use options::{AiModelConfig, AiOptions, GenerateObjectRequest, GenerateTextRequest};
pub use service::{AiError, AiService, GenerateResult, LanguageModel};
pub use tools::{AgentFunction, AgentToolRegistry};

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn fake_model_generate_text() {
        let fake = FakeModel::new(vec![Ok(json!({"text": "hello"}))]);
        let mut models = std::collections::HashMap::new();
        models.insert(
            "default".to_string(),
            Arc::new(fake) as Arc<dyn LanguageModel>,
        );
        let svc = AiService::new(AiOptions {
            models,
            defaults_timeout_ms: 5_000,
            max_steps: 5,
        });
        let out = svc
            .generate_text(GenerateTextRequest {
                model: None,
                label: "t".into(),
                prompt: "hi".into(),
                instructions: None,
                tools: vec![],
            })
            .await
            .expect("text");
        assert_eq!(out.text, "hello");
    }

    #[tokio::test]
    async fn agent_loop_calls_tool() {
        let fake = FakeModel::new(vec![
            Ok(json!({
                "tool_calls": [{"name": "echo", "arguments": {"x": 1}}],
                "text": ""
            })),
            Ok(json!({"text": "done"})),
        ]);
        let mut models = std::collections::HashMap::new();
        models.insert(
            "default".to_string(),
            Arc::new(fake) as Arc<dyn LanguageModel>,
        );
        let tools = AgentToolRegistry::new();
        tools.register(AgentFunction {
            name: "echo".into(),
            description: "echo".into(),
            handler: Arc::new(|args| Box::pin(async move { Ok(args) })),
        });
        let svc = AiService::new(AiOptions {
            models,
            defaults_timeout_ms: 5_000,
            max_steps: 5,
        })
        .with_tools(tools);
        let out = svc
            .generate_text(GenerateTextRequest {
                model: None,
                label: "agent".into(),
                prompt: "run".into(),
                instructions: None,
                tools: vec!["echo".into()],
            })
            .await
            .expect("agent");
        assert_eq!(out.text, "done");
        assert_eq!(out.steps.len(), 2);
    }

    #[tokio::test]
    async fn generate_object_fails_closed_on_invalid_json() {
        let fake = FakeModel::new(vec![Ok(json!({"text": "not-json"}))]);
        let mut models = std::collections::HashMap::new();
        models.insert(
            "default".to_string(),
            Arc::new(fake) as Arc<dyn LanguageModel>,
        );
        let svc = AiService::new(AiOptions {
            models,
            defaults_timeout_ms: 5_000,
            max_steps: 1,
        });
        let schema = schemars::json_schema!({
            "type": "object"
        });
        let err = svc
            .generate_object(GenerateObjectRequest {
                model: None,
                label: "obj".into(),
                prompt: "give object".into(),
                instructions: None,
                schema_name: "Obj".into(),
                schema,
            })
            .await
            .expect_err("must fail closed");
        assert!(matches!(err, AiError::Output(_)));
    }

    #[tokio::test]
    async fn generate_object_fails_closed_on_missing_required() {
        #[derive(serde::Serialize, schemars::JsonSchema)]
        struct NeedName {
            name: String,
        }

        let fake = FakeModel::new(vec![Ok(json!({"text": "{}"}))]);
        let mut models = std::collections::HashMap::new();
        models.insert(
            "default".to_string(),
            Arc::new(fake) as Arc<dyn LanguageModel>,
        );
        let svc = AiService::new(AiOptions {
            models,
            defaults_timeout_ms: 5_000,
            max_steps: 1,
        });
        let schema = schemars::schema_for!(NeedName);
        let err = svc
            .generate_object(GenerateObjectRequest {
                model: None,
                label: "obj".into(),
                prompt: "give object".into(),
                instructions: None,
                schema_name: "NeedName".into(),
                schema,
            })
            .await
            .expect_err("missing required must fail");
        assert!(
            matches!(&err, AiError::Output(m) if m.contains("name")),
            "unexpected: {err}"
        );
    }
}
