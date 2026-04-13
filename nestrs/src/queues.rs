use crate::core::{DynamicModule, Injectable, ProviderRegistry};
use crate::injectable;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, Semaphore};

#[cfg(feature = "queues-redis")]
use redis::Client as RedisClient;

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

/// Serializable job envelope (Redis / Bull-style backends use JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueJob {
    pub id: u64,
    pub queue: String,
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

#[cfg(feature = "queues-redis")]
fn redis_queue_key(name: &str) -> String {
    format!("nestrs:queue:{name}")
}

enum QueueBackend {
    Memory {
        tx: mpsc::Sender<QueueJob>,
        rx: Mutex<Option<mpsc::Receiver<QueueJob>>>,
    },
    #[cfg(feature = "queues-redis")]
    Redis { key: String },
}

struct QueueState {
    name: &'static str,
    backend: QueueBackend,
    concurrency: usize,
}

#[derive(Clone)]
pub struct QueueHandle {
    queue: &'static str,
    memory_tx: Option<mpsc::Sender<QueueJob>>,
    #[cfg(feature = "queues-redis")]
    redis: Option<(Arc<RedisClient>, String)>,
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
            queue: self.queue.to_string(),
            name: name.into(),
            payload,
            attempts_left: options.attempts.max(1),
            backoff: options.backoff,
        };

        #[cfg(feature = "queues-redis")]
        if let Some((client, key)) = &self.redis {
            use redis::AsyncCommands;
            let body = serde_json::to_string(&job)
                .map_err(|e| QueueError::new(format!("job json: {e}")))?;
            let client = client.clone();
            let key = key.clone();
            if let Some(delay) = options.delay {
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = async {
                        let mut conn = client.get_multiplexed_async_connection().await.ok()?;
                        conn.lpush::<_, _, ()>(&key, body).await.ok()
                    }
                    .await;
                });
            } else {
                let mut conn = client
                    .get_multiplexed_async_connection()
                    .await
                    .map_err(|e| QueueError::new(format!("redis: {e}")))?;
                conn.lpush::<_, _, ()>(&key, body)
                    .await
                    .map_err(|e| QueueError::new(format!("redis lpush: {e}")))?;
            }
            return Ok(id);
        }

        let tx = self
            .memory_tx
            .as_ref()
            .ok_or_else(|| QueueError::new("queue transport misconfigured"))?
            .clone();
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
    dead_letter_tx: HashMap<&'static str, mpsc::Sender<QueueJob>>,
    #[cfg(feature = "queues-redis")]
    dead_letter_redis: HashMap<&'static str, String>,
    #[cfg(feature = "queues-redis")]
    redis: Option<Arc<RedisClient>>,
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
    pub fn from_configs(configs: &[QueueConfig]) -> Arc<Self> {
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
                    backend: QueueBackend::Memory {
                        tx,
                        rx: Mutex::new(Some(rx)),
                    },
                    concurrency: cfg.concurrency.max(1),
                }),
            );
        }

        let mut dead_letter_tx = HashMap::<&'static str, mpsc::Sender<QueueJob>>::new();
        for cfg in configs {
            if let Some(dlq) = cfg.dead_letter {
                let sender = queues
                    .get(dlq)
                    .and_then(|s| match &s.backend {
                        QueueBackend::Memory { tx, .. } => Some(tx.clone()),
                        #[cfg(feature = "queues-redis")]
                        QueueBackend::Redis { .. } => None,
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "QueuesModule::register: dead_letter queue `{dlq}` for `{}` is not registered",
                            cfg.name
                        )
                    });
                dead_letter_tx.insert(cfg.name, sender);
            }
        }

        Arc::new(Self {
            queues,
            dead_letter_tx,
            #[cfg(feature = "queues-redis")]
            dead_letter_redis: HashMap::new(),
            #[cfg(feature = "queues-redis")]
            redis: None,
            next_id: Arc::new(AtomicU64::new(1)),
            started: AtomicBool::new(false),
            tasks: Mutex::new(Vec::new()),
        })
    }

    /// Bull-style Redis lists (**LPUSH** / **BRPOP**) for durable jobs (feature **`queues-redis`**).
    #[cfg(feature = "queues-redis")]
    pub fn from_configs_redis(client: Arc<RedisClient>, configs: &[QueueConfig]) -> Arc<Self> {
        if configs.is_empty() {
            panic!("QueuesModule::register_with_redis requires at least one QueueConfig");
        }

        let mut queues = HashMap::<&'static str, Arc<QueueState>>::new();
        for cfg in configs {
            if queues.contains_key(cfg.name) {
                panic!(
                    "QueuesModule::register_with_redis: duplicate queue name `{}`",
                    cfg.name
                );
            }
            queues.insert(
                cfg.name,
                Arc::new(QueueState {
                    name: cfg.name,
                    backend: QueueBackend::Redis {
                        key: redis_queue_key(cfg.name),
                    },
                    concurrency: cfg.concurrency.max(1),
                }),
            );
        }

        let mut dead_letter_redis = HashMap::new();
        for cfg in configs {
            if let Some(dlq) = cfg.dead_letter {
                dead_letter_redis.insert(cfg.name, redis_queue_key(dlq));
            }
        }

        Arc::new(Self {
            queues,
            dead_letter_tx: HashMap::new(),
            dead_letter_redis,
            redis: Some(client),
            next_id: Arc::new(AtomicU64::new(1)),
            started: AtomicBool::new(false),
            tasks: Mutex::new(Vec::new()),
        })
    }

    pub fn queue(&self, name: &str) -> Option<QueueHandle> {
        let state = self.queues.get(name)?;
        let memory_tx = match &state.backend {
            QueueBackend::Memory { tx, .. } => Some(tx.clone()),
            #[cfg(feature = "queues-redis")]
            QueueBackend::Redis { .. } => None,
        };
        #[cfg(feature = "queues-redis")]
        let redis = match &state.backend {
            QueueBackend::Redis { key } => Some((self.redis.as_ref()?.clone(), key.clone())),
            QueueBackend::Memory { .. } => None,
        };
        Some(QueueHandle {
            queue: state.name,
            memory_tx,
            #[cfg(feature = "queues-redis")]
            redis,
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
            let sem = Arc::new(Semaphore::new(state.concurrency));
            let dlq_opt = self.dead_letter_tx.get(*queue_name).cloned();
            #[cfg(feature = "queues-redis")]
            let dlq_redis_key = self.dead_letter_redis.get(*queue_name).cloned();
            #[cfg(feature = "queues-redis")]
            let redis_client = self.redis.clone();

            match &state.backend {
                QueueBackend::Memory { tx, rx } => {
                    let rx = {
                        let mut guard = rx.lock().await;
                        guard.take()
                    };
                    let Some(mut rx) = rx else {
                        continue;
                    };
                    let tx = tx.clone();
                    let task = tokio::spawn(async move {
                        while let Some(job) = rx.recv().await {
                            Self::spawn_job_task(
                                job,
                                processors.clone(),
                                pick.clone(),
                                sem.clone(),
                                JobTransport::Memory {
                                    requeue_tx: tx.clone(),
                                    dlq_tx: dlq_opt.clone(),
                                },
                            );
                        }
                    });
                    join_handles.push(task);
                }
                #[cfg(feature = "queues-redis")]
                QueueBackend::Redis { key } => {
                    let Some(client) = redis_client.clone() else {
                        continue;
                    };
                    let key = key.clone();
                    let task = tokio::spawn(async move {
                        use redis::AsyncCommands;
                        loop {
                            let mut conn = match client.get_multiplexed_async_connection().await {
                                Ok(c) => c,
                                Err(_) => {
                                    tokio::time::sleep(Duration::from_secs(1)).await;
                                    continue;
                                }
                            };
                            let popped: Result<(String, String), redis::RedisError> =
                                conn.brpop(&key, 5.0).await;
                            let Ok((_k, payload)) = popped else {
                                continue;
                            };
                            let job: QueueJob = match serde_json::from_str(&payload) {
                                Ok(j) => j,
                                Err(_) => continue,
                            };
                            let retry_target = (client.clone(), key.clone());
                            Self::spawn_job_task(
                                job,
                                processors.clone(),
                                pick.clone(),
                                sem.clone(),
                                JobTransport::Redis {
                                    retry: retry_target,
                                    dlq: dlq_redis_key
                                        .as_ref()
                                        .map(|k| (client.clone(), k.clone())),
                                },
                            );
                        }
                    });
                    join_handles.push(task);
                }
            }
        }

        let mut guard = self.tasks.lock().await;
        guard.extend(join_handles);
    }

    fn spawn_job_task(
        job: QueueJob,
        processors: Arc<Vec<Arc<dyn QueueHandler>>>,
        pick: Arc<AtomicUsize>,
        sem: Arc<Semaphore>,
        transport: JobTransport,
    ) {
        tokio::spawn(async move {
            let permit = match sem.acquire_owned().await {
                Ok(p) => p,
                Err(_) => return,
            };
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
                match &transport {
                    JobTransport::Memory { requeue_tx, .. } => {
                        let _ = requeue_tx.send(next).await;
                    }
                    #[cfg(feature = "queues-redis")]
                    JobTransport::Redis { retry, .. } => {
                        if let Ok(json) = serde_json::to_string(&next) {
                            let _ = redis_lpush(&retry.0, &retry.1, &json).await;
                        }
                    }
                }
            } else {
                match &transport {
                    JobTransport::Memory { dlq_tx, .. } => {
                        if let Some(dlq) = dlq_tx {
                            let _ = dlq.send(job).await;
                        }
                    }
                    #[cfg(feature = "queues-redis")]
                    JobTransport::Redis { dlq, .. } => {
                        if let Some((c, k)) = dlq {
                            if let Ok(json) = serde_json::to_string(&job) {
                                let _ = redis_lpush(c, k, &json).await;
                            }
                        }
                    }
                }
            }
            drop(permit);
        });
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

enum JobTransport {
    Memory {
        requeue_tx: mpsc::Sender<QueueJob>,
        dlq_tx: Option<mpsc::Sender<QueueJob>>,
    },
    #[cfg(feature = "queues-redis")]
    Redis {
        retry: (Arc<RedisClient>, String),
        dlq: Option<(Arc<RedisClient>, String)>,
    },
}

#[cfg(feature = "queues-redis")]
async fn redis_lpush(client: &RedisClient, key: &str, json: &str) -> Result<(), ()> {
    use redis::AsyncCommands;
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .map_err(|_| ())?;
    conn.lpush::<_, _, ()>(key, json).await.map_err(|_| ())?;
    Ok(())
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

    #[cfg(feature = "queues-redis")]
    pub fn register_with_redis(url: &str, configs: &[QueueConfig]) -> DynamicModule {
        let client = RedisClient::open(url).expect("invalid redis URL for queues");
        let runtime = QueuesRuntime::from_configs_redis(Arc::new(client), configs);

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

pub async fn wire_queue_processors(registry: &ProviderRegistry) {
    if QUEUE_PROCESSORS.is_empty() {
        return;
    }
    let runtime = registry.get::<QueuesRuntime>();
    runtime.start_workers(registry).await;
}
