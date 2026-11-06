use crate::service::{AiError, LanguageModel, ModelCall, ModelResponse};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Mutex;

/// NestJS `FakeModel` analogue — scripted answers for tests.
pub struct FakeModel {
    answers: Mutex<Vec<Result<Value, AiError>>>,
    pub calls: Mutex<Vec<ModelCall>>,
}

impl FakeModel {
    pub fn new(answers: Vec<Result<Value, AiError>>) -> Self {
        Self {
            answers: Mutex::new(answers),
            calls: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl LanguageModel for FakeModel {
    async fn call(&self, request: ModelCall) -> Result<ModelResponse, AiError> {
        self.calls.lock().expect("calls").push(request);
        let next = {
            let mut answers = self.answers.lock().expect("answers");
            if answers.is_empty() {
                Err(AiError::Output("no more fake answers".into()))
            } else {
                answers.remove(0)
            }
        };
        let value = next?;
        let text = value
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let tool_calls = value
            .get("tool_calls")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| {
                Some(crate::service::ToolCall {
                    name: c.get("name")?.as_str()?.to_string(),
                    arguments: c.get("arguments").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        Ok(ModelResponse { text, tool_calls })
    }
}
