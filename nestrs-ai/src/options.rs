use crate::service::LanguageModel;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub struct AiModelConfig {
    pub model: Arc<dyn LanguageModel>,
    pub fallback: Option<String>,
}

#[derive(Clone)]
pub struct AiOptions {
    pub models: HashMap<String, Arc<dyn LanguageModel>>,
    pub defaults_timeout_ms: u64,
    pub max_steps: u32,
}

impl Default for AiOptions {
    fn default() -> Self {
        Self {
            models: HashMap::new(),
            defaults_timeout_ms: 60_000,
            max_steps: 10,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GenerateTextRequest {
    pub model: Option<String>,
    pub label: String,
    pub prompt: String,
    pub instructions: Option<String>,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GenerateObjectRequest {
    pub model: Option<String>,
    pub label: String,
    pub prompt: String,
    pub instructions: Option<String>,
    pub schema_name: String,
    pub schema: schemars::Schema,
}
