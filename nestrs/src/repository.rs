//! Typed **repository** abstraction over [`sqlx::AnyPool`] (NestJS-TypeORM analogue).
//!
//! * [`Entity`] — hand-written trait; no derive macro. Each entity knows its
//!   table name, its primary key, and how to decode itself from a row.
//! * [`Repository<T>`] — typed per-entity facade with `find_one` / `find_all` /
//!   `find_where` / `delete` / `count` plus the `*_authorized` variants from
//!   Feature D (RLS) that append a policy-derived `WHERE` clause.
//! * [`CrudService<T>`] — `create` / `read` / `update` / `delete` / `list`
//!   over the repository. Uses a `(id INTEGER PK, data TEXT)` shape so it
//!   works on every SQL backend without per-entity boilerplate.
//!
//! All symbols are gated behind the `database-sqlx` Cargo feature.

use crate::policies::{conditions_to_sql, current_ability, Action, Subject};
use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::Row;
use std::marker::PhantomData;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// Hand-written entity trait. Each implementor declares the SQL table name and
/// how to decode one row. `id()` is `Option<i64>` so pre-insert entities can
/// return `None` and `CrudService::create` can INSERT then return the new id.
pub trait Entity: Sized + Send + Sync + 'static {
    /// SQL table name (no quoting — keep it simple ASCII).
    const TABLE: &'static str;
    /// Primary key column name.
    const ID_COLUMN: &'static str = "id";
    /// JSON-blob column that stores the entity's serialized fields. Used by
    /// `find_*_authorized` to translate Ability conditions into SQL.
    const JSON_COLUMN: &'static str = "data";

    /// Primary key value. `None` for new (un-inserted) entities.
    fn id(&self) -> Option<i64>;
    /// Decode this entity from a single SQL row.
    fn from_row(row: &sqlx::any::AnyRow) -> Result<Self, sqlx::Error>;
}

// ---------------------------------------------------------------------------
// Repository<T>
// ---------------------------------------------------------------------------

/// Typed per-entity repository. Cheap to clone (holds an `Arc<AnyPool>`).
#[derive(Clone)]
pub struct Repository<T: Entity> {
    pool: Arc<sqlx::AnyPool>,
    _marker: PhantomData<T>,
}

impl<T: Entity> Repository<T> {
    /// Build a repository over the given pool. Pair with
    /// [`crate::SqlxDatabaseService::pool_arc`] in a handler.
    pub fn new(pool: Arc<sqlx::AnyPool>) -> Self {
        Self {
            pool,
            _marker: PhantomData,
        }
    }

    /// Look up an entity by primary key.
    pub async fn find_one(&self, id: i64) -> Result<Option<T>, sqlx::Error> {
        let sql = format!("SELECT * FROM {} WHERE {} = $1", T::TABLE, T::ID_COLUMN);
        let row = sqlx::query(&sql)
            .bind(id)
            .fetch_optional(self.pool.as_ref())
            .await?;
        match row {
            Some(r) => T::from_row(&r).map(Some),
            None => Ok(None),
        }
    }

    /// Return all rows (ordered by id ascending for stable pagination tests).
    pub async fn find_all(&self) -> Result<Vec<T>, sqlx::Error> {
        let sql = format!("SELECT * FROM {} ORDER BY {} ASC", T::TABLE, T::ID_COLUMN);
        let rows = sqlx::query(&sql).fetch_all(self.pool.as_ref()).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Run a `WHERE` clause that you compose yourself (still parameterized via
    /// the bound values). Use this for queries the convenience methods can't
    /// express. The clause is appended after `WHERE ` (no leading keyword).
    pub async fn find_where(
        &self,
        where_clause: &str,
        binds: &[serde_json::Value],
    ) -> Result<Vec<T>, sqlx::Error> {
        let sql = format!(
            "SELECT * FROM {} WHERE {} ORDER BY {} ASC",
            T::TABLE,
            where_clause,
            T::ID_COLUMN
        );
        let mut q = sqlx::query(&sql);
        for v in binds {
            q = bind_json(q, v);
        }
        let rows = q.fetch_all(self.pool.as_ref()).await?;
        rows.iter().map(T::from_row).collect()
    }

    /// Delete by primary key. Returns `true` when a row was removed.
    pub async fn delete(&self, id: i64) -> Result<bool, sqlx::Error> {
        let sql = format!("DELETE FROM {} WHERE {} = $1", T::TABLE, T::ID_COLUMN);
        let result = sqlx::query(&sql)
            .bind(id)
            .execute(self.pool.as_ref())
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Count all rows in the table.
    pub async fn count(&self) -> Result<i64, sqlx::Error> {
        let sql = format!("SELECT COUNT(*) AS c FROM {}", T::TABLE);
        let row = sqlx::query(&sql).fetch_one(self.pool.as_ref()).await?;
        let c: i64 = row.try_get("c")?;
        Ok(c)
    }

    /// Direct access to the underlying pool (for advanced queries).
    pub fn pool(&self) -> &sqlx::AnyPool {
        self.pool.as_ref()
    }

    // ----- Feature D: policy-driven variants ---------------------------------

    /// Same as [`Self::find_one`] but consults the request-scoped [`Ability`]
    /// and appends the resolved constraint as a `WHERE` clause. Returns
    /// `Ok(None)` when the principal lacks the action OR the constraint
    /// filters the row out.
    ///
    /// The action is evaluated against `Subject::Type(T::TABLE)` (entities
    /// declare their table name as their type). Conditions on the matched
    /// rule are appended as a parameterized `AND` clause.
    pub async fn find_one_authorized(
        &self,
        action: Action,
        id: i64,
    ) -> Result<Option<T>, sqlx::Error> {
        let ability = current_ability().ok_or_else(|| {
            sqlx::Error::Protocol(
                "find_one_authorized requires install_policies_middleware (no Ability on request)"
                    .into(),
            )
        })?;
        let subject = Subject::Type(T::TABLE);
        if !ability.can(&action, &subject) {
            return Ok(None);
        }
        let (sql, binds) = match ability.constraint(&action, &subject) {
            Some(conds) if !conds.is_empty() => {
                let (clause, mut binds) = conditions_to_sql(&conds, 2, Some(T::JSON_COLUMN));
                let sql = format!(
                    "SELECT * FROM {} WHERE {} = $1 AND ({}) ORDER BY {} ASC",
                    T::TABLE,
                    T::ID_COLUMN,
                    clause,
                    T::ID_COLUMN,
                );
                // The constraint's $1 was rewritten to $2+; the entity id is $1.
                binds.insert(0, serde_json::Value::from(id));
                (sql, binds)
            }
            _ => (
                format!("SELECT * FROM {} WHERE {} = $1", T::TABLE, T::ID_COLUMN),
                vec![serde_json::Value::from(id)],
            ),
        };
        let mut q = sqlx::query(&sql);
        for v in &binds {
            q = bind_json(q, v);
        }
        let row = q.fetch_optional(self.pool.as_ref()).await?;
        match row {
            Some(r) => T::from_row(&r).map(Some),
            None => Ok(None),
        }
    }

    /// Same as [`Self::find_all`] but appends the policy constraint as a
    /// `WHERE` clause.
    pub async fn find_all_authorized(&self, action: Action) -> Result<Vec<T>, sqlx::Error> {
        let ability = current_ability().ok_or_else(|| {
            sqlx::Error::Protocol("find_all_authorized requires install_policies_middleware".into())
        })?;
        let subject = Subject::Type(T::TABLE);
        if !ability.can(&action, &subject) {
            return Ok(Vec::new());
        }
        let (sql, binds) = match ability.constraint(&action, &subject) {
            Some(conds) if !conds.is_empty() => {
                let (clause, binds) = conditions_to_sql(&conds, 1, Some(T::JSON_COLUMN));
                (
                    format!(
                        "SELECT * FROM {} WHERE {} ORDER BY {} ASC",
                        T::TABLE,
                        clause,
                        T::ID_COLUMN,
                    ),
                    binds,
                )
            }
            _ => (
                format!("SELECT * FROM {} ORDER BY {} ASC", T::TABLE, T::ID_COLUMN),
                Vec::new(),
            ),
        };
        let mut q = sqlx::query(&sql);
        for v in &binds {
            q = bind_json(q, v);
        }
        let rows = q.fetch_all(self.pool.as_ref()).await?;
        rows.iter().map(T::from_row).collect()
    }
}

impl<T> Repository<T>
where
    T: Entity + Serialize + DeserializeOwned,
{
    /// Convenience: round-trip an entity through the `CrudService` JSON-blob
    /// path. Returns the entity with the new id populated.
    pub async fn repo_crud_create(&self, entity: &T) -> Result<T, sqlx::Error> {
        let value = serde_json::to_value(entity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        CrudService::<T>::new(self.pool.clone()).create(value).await
    }
}

/// Bind a JSON value to a sqlx query in the same shape as `find_where`.
fn bind_json<'q>(
    q: sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>>,
    v: &serde_json::Value,
) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments<'q>> {
    match v {
        serde_json::Value::Null => q.bind(Option::<i64>::None),
        serde_json::Value::Bool(b) => q.bind(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                q.bind(i)
            } else if let Some(f) = n.as_f64() {
                q.bind(f)
            } else {
                q.bind(n.to_string())
            }
        }
        serde_json::Value::String(s) => q.bind(s.clone()),
        other => q.bind(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// CrudService<T>
// ---------------------------------------------------------------------------

/// Thin CRUD wrapper around [`Repository<T>`]. Provides the standard
/// `create` / `read` / `update` / `delete` / `list` shape that controllers
/// commonly expect. Persists each entity as a JSON blob in a `data TEXT`
/// column so the same service code works on every SQL backend.
pub struct CrudService<T: Entity + Serialize + DeserializeOwned> {
    repo: Repository<T>,
}

impl<T: Entity + Serialize + DeserializeOwned> CrudService<T> {
    pub fn new(pool: Arc<sqlx::AnyPool>) -> Self {
        Self {
            repo: Repository::new(pool),
        }
    }

    pub fn repo(&self) -> &Repository<T> {
        &self.repo
    }

    /// INSERT a new row. The id is assigned by the database; the returned
    /// entity has the new id populated.
    pub async fn create(&self, value: serde_json::Value) -> Result<T, sqlx::Error> {
        let entity: T =
            serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let json = serde_json::to_value(&entity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let blob = serde_json::to_string(&json).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let sql = format!("INSERT INTO {} (data) VALUES ($1) RETURNING *", T::TABLE);
        let row = sqlx::query(&sql)
            .bind(blob)
            .fetch_one(self.repo.pool.as_ref())
            .await?;
        T::from_row(&row)
    }

    pub async fn read(&self, id: i64) -> Result<Option<T>, sqlx::Error> {
        self.repo.find_one(id).await
    }

    /// UPDATE a row by id (full JSON blob replace).
    pub async fn update(
        &self,
        id: i64,
        value: serde_json::Value,
    ) -> Result<Option<T>, sqlx::Error> {
        let entity: T =
            serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let json = serde_json::to_value(&entity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let blob = serde_json::to_string(&json).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let sql = format!(
            "UPDATE {} SET data = $1 WHERE {} = $2 RETURNING *",
            T::TABLE,
            T::ID_COLUMN
        );
        let row = sqlx::query(&sql)
            .bind(blob)
            .bind(id)
            .fetch_optional(self.repo.pool.as_ref())
            .await?;
        match row {
            Some(r) => T::from_row(&r).map(Some),
            None => Ok(None),
        }
    }

    pub async fn delete(&self, id: i64) -> Result<bool, sqlx::Error> {
        self.repo.delete(id).await
    }

    pub async fn list(&self) -> Result<Vec<T>, sqlx::Error> {
        self.repo.find_all().await
    }
}
