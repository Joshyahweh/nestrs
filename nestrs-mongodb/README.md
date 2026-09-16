# nestrs-mongodb

Mongoose-style MongoDB adapter for the [nestrs](https://crates.io/crates/nestrs) framework — the Rust equivalent of [`@nestjs/mongoose`](https://docs.nestjs.com/techniques/mongodb).

```rust,ignore
use nestrs_mongodb::{MongoModule, MongoService, Document, MongoRepository};
use bson::oid::ObjectId;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, Document)]
#[schema(collection = "users", timestamps)]
struct User {
    #[serde(skip_serializing_if = "Option::is_none")]
    _id: Option<ObjectId>,
    #[prop(index = "unique")]
    email: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    created_at: Option<bson::DateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_at: Option<bson::DateTime>,
}

#[tokio::main]
async fn main() {
    MongoModule::for_root("mongodb://127.0.0.1:27017");
    let svc = MongoService::new();
    let users: MongoRepository<User> = svc.repository("app");
    users.insert_one(User { _id: None, email: "ada@example.com".into(), name: "Ada".into(), created_at: None, updated_at: None }).await.unwrap();
}
```

## What you get

- `MongoModule::for_root(uri)` — global singleton client + named-database service.
- `MongoService` — async client / database / ping / list-databases helpers, ready to inject.
- `Document` derive macro + `#[schema(...)]` / `#[prop(...)]` attributes for typed Mongoose-style schemas (collection name, indexes, defaults, renames, refs).
- `MongoRepository<T>` — typed CRUD wrapper over a `Collection<T>`: `find_by_id`, `find_one`, `find`, `insert_one`, `insert_many`, `update_one`, `update_many`, `replace_one`, `delete_one`, `delete_many`, `count_documents`, `find_one_and_*`.
- `MongoOptions` builder for non-default URIs (app name, timeouts, direct-connection, replica-set).

## Feature flags

- `default = []` — TLS via `rustls`, BSON `compat-3-0-0` codec.
- `dns-resolver` — `mongodb+srv://` Atlas-style seed lists (pulls `hickory-*`).
- `all` — convenience for everything.

## License

MIT OR Apache-2.0.