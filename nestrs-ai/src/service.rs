use crate::options::{AiOptions, GenerateObjectRequest, GenerateTextRequest};
use crate::tools::AgentToolRegistry;
use async_trait::async_trait;
use nestrs_core::Injectable;
use nestrs_resilience::{
    PolicyBundle, ResilienceError, ResilienceOptions, ResilienceService, RetryPolicy,
    TimeoutPolicy,
};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum AiError {
    Configuration(String),
    Output(String),
    Transport(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration(m) | Self::Output(m) | Self::Transport(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AiError {}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub struct ModelCall {
    pub prompt: String,
    pub instructions: Option<String>,
    pub tools: Vec<String>,
    pub tool_results: Vec<(String, Value)>,
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
}

#[async_trait]
pub trait LanguageModel: Send + Sync {
    async fn call(&self, request: ModelCall) -> Result<ModelResponse, AiError>;
}

#[derive(Debug, Clone)]
pub struct GenerateResult {
    pub text: String,
    pub object: Option<Value>,
    pub steps: Vec<ModelResponse>,
    pub finish_reason: String,
}

pub struct AiService {
    options: AiOptions,
    tools: Option<Arc<AgentToolRegistry>>,
    resilience: ResilienceService,
}

impl AiService {
    pub fn new(options: AiOptions) -> Self {
        Self {
            options,
            tools: None,
            resilience: ResilienceService::from_options(ResilienceOptions::default()),
        }
    }

    pub fn with_tools(mut self, tools: Arc<AgentToolRegistry>) -> Self {
        self.tools = Some(tools);
        self
    }

    fn model(&self, name: Option<&str>) -> Result<Arc<dyn LanguageModel>, AiError> {
        let key = name.unwrap_or("default");
        self.options
            .models
            .get(key)
            .cloned()
            .ok_or_else(|| AiError::Configuration(format!("unknown model `{key}`")))
    }

    fn model_policy(&self) -> PolicyBundle {
        PolicyBundle {
            retry: Some(RetryPolicy {
                attempts: 2,
                delay: Duration::from_millis(25),
                factor: 2.0,
                max_delay: Duration::from_millis(200),
                jitter: true,
            }),
            timeout: Some(TimeoutPolicy {
                timeout: Duration::from_millis(self.options.defaults_timeout_ms.max(1)),
            }),
            ..Default::default()
        }
    }

    async fn call_model(
        &self,
        label: &str,
        model: Arc<dyn LanguageModel>,
        request: ModelCall,
    ) -> Result<ModelResponse, AiError> {
        let policy = self.model_policy();
        self.resilience
            .execute(label, policy, || {
                let model = Arc::clone(&model);
                let request = request.clone();
                async move {
                    model
                        .call(request)
                        .await
                        .map_err(|e| ResilienceError::Other(e.to_string()))
                }
            })
            .await
            .map_err(|e| match e {
                ResilienceError::Timeout => AiError::Transport("model call timed out".into()),
                other => AiError::Transport(other.to_string()),
            })
    }

    pub async fn generate_text(
        &self,
        req: GenerateTextRequest,
    ) -> Result<GenerateResult, AiError> {
        let model = self.model(req.model.as_deref())?;
        let mut steps = Vec::new();
        let mut tool_results = Vec::new();
        let max_steps = self.options.max_steps.max(1);

        for _ in 0..max_steps {
            let response = self
                .call_model(
                    &req.label,
                    Arc::clone(&model),
                    ModelCall {
                        prompt: req.prompt.clone(),
                        instructions: req.instructions.clone(),
                        tools: req.tools.clone(),
                        tool_results: tool_results.clone(),
                    },
                )
                .await?;
            steps.push(response.clone());
            if response.tool_calls.is_empty() {
                return Ok(GenerateResult {
                    text: response.text,
                    object: None,
                    steps,
                    finish_reason: "stop".into(),
                });
            }
            let Some(tools) = &self.tools else {
                return Err(AiError::Configuration(
                    "tools requested but no AgentToolRegistry configured".into(),
                ));
            };
            for call in response.tool_calls {
                let result = tools
                    .call(&call.name, call.arguments)
                    .await
                    .map_err(AiError::Output)?;
                tool_results.push((call.name, result));
            }
        }

        Ok(GenerateResult {
            text: steps.last().map(|s| s.text.clone()).unwrap_or_default(),
            object: None,
            steps,
            finish_reason: "tool-calls".into(),
        })
    }

    pub async fn generate_object(
        &self,
        req: GenerateObjectRequest,
    ) -> Result<GenerateResult, AiError> {
        let text = self
            .generate_text(GenerateTextRequest {
                model: req.model,
                label: req.label,
                prompt: format!(
                    "{}\nRespond with JSON matching schema {}.",
                    req.prompt, req.schema_name
                ),
                instructions: req.instructions,
                tools: vec![],
            })
            .await?;
        let object: Value = serde_json::from_str(&text.text)
            .map_err(|e| AiError::Output(format!("invalid JSON object: {e}")))?;
        validate_object_against_schema(&object, &req.schema)?;
        Ok(GenerateResult {
            text: text.text,
            object: Some(object),
            steps: text.steps,
            finish_reason: text.finish_reason,
        })
    }
}

/// Fail-closed structured-output check against a JSON Schema document.
///
/// Validates `type` / `required` / declared `properties` presence. Nested
/// keyword enforcement is intentionally shallow; reject anything that is not a
/// JSON object when the schema says `type: object`.
fn validate_object_against_schema(
    object: &Value,
    schema: &schemars::Schema,
) -> Result<(), AiError> {
    let schema_val = serde_json::to_value(schema)
        .map_err(|e| AiError::Output(format!("invalid schema: {e}")))?;

    if let Some(ty) = schema_val.get("type").and_then(|t| t.as_str()) {
        match ty {
            "object" if !object.is_object() => {
                return Err(AiError::Output(
                    "structured output must be a JSON object".into(),
                ));
            }
            "array" if !object.is_array() => {
                return Err(AiError::Output(
                    "structured output must be a JSON array".into(),
                ));
            }
            "string" if !object.is_string() => {
                return Err(AiError::Output(
                    "structured output must be a JSON string".into(),
                ));
            }
            "number" | "integer" if !object.is_number() => {
                return Err(AiError::Output(
                    "structured output must be a JSON number".into(),
                ));
            }
            "boolean" if !object.is_boolean() => {
                return Err(AiError::Output(
                    "structured output must be a JSON boolean".into(),
                ));
            }
            _ => {}
        }
    }

    if let Some(obj) = object.as_object() {
        if let Some(required) = schema_val.get("required").and_then(|r| r.as_array()) {
            for key in required {
                let Some(k) = key.as_str() else { continue };
                if !obj.contains_key(k) {
                    return Err(AiError::Output(format!(
                        "structured output missing required field `{k}`"
                    )));
                }
            }
        }
        if let Some(props) = schema_val.get("properties").and_then(|p| p.as_object()) {
            let additional = schema_val
                .get("additionalProperties")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !additional {
                for key in obj.keys() {
                    if !props.contains_key(key) {
                        return Err(AiError::Output(format!(
                            "structured output has undeclared field `{key}`"
                        )));
                    }
                }
            }
        }
    }

    Ok(())
}

impl Injectable for AiService {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("AiService must be provided by AiModule::register(...)");
    }
}
