use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub type AgentHandler =
    Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Result<Value, String>> + Send>> + Send + Sync>;

pub struct AgentFunction {
    pub name: String,
    pub description: String,
    pub handler: AgentHandler,
}

pub struct AgentToolRegistry {
    tools: Mutex<HashMap<String, AgentFunction>>,
}

impl AgentToolRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tools: Mutex::new(HashMap::new()),
        })
    }

    pub fn register(&self, func: AgentFunction) {
        self.tools
            .lock()
            .expect("tools")
            .insert(func.name.clone(), func);
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        let handler = {
            let guard = self.tools.lock().expect("tools");
            let f = guard
                .get(name)
                .ok_or_else(|| format!("unknown tool {name}"))?;
            f.handler.clone()
        };
        handler(args).await
    }
}

impl Default for AgentToolRegistry {
    fn default() -> Self {
        Self {
            tools: Mutex::new(HashMap::new()),
        }
    }
}
