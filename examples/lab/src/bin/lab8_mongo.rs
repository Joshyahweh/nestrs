//! Lab 8 — MongoDB: live CRUD round-trip through `MongoService` (DI-injected)
//! against the local mongod, plus health ping.
//!
//! Run: `cargo run -p lab --bin lab8_mongo`

use mongodb::bson::{doc, Document};
use nestrs::prelude::*;
use std::sync::Arc;

const DB: &str = "lab8";
const COLL: &str = "users";

#[derive(serde::Deserialize)]
pub struct NameParams {
    name: String,
}

#[controller(prefix = "/db")]
pub struct MongoController;

#[routes(state = MongoService)]
impl MongoController {
    #[get("/ping")]
    pub async fn ping(State(svc): State<Arc<MongoService>>) -> Result<Json<serde_json::Value>, HttpException> {
        svc.ping()
            .await
            .map(|_| Json(serde_json::json!({ "ok": true })))
            .map_err(BadRequestException::new)
    }

    #[post("/users")]
    pub async fn create_user(
        State(svc): State<Arc<MongoService>>,
        Json(body): Json<serde_json::Value>,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let name = body["name"].as_str().unwrap_or_default().to_string();
        if name.is_empty() {
            return Err(BadRequestException::new("name required"));
        }
        let coll = svc
            .database(DB)
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?
            .collection::<Document>(COLL);
        let res = coll
            .insert_one(doc! { "name": name })
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?;
        Ok(Json(serde_json::json!({ "inserted_id": res.inserted_id.to_string() })))
    }

    #[get("/users/:name")]
    pub async fn find_user(
        State(svc): State<Arc<MongoService>>,
        #[param::param] p: NameParams,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let coll = svc
            .database(DB)
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?
            .collection::<Document>(COLL);
        let found = coll
            .find_one(doc! { "name": &p.name })
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?;
        Ok(Json(match found {
            Some(d) => serde_json::json!({ "found": true, "name": d.get_str("name").unwrap_or("") }),
            None => serde_json::json!({ "found": false }),
        }))
    }

    #[get("/users")]
    pub async fn list_users(
        State(svc): State<Arc<MongoService>>,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let coll = svc
            .database(DB)
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?
            .collection::<Document>(COLL);
        let mut cursor = coll
            .find(doc! {})
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?;
        let mut names = Vec::new();
        while let Some(d) = cursor.try_next().await.map_err(|e| BadRequestException::new(e.to_string()))? {
            names.push(d.get_str("name").unwrap_or("").to_string());
        }
        names.sort();
        Ok(Json(serde_json::json!({ "count": names.len(), "names": names })))
    }

    #[delete("/users/:name")]
    pub async fn delete_user(
        State(svc): State<Arc<MongoService>>,
        #[param::param] p: NameParams,
    ) -> Result<Json<serde_json::Value>, HttpException> {
        let coll = svc
            .database(DB)
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?
            .collection::<Document>(COLL);
        let res = coll
            .delete_many(doc! { "name": &p.name })
            .await
            .map_err(|e| BadRequestException::new(e.to_string()))?;
        Ok(Json(serde_json::json!({ "deleted": res.deleted_count })))
    }
}

// futures_util TryStreamExt for cursor.try_next()
use futures_util::TryStreamExt;

#[module(
    imports = [DynamicModule::from_module::<nestrs::MongoModule>()],
    controllers = [MongoController]
)]
pub struct LabModule;

#[tokio::main]
async fn main() {
    // Sets the global URI consumed lazily by MongoService.
    nestrs::MongoModule::for_root("mongodb://127.0.0.1:27017");

    NestFactory::create::<LabModule>()
        .set_global_prefix("lab")
        .listen_graceful(3700)
        .await;
}
