//! NestJS [`@nestjs/workflows`](https://docs.nestjs.com/fundamentals/workflows) analogue —
//! durable multi-step workflows with resume-after-restart and human-approval waits.
//!
//! Persist steps via [`WorkflowsService::complete_step`]; park with
//! [`WorkflowsService::wait_for_approval`] and continue with [`WorkflowsService::approve`].
//! Combine with `nestrs-locks` for single-runner steppers and `nestrs-outbox` for
//! side effects.
//!
//! Umbrella feature: `nestrs` / `workflows` → `nestrs::workflows`.
#![doc(html_root_url = "https://docs.rs/nestrs-workflows/1.7.1")]

mod module;
mod options;
mod store;

pub use module::WorkflowsModule;
pub use options::WorkflowsOptions;
pub use store::{
    InMemoryWorkflowStore, SharedWorkflowStore, WorkflowInstance, WorkflowStatus, WorkflowStore,
};

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
use serde_json::Value;
use std::sync::Arc;

pub struct WorkflowsService {
    store: SharedWorkflowStore,
}

impl WorkflowsService {
    pub fn from_options(opts: &WorkflowsOptions) -> Self {
        Self {
            store: opts.store.clone(),
        }
    }

    pub async fn start(&self, name: &str, input: Value) -> WorkflowInstance {
        self.store.create(name, input).await
    }

    pub async fn get(&self, id: &str) -> Option<WorkflowInstance> {
        self.store.get(id).await
    }

    /// Advance to the next step, persisting result. Survives process restart via the store.
    pub async fn complete_step(
        &self,
        id: &str,
        step: &str,
        result: Value,
    ) -> Result<WorkflowInstance, String> {
        self.store.complete_step(id, step, result).await
    }

    /// Park the workflow until [`Self::approve`].
    pub async fn wait_for_approval(&self, id: &str) -> Result<WorkflowInstance, String> {
        self.store
            .set_status(id, WorkflowStatus::WaitingApproval)
            .await
    }

    pub async fn approve(&self, id: &str) -> Result<WorkflowInstance, String> {
        self.store.set_status(id, WorkflowStatus::Running).await
    }
}

impl Injectable for WorkflowsService {
    fn construct(_registry: &nestrs_core::ProviderRegistry) -> Arc<Self> {
        panic!("WorkflowsService must be provided by WorkflowsModule::register(...)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn survives_logical_restart() {
        let store = InMemoryWorkflowStore::shared();
        let svc = WorkflowsService {
            store: store.clone(),
        };
        let inst = svc.start("order", json!({"id": 1})).await;
        svc.complete_step(&inst.id, "charge", json!({"ok": true}))
            .await
            .unwrap();
        // "restart": new service, same store
        let svc2 = WorkflowsService { store };
        let loaded = svc2.get(&inst.id).await.expect("loaded");
        assert!(loaded.steps.contains_key("charge"));
        assert_eq!(loaded.status, WorkflowStatus::Running);
    }
}
