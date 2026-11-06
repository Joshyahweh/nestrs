use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutboxStatus {
    Pending,
    Published,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxMessage {
    pub id: String,
    pub topic: String,
    pub payload: serde_json::Value,
    pub attempts: u32,
    pub status: OutboxStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetter {
    pub message: OutboxMessage,
    pub reason: String,
}

#[async_trait]
pub trait OutboxStore: Send + Sync {
    async fn add(&self, topic: &str, payload: serde_json::Value) -> OutboxMessage;
    async fn claim_batch(&self, limit: usize) -> Vec<OutboxMessage>;
    async fn mark_published(&self, id: &str);
    async fn mark_retry(&self, id: &str);
    async fn dead_letter(&self, id: &str, reason: &str);
    async fn inbox_seen(&self, consumer: &str, message_id: &str) -> bool;
    async fn inbox_mark(&self, consumer: &str, message_id: &str);
    async fn list_dead_letters(&self) -> Vec<DeadLetter>;
}

pub type SharedOutboxStore = Arc<dyn OutboxStore>;

pub struct InMemoryOutboxStore {
    messages: Mutex<HashMap<String, OutboxMessage>>,
    dead: Mutex<Vec<DeadLetter>>,
    inbox: Mutex<HashSet<(String, String)>>,
}

impl InMemoryOutboxStore {
    pub fn new() -> Self {
        Self {
            messages: Mutex::new(HashMap::new()),
            dead: Mutex::new(Vec::new()),
            inbox: Mutex::new(HashSet::new()),
        }
    }
}

impl Default for InMemoryOutboxStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OutboxStore for InMemoryOutboxStore {
    async fn add(&self, topic: &str, payload: serde_json::Value) -> OutboxMessage {
        let msg = OutboxMessage {
            id: Uuid::new_v4().to_string(),
            topic: topic.to_string(),
            payload,
            attempts: 0,
            status: OutboxStatus::Pending,
        };
        self.messages
            .lock()
            .await
            .insert(msg.id.clone(), msg.clone());
        msg
    }

    async fn claim_batch(&self, limit: usize) -> Vec<OutboxMessage> {
        let mut guard = self.messages.lock().await;
        guard
            .values_mut()
            .filter(|m| m.status == OutboxStatus::Pending)
            .take(limit)
            .map(|m| {
                m.attempts += 1;
                m.clone()
            })
            .collect()
    }

    async fn mark_published(&self, id: &str) {
        if let Some(m) = self.messages.lock().await.get_mut(id) {
            m.status = OutboxStatus::Published;
        }
    }

    async fn mark_retry(&self, id: &str) {
        if let Some(m) = self.messages.lock().await.get_mut(id) {
            m.status = OutboxStatus::Pending;
        }
    }

    async fn dead_letter(&self, id: &str, reason: &str) {
        let mut msgs = self.messages.lock().await;
        if let Some(mut m) = msgs.remove(id) {
            m.status = OutboxStatus::Dead;
            self.dead.lock().await.push(DeadLetter {
                message: m,
                reason: reason.to_string(),
            });
        }
    }

    async fn inbox_seen(&self, consumer: &str, message_id: &str) -> bool {
        self.inbox
            .lock()
            .await
            .contains(&(consumer.to_string(), message_id.to_string()))
    }

    async fn inbox_mark(&self, consumer: &str, message_id: &str) {
        self.inbox
            .lock()
            .await
            .insert((consumer.to_string(), message_id.to_string()));
    }

    async fn list_dead_letters(&self) -> Vec<DeadLetter> {
        self.dead.lock().await.clone()
    }
}
