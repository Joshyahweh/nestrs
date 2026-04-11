use crate::core::{DynamicModule, Injectable, ProviderRegistry};
use crate::injectable;
use async_trait::async_trait;
use serde::Serialize;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, Semaphore};

#[doc(hidden)]
pub use linkme;

#[derive(Debug, Clone)]
pub struct QueueError {
    pub message: String,
}

impl QueueError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for QueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for QueueError {}

#[derive(Debug, Clone)]
pub struct JobOptions {
    pub attempts: u32,
    pub backoff: Option<Duration>,
    pub delay: Option<Duration>,
}

impl Default for JobOptions {
    fn default() -> Self {
        Self {
            attempts: 1,
            backoff: None,
            delay: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueJob {
    pub id: u64,
    pub queue: &'static str,
    pub name: String,
    pub payload: serde_json::Value,
    pub attempts_left: u32,
    pub backoff: Option<Duration>,
}

#[async_trait]
pub trait QueueHandler: Send + Sync + 'static {
    async fn process(&self, job: QueueJob) -> Result<(), QueueError>;
}

pub type QueueHandlerFactory = fn(&ProviderRegistry) -> Arc<dyn QueueHandler>;

pub fn handler_factory<T>(registry: &ProviderRegistry) -> Arc<dyn QueueHandler>
where
    T: Injectable + QueueHandler,
{
    registry.get::<T>()
}

pub struct QueueProcessorRegistration {
    pub queue: &'static str,
    pub create: QueueHandlerFactory,
}

#[linkme::distributed_slice]
pub static QUEUE_PROCESSORS: [QueueProcessorRegistration] = [..];

#[derive(Clone, Debug)]
pub struct QueueConfig {
    pub name: &'static str,
    pub concurrency: usize,
    pub buffer: usize,
    /// When set, jobs that **fail after all attempts** are pushed to this queue (Bull-style dead-letter).
    pub dead_letter: Option<&'static str>,
}

impl QueueConfig {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            concurrency: 1,
            buffer: 256,
            dead_letter: None,
        }
    }

    pub fn with_concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency.max(1);
        self
    }

    pub fn with_buffer(mut self, buffer: usize) -> Self {
        self.buffer = buffer.max(1);
        self
    }

    pub fn with_dead_letter(mut self, target: &'static str) -> Self {
        self.dead_letter = Some(target);
        self
    }
}

struct QueueState {
    name: &'static str,
    tx: mpsc::Sender<QueueJob>,
    rx: Mutex<Option<mpsc::Receiver<QueueJob>>>,
    concurrency: usize,
}

#[derive(Clone)]
pub struct QueueHandle {
    queue: &'static str,
    tx: mpsc::Sender<QueueJob>,
    next_id: Arc<AtomicU64>,
}

impl QueueHandle {
    pub async fn add_json(
        &self,
        name: impl Into<String>,
        payload: serde_json::Value,
    ) -> Result<u64, QueueError> {
        self.add_json_with_options(name, payload, JobOptions::default())
            .await
    }

    pub async fn add<T>(&self, name: impl Into<String>, payload: &T) -> Result<u64, QueueError>
    where
        T: Serialize + Send + Sync,
    {
        let json = serde_json::to_value(payload)
            .map_err(|e| QueueError::new(format!("job encode failed: {e}")))?;
        self.add_json(name, json).await
    }

    pub async fn add_json_with_options(
        &self,
        name: impl Into<String>,
        payload: serde_json::Value,
        options: JobOptions,
    ) -> Result<u64, QueueError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let job = QueueJob {
            id,
            queue: self.queue,
            name: name.into(),
            payload,
            attempts_left: options.attempts.max(1),
            backoff: options.backoff,
        };

        let tx = self.tx.clone();
        if let Some(delay) = options.delay {
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = tx.send(job).await;
            });
            Ok(id)
        } else {
            tx.send(job)
                .await
                .map_err(|_| QueueError::new("queue is closed"))?;
            Ok(id)
        }
    }
}

pub struct QueuesRuntime {
    queues: HashMap<&'static str, Arc<QueueState>>,
    /// Source queue name → DLQ sender (must be registered in the same `QueuesModule`).
    dead_letter_tx: HashMap<&'static str, mpsc::Sender<QueueJob>>,
    next_id: Arc<AtomicU64>,
    started: AtomicBool,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

#[nestrs::async_trait]
impl Injectable for QueuesRuntime {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        panic!("QueuesRuntime must be provided by QueuesModule::register(...)");
    }

    async fn on_application_shutdown(&self) {
        self.shutdown().await;
    }
}

impl QueuesRuntime {
    fn from_configs(configs: &[QueueConfig]) -> Arc<Self> {
        if configs.is_empty() {
            panic!("QueuesModule::register requires at least one QueueConfig");
        }

        let mut queues = HashMap::<&'static str, Arc<QueueState>>::new();
        for cfg in configs {
            if queues.contains_key(cfg.name) {
                panic!(
                    "QueuesModule::register: duplicate queue name `{}`",
                    cfg.name
                );
            }
            let (tx, rx) = mpsc::channel::<QueueJob>(cfg.buffer);
            queues.insert(
                cfg.name,
                Arc::new(QueueState {
                    name: cfg.name,
                    tx,
                    rx: Mutex::new(Some(rx)),
                    concurrency: cfg.concurrency.max(1),
                }),
            );
        }

        let mut dead_letter_tx = HashMap::<&'static str, mpsc::Sender<QueueJob>>::new();
        for cfg in configs {
            if let Some(dlq) = cfg.dead_letter {
                let sender = queues
                    .get(dlq)
                    .unwrap_or_else(|| {
                        panic!(
                            "QueuesModule::register: dead_letter queue `{dlq}` for `{}` is not registered",
                            cfg.name
                        )
                    })
                    .tx
                    .clone();
                dead_letter_tx.insert(cfg.name, sender);
            }
        }

        Arc::new(Self {
            queues,
            dead_letter_tx,
            next_id: Arc::new(AtomicU64::new(1)),
            started: AtomicBool::new(false),
            tasks: Mutex::new(Vec::new()),
        })
    }

    pub fn queue(&self, name: &str) -> Option<QueueHandle> {
        let state = self.queues.get(name)?;
        Some(QueueHandle {
            queue: state.name,
            tx: state.tx.clone(),
            next_id: self.next_id.clone(),
        })
    }

    pub fn expect_queue(&self, name: &str) -> QueueHandle {
        self.queue(name).unwrap_or_else(|| {
            let known = self.queues.keys().copied().collect::<Vec<_>>().join(", ");
            panic!("Queue `{name}` not registered. Known queues: [{known}]");
        })
    }

    pub async fn start_workers(&self, registry: &ProviderRegistry) {
        if self
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let mut join_handles = Vec::<tokio::task::JoinHandle<()>>::new();

        for (queue_name, state) in &self.queues {
            let factories = QUEUE_PROCESSORS
                .iter()
                .filter(|p| p.queue == *queue_name)
                .map(|p| p.create)
                .collect::<Vec<_>>();

            if factories.is_empty() {
                continue;
            }

            let processors = factories
                .into_iter()
                .map(|f| f(registry))
                .collect::<Vec<_>>();
            let processors = Arc::new(processors);
            let pick = Arc::new(AtomicUsize::new(0));

            let tx = state.tx.clone();
            let sem = Arc::new(Semaphore::new(state.concurrency));
            let dlq_opt = self.dead_letter_tx.get(*queue_name).cloned();

            let rx = {
                let mut guard = state.rx.lock().await;
                guard.take()
            };
            let Some(mut rx) = rx else {
                continue;
            };

            let task = tokio::spawn(async move {
                while let Some(job) = rx.recv().await {
                    let permit = match sem.clone().acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => break,
                    };

                    let processors = processors.clone();
                    let pick = pick.clone();
                    let tx = tx.clone();
                    let dlq_opt = dlq_opt.clone();

                    tokio::spawn(async move {
                        let idx = pick.fetch_add(1, Ordering::Relaxed) % processors.len();
                        let proc = processors[idx].clone();

                        let res = proc.process(job.clone()).await;
                        if res.is_ok() {
                            drop(permit);
                            return;
                        }
                        if job.attempts_left > 1 {
                            let mut next = job.clone();
                            next.attempts_left -= 1;
                            if let Some(backoff) = next.backoff {
                                tokio::time::sleep(backoff).await;
                            }
                            let _ = tx.send(next).await;
                        } else if let Some(ref dlq) = dlq_opt {
                            let _ = dlq.send(job).await;
                        }

                        drop(permit);
                    });
                }
            });

            join_handles.push(task);
        }

        let mut guard = self.tasks.lock().await;
        guard.extend(join_handles);
    }

    pub async fn shutdown(&self) {
        let tasks = {
            let mut guard = self.tasks.lock().await;
            std::mem::take(&mut *guard)
        };
        for t in tasks {
            t.abort();
        }
    }
}

#[injectable]
pub struct QueuesService {
    runtime: Arc<QueuesRuntime>,
}

impl QueuesService {
    pub fn queue(&self, name: &str) -> Option<QueueHandle> {
        self.runtime.queue(name)
    }

    pub fn expect_queue(&self, name: &str) -> QueueHandle {
        self.runtime.expect_queue(name)
    }
}

pub struct QueuesModule;

impl QueuesModule {
    pub fn register(configs: &[QueueConfig]) -> DynamicModule {
        let runtime = QueuesRuntime::from_configs(configs);

        let mut registry = ProviderRegistry::new();
        registry.override_provider::<QueuesRuntime>(runtime);
        registry.register::<QueuesService>();

        DynamicModule::from_parts(
            registry,
            axum::Router::new(),
            vec![TypeId::of::<QueuesRuntime>(), TypeId::of::<QueuesService>()],
        )
    }
}

/// Wire all registered `#[queue_processor]` handlers into running queue workers.
///
/// Called automatically by `nestrs` during application bootstrap (feature: `queues`).
pub async fn wire_queue_processors(registry: &ProviderRegistry) {
    if QUEUE_PROCESSORS.is_empty() {
        return;
    }
    let runtime = registry.get::<QueuesRuntime>();
    runtime.start_workers(registry).await;
}
