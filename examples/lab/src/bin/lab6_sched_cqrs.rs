//! Lab 6 — scheduler + CQRS + event bus: interval tasks wired through
//! `ScheduleModule`, command/query buses with typed handlers, and in-process
//! `EventBus` pub/sub.
//!
//! Run: `cargo run -p lab --bin lab6_sched_cqrs`

use async_trait::async_trait;
use nestrs::prelude::*;
use nestrs_cqrs::{CqrsError, Query, QueryBus, QueryHandler};
use nestrs_cqrs::{Command, CommandBus, CommandHandler};
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

fn items() -> &'static Mutex<HashMap<u64, String>> {
    static ITEMS: OnceLock<Mutex<HashMap<u64, String>>> = OnceLock::new();
    ITEMS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed) + 1
}

// ---- Scheduler ----

static INTERVAL_HITS: AtomicU64 = AtomicU64::new(0);

#[injectable]
struct TasksService;

#[schedule_routes]
impl TasksService {
    #[interval(700)]
    async fn tick(&self) {
        INTERVAL_HITS.fetch_add(1, Ordering::Relaxed);
    }
}

// ---- CQRS command ----

struct CreateItem {
    name: String,
}

impl Command for CreateItem {
    type Response = u64;
}

struct CreateItemHandler;

#[async_trait]
impl CommandHandler<CreateItem> for CreateItemHandler {
    async fn execute(&self, c: CreateItem) -> Result<u64, CqrsError> {
        let id = next_id();
        items().lock().expect("items lock").insert(id, c.name);
        Ok(id)
    }
}

// ---- CQRS query ----

struct GetItem {
    id: u64,
}

impl Query for GetItem {
    type Response = Option<String>;
}

struct GetItemHandler;

#[async_trait]
impl QueryHandler<GetItem> for GetItemHandler {
    async fn execute(&self, q: GetItem) -> Result<Option<String>, CqrsError> {
        Ok(items().lock().expect("items lock").get(&q.id).cloned())
    }
}

// ---- Controllers ----

#[controller(prefix = "/sched")]
pub struct SchedController;

#[routes(state = TasksService)]
impl SchedController {
    #[get("/hits")]
    pub async fn hits() -> Json<serde_json::Value> {
        Json(json!({ "interval_hits": INTERVAL_HITS.load(Ordering::Relaxed) }))
    }
}

#[derive(serde::Deserialize)]
pub struct IdParams {
    id: u64,
}

#[controller(prefix = "/items")]
pub struct ItemsController;

#[routes(state = CommandBus)]
impl ItemsController {
    #[post("/create")]
    pub async fn create_item(
        State(bus): State<Arc<CommandBus>>,
        Json(body): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let name = body["name"].as_str().unwrap_or_default().to_string();
        if name.is_empty() {
            return Err(BadRequestException::new("name required"));
        }
        let id = bus
            .execute(CreateItem { name })
            .await
            .map_err(|e| InternalServerErrorException::new(e.message))?;
        Ok(Json(json!({ "id": id })))
    }
}

#[controller(prefix = "/itemsq")]
pub struct ItemQueryController;

#[routes(state = QueryBus)]
impl ItemQueryController {
    #[get("/:id")]
    pub async fn get_item(
        State(bus): State<Arc<QueryBus>>,
        #[param::param] p: IdParams,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let found = bus
            .execute(GetItem { id: p.id })
            .await
            .map_err(|e| InternalServerErrorException::new(e.message))?;
        Ok(Json(json!({ "found": found })))
    }
}

// ---- Event bus ----

static EVENTS_HANDLED: AtomicU64 = AtomicU64::new(0);

#[controller(prefix = "/bus")]
pub struct BusController;

#[routes(state = EventBus)]
impl BusController {
    #[post("/emit")]
    pub async fn emit_item_created(
        State(bus): State<Arc<EventBus>>,
        Json(body): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        bus.emit("item.created", &body).await;
        Ok(Json(json!({
            "emitted": true,
            "handled_so_far": EVENTS_HANDLED.load(Ordering::Relaxed),
        })))
    }

    #[get("/events")]
    pub async fn events_handled(State(_bus): State<Arc<EventBus>>) -> Json<serde_json::Value> {
        Json(json!({ "events_handled": EVENTS_HANDLED.load(Ordering::Relaxed) }))
    }
}

// ---- Wiring ----

/// Registers CQRS handlers and event subscriptions once the app boots.
/// Uses the framework lifecycle: `on_application_bootstrap` runs inside `listen*`.
struct BusSetup {
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
    events: Arc<EventBus>,
}

#[async_trait]
impl Injectable for BusSetup {
    fn construct(registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self {
            command_bus: registry.get::<CommandBus>(),
            query_bus: registry.get::<QueryBus>(),
            events: registry.get::<EventBus>(),
        })
    }

    async fn on_application_bootstrap(&self) {
        self.command_bus.register(Arc::new(CreateItemHandler)).await;
        self.query_bus.register(Arc::new(GetItemHandler)).await;

        self.events.subscribe("item.created", |_payload| {
            Box::pin(async {
                EVENTS_HANDLED.fetch_add(1, Ordering::Relaxed);
            })
        });
    }
}

#[module(
    imports = [ScheduleModule::for_root()],
    controllers = [SchedController, ItemsController, ItemQueryController, BusController],
    providers = [TasksService, CommandBus, QueryBus, EventBus, BusSetup]
)]
pub struct LabModule;

#[tokio::main]
async fn main() {
    NestFactory::create::<LabModule>()
        .set_global_prefix("lab")
        .listen_graceful(3600)
        .await;
}
