use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    Running,
    WaitingApproval,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInstance {
    pub id: String,
    pub name: String,
    pub input: Value,
    pub status: WorkflowStatus,
    pub steps: HashMap<String, Value>,
}

#[async_trait]
pub trait WorkflowStore: Send + Sync {
    async fn create(&self, name: &str, input: Value) -> WorkflowInstance;
    async fn get(&self, id: &str) -> Option<WorkflowInstance>;
    async fn complete_step(
        &self,
        id: &str,
        step: &str,
        result: Value,
    ) -> Result<WorkflowInstance, String>;
    async fn set_status(
        &self,
        id: &str,
        status: WorkflowStatus,
    ) -> Result<WorkflowInstance, String>;
}

pub type SharedWorkflowStore = Arc<dyn WorkflowStore>;

pub struct InMemoryWorkflowStore {
    inner: Mutex<HashMap<String, WorkflowInstance>>,
}

impl InMemoryWorkflowStore {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
        })
    }
}

#[async_trait]
impl WorkflowStore for InMemoryWorkflowStore {
    async fn create(&self, name: &str, input: Value) -> WorkflowInstance {
        let inst = WorkflowInstance {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            input,
            status: WorkflowStatus::Running,
            steps: HashMap::new(),
        };
        self.inner.lock().await.insert(inst.id.clone(), inst.clone());
        inst
    }

    async fn get(&self, id: &str) -> Option<WorkflowInstance> {
        self.inner.lock().await.get(id).cloned()
    }

    async fn complete_step(
        &self,
        id: &str,
        step: &str,
        result: Value,
    ) -> Result<WorkflowInstance, String> {
        let mut guard = self.inner.lock().await;
        let inst = guard.get_mut(id).ok_or_else(|| "workflow not found".to_string())?;
        inst.steps.insert(step.to_string(), result);
        Ok(inst.clone())
    }

    async fn set_status(
        &self,
        id: &str,
        status: WorkflowStatus,
    ) -> Result<WorkflowInstance, String> {
        let mut guard = self.inner.lock().await;
        let inst = guard.get_mut(id).ok_or_else(|| "workflow not found".to_string())?;
        inst.status = status;
        Ok(inst.clone())
    }
}
