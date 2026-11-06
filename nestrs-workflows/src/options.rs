use crate::store::{InMemoryWorkflowStore, SharedWorkflowStore};

#[derive(Clone)]
pub struct WorkflowsOptions {
    pub store: SharedWorkflowStore,
}

impl Default for WorkflowsOptions {
    fn default() -> Self {
        Self {
            store: InMemoryWorkflowStore::shared(),
        }
    }
}
