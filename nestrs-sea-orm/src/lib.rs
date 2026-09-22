//! SeaORM adapter for nestrs — NestJS TypeORM / Sequelize analogue.
//!
//! # What this crate provides
//!
//! - [`SeaOrmModule::for_root_async`] — connect and export `Arc<DatabaseConnection>`
//! - [`Repo`] — typed entity repository that prefers an ambient transaction
//! - [`install_sea_orm_transactional_middleware`] — request-scoped tx
//!   (commit on 2xx/3xx/4xx, rollback on 5xx)
//! - [`RowAuthz`] — pluggable deny-closed authorization for repo helpers
//! - [`bind_read`] / [`Bind`] — NestRS-style authorized path→row loading
//! - [`expose_schema`] (feature `expose`) — OpenAPI schema from one shared model type
//!
//! Pair with the umbrella crate feature `sea-orm` (+ `authz`) for
//! `AbilityAuthz`, `attach_row_authz_layer`, and
//! `NestApplication::require_route_posture`.

#![doc(html_root_url = "https://docs.rs/nestrs-sea-orm/1.5.0")]

mod authz;
mod bind;
mod error;
mod expose;
mod repo;
mod transaction;

pub use authz::RowAuthz;
pub use bind::{
    bind_delete, bind_read, bind_update, Bind, BindError, BoundAuthz, EntitySubject,
};
pub use error::RepoError;
#[cfg(feature = "expose")]
pub use expose::expose_schema;
#[cfg(not(feature = "expose"))]
pub use expose::expose_schema_hint;
pub use repo::{eq_condition, Repo};
pub use transaction::{
    current_sea_orm_transaction, install_sea_orm_transactional_middleware, SeaOrmTransactionSlot,
};

use nestrs_core::{DynamicModule, ProviderRegistry};
use sea_orm::{Database, DatabaseConnection, DbErr};
use std::any::TypeId;
use std::sync::Arc;

/// `TypeOrmModule.forRoot` analogue.
pub struct SeaOrmModule;

impl SeaOrmModule {
    /// Connect and export [`DatabaseConnection`] for injection.
    ///
    /// Must complete **before** `NestFactory::create` (same rule as
    /// `MongoModule::for_root_async`).
    pub async fn for_root_async(database_url: impl AsRef<str>) -> Result<DynamicModule, DbErr> {
        let conn = Database::connect(database_url.as_ref()).await?;
        Ok(Self::from_connection(conn))
    }

    /// Register an already-open connection (tests, custom pools).
    pub fn from_connection(conn: DatabaseConnection) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        registry.register_use_value::<DatabaseConnection>(Arc::new(conn));
        DynamicModule {
            registry,
            router: axum::Router::new(),
            exports: vec![TypeId::of::<DatabaseConnection>()],
        }
    }
}

/// SeaORM connection type (inject `Arc<DatabaseConnection>`).
pub type DbConn = DatabaseConnection;

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::entity::prelude::*;
    use sea_orm::{ConnectionTrait, Set};

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize)]
    #[sea_orm(table_name = "posts")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i32,
        pub title: String,
        pub author_id: i32,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}

    struct AllowAll;

    impl RowAuthz for AllowAll {
        fn can(&self, _action: &str, _subject_type: &str) -> bool {
            true
        }
        fn allows_row(&self, _action: &str, _subject_type: &str, _row: &serde_json::Value) -> bool {
            true
        }
    }

    struct DenyAll;

    impl RowAuthz for DenyAll {
        fn can(&self, _action: &str, _subject_type: &str) -> bool {
            false
        }
        fn allows_row(&self, _action: &str, _subject_type: &str, _row: &serde_json::Value) -> bool {
            false
        }
    }

    async fn setup() -> Arc<DatabaseConnection> {
        let db = Database::connect("sqlite::memory:")
            .await
            .expect("sqlite memory");
        db.execute_unprepared(
            "CREATE TABLE posts (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, author_id INTEGER NOT NULL)",
        )
        .await
        .expect("create table");
        Arc::new(db)
    }

    #[tokio::test]
    async fn sqlite_memory_connects() {
        let dm = SeaOrmModule::for_root_async("sqlite::memory:")
            .await
            .expect("sqlite memory should connect");
        assert!(!dm.exports.is_empty());
    }

    #[tokio::test]
    async fn repo_insert_and_find() {
        let db = setup().await;
        let repo = Repo::<Entity>::new(db);
        let inserted = repo
            .insert(ActiveModel {
                title: Set("hello".into()),
                author_id: Set(7),
                ..Default::default()
            })
            .await
            .expect("insert");
        assert_eq!(inserted.title, "hello");
        let found = repo
            .find_by_id(inserted.id)
            .await
            .expect("find")
            .expect("row");
        assert_eq!(found.author_id, 7);
    }

    #[tokio::test]
    async fn repo_authorized_denies_without_can() {
        let db = setup().await;
        let repo = Repo::<Entity>::new(db);
        let err = repo
            .find_by_id_authorized(&DenyAll, "Post", 1)
            .await
            .expect_err("denied");
        assert!(matches!(err, RepoError::Denied(_)));
    }

    #[tokio::test]
    async fn repo_authorized_allows() {
        let db = setup().await;
        let repo = Repo::<Entity>::new(db.clone());
        let row = repo
            .insert(ActiveModel {
                title: Set("x".into()),
                author_id: Set(1),
                ..Default::default()
            })
            .await
            .unwrap();
        let found = repo
            .find_by_id_authorized(&AllowAll, "Post", row.id)
            .await
            .unwrap();
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn ambient_tx_rollback_on_5xx() {
        use axum::body::Body;
        use axum::http::{Request as HttpRequest, StatusCode};
        use axum::routing::post;
        use axum::Router;
        use tower::ServiceExt;

        let db = setup().await;
        let repo_db = db.clone();
        let app = Router::new()
            .route(
                "/boom",
                post(move || {
                    let repo_db = repo_db.clone();
                    async move {
                        let repo = Repo::<Entity>::new(repo_db);
                        let _ = repo
                            .insert(ActiveModel {
                                title: Set("should-roll-back".into()),
                                author_id: Set(1),
                                ..Default::default()
                            })
                            .await
                            .expect("insert in tx");
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                db.clone(),
                install_sea_orm_transactional_middleware,
            ));

        let res = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/boom")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let count = Entity::find().all(db.as_ref()).await.unwrap().len();
        assert_eq!(count, 0, "5xx must roll back the ambient sea-orm tx");
    }

    #[tokio::test]
    async fn bind_read_not_found_and_allow() {
        let db = setup().await;
        let repo = Repo::<Entity>::new(db.clone());
        let err = bind_read(&repo, &AllowAll, "Post", 999)
            .await
            .expect_err("missing");
        assert!(matches!(err, BindError::NotFound));

        let row = repo
            .insert(ActiveModel {
                title: Set("bound".into()),
                author_id: Set(3),
                ..Default::default()
            })
            .await
            .unwrap();
        let bound = bind_read(&repo, &AllowAll, "Post", row.id)
            .await
            .expect("bind");
        assert_eq!(bound.title, "bound");

        let denied = bind_read(&repo, &DenyAll, "Post", row.id)
            .await
            .expect_err("deny");
        assert!(matches!(denied, BindError::Denied(_)));
    }
}
