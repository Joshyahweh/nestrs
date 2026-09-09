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

#[cfg(feature = "authz-row-level")]
use crate::policies::{
    conditions_to_sql, current_ability, current_principal, Action, RowPredicate, Subject,
};
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
    // All methods in this section require `authz-row-level` feature.

    #[cfg(feature = "authz-row-level")]
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
        let predicate = ability.predicate(&action, &subject);
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
            Some(r) => {
                // Post-load predicate check (leak prevention): the row must
                // also satisfy the rule's closure, not just the pushdown
                // conditions.
                if predicate.is_some() {
                    let json = row_json(&r, T::JSON_COLUMN)?;
                    if !predicate_allows(predicate.as_ref(), &json)? {
                        return Ok(None);
                    }
                }
                T::from_row(&r).map(Some)
            }
            None => Ok(None),
        }
    }

    #[cfg(feature = "authz-row-level")]
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
        let predicate = ability.predicate(&action, &subject);
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
        filter_rows_authorized(rows, predicate, T::JSON_COLUMN).await
    }
}

// ---------------------------------------------------------------------------
// Row-level predicate helpers (Feature D + authz-row-level)
// ---------------------------------------------------------------------------

#[cfg(feature = "authz-row-level")]
/// Parse a row's JSON blob (`Entity::JSON_COLUMN`) — the value predicates
/// receive.
fn row_json(row: &sqlx::any::AnyRow, json_column: &str) -> Result<serde_json::Value, sqlx::Error> {
    let blob: String = row.try_get(json_column)?;
    serde_json::from_str(&blob).map_err(|e| sqlx::Error::Decode(Box::new(e)))
}

#[cfg(feature = "authz-row-level")]
/// Evaluate a row-level predicate against one row. A predicate with no
/// [`Principal`] in request scope is a misconfiguration — fail loud
/// (deny-closed) rather than silently filtering everything.
fn predicate_allows(
    predicate: Option<&Arc<dyn RowPredicate>>,
    row: &serde_json::Value,
) -> Result<bool, sqlx::Error> {
    match predicate {
        None => Ok(true),
        Some(pred) => match current_principal() {
            Some(p) => Ok(pred.check(row, &p)),
            None => Err(sqlx::Error::Protocol(
                "row-level predicate requires a Principal in request scope \
                 (install_authn_middleware, a transport scope, or with_principal)"
                    .into(),
            )),
        },
    }
}

#[cfg(feature = "authz-row-level")]
/// Post-load filter shared by the authorized find paths: evaluate the
/// predicate on each row's JSON blob *before* decoding the entity, so
/// predicate-rejected rows never reach the caller.
async fn filter_rows_authorized<T: Entity>(
    rows: Vec<sqlx::any::AnyRow>,
    predicate: Option<Arc<dyn RowPredicate>>,
    json_column: &'static str,
) -> Result<Vec<T>, sqlx::Error> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if predicate.is_some() {
            let json = row_json(&row, json_column)?;
            if !predicate_allows(predicate.as_ref(), &json)? {
                continue;
            }
        }
        out.push(T::from_row(&row)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// find_many_authorized (authz-row-level)
// ---------------------------------------------------------------------------

#[cfg(feature = "authz-row-level")]
/// Parameters for [`Repository::find_many_authorized`].
///
/// `extra_where` / `extra_binds` compose a caller-supplied filter with the
/// policy pushdown: the extra clause's placeholders are numbered `$1..$k`
/// (bind order = `extra_binds` first), and the policy-derived binds continue
/// at `$k+1`. Numbered placeholders make SQL-text order independent of bind
/// order.
#[derive(Debug, Default, Clone)]
pub struct FindManyParams {
    /// `LIMIT n`. Applies to *fetched* rows: predicates without SQL pushdown
    /// (custom closures) filter after the limit, so the final count may be
    /// lower. Pre-built predicates push their check into the `WHERE` clause
    /// and paginate exactly.
    pub limit: Option<i64>,
    /// `OFFSET n` (same caveat as `limit`).
    pub offset: Option<i64>,
    /// Raw extra `WHERE` fragment (no leading keyword), placeholders numbered
    /// from `$1`. Injected first, ANDed with the policy pushdown.
    pub extra_where: Option<String>,
    /// Bind values for `extra_where`, in `$1..$k` order.
    pub extra_binds: Vec<serde_json::Value>,
}

#[cfg(feature = "authz-row-level")]
impl<T: Entity> Repository<T> {
    /// Fetch many rows with the request-scoped [`Ability`] enforced — the
    /// row-level analogue of [`Self::find_all_authorized`] with pagination
    /// and a caller-supplied extra filter.
    ///
    /// Enforcement is layered:
    /// 1. No `Ability` in request scope → `Err(sqlx::Error::Protocol)`
    ///    (deny-closed).
    /// 2. `can(action, Subject::Type(T::TABLE))` fails → `Ok(vec![])`.
    /// 3. The matched rule's declarative conditions **and** the predicate's
    ///    `sql_conditions(&principal)` pushdown are compiled into the
    ///    `WHERE` clause (via `conditions_to_sql`, JSON-blob path).
    /// 4. Surviving rows are post-filtered through the predicate closure
    ///    itself, so even a pushdown gap can't leak a row.
    ///
    /// Note: the pushdown renders `json_extract(data, '$.col')`, which is
    /// SQLite-flavored; on Postgres/MySQL the `database-sqlx` `AnyPool`
    /// JSON-blob path (shared with the existing `find_*_authorized`) needs a
    /// dialect-aware renderer (known limitation, inherited not introduced).
    ///
    /// Requires the `authz-row-level` feature.
    pub async fn find_many_authorized(
        &self,
        action: Action,
        params: FindManyParams,
    ) -> Result<Vec<T>, sqlx::Error> {
        let ability = current_ability().ok_or_else(|| {
            sqlx::Error::Protocol(
                "find_many_authorized requires an Ability in request scope \
                 (install_policies_middleware / run_in_ws_scope / a transport data context)"
                    .into(),
            )
        })?;
        let subject = Subject::Type(T::TABLE);
        if !ability.can(&action, &subject) {
            return Ok(Vec::new());
        }
        let predicate = ability.predicate(&action, &subject);
        let principal = current_principal();

        // WHERE parts: caller's extra filter, then rule conditions, then the
        // predicate's per-request pushdown. Bind indexes continue across all
        // three segments.
        let mut where_parts: Vec<String> = Vec::new();
        let mut binds: Vec<serde_json::Value> = params.extra_binds;
        let mut idx = binds.len() + 1;
        if let Some(extra) = &params.extra_where {
            where_parts.push(extra.clone());
        }
        if let Some(conds) = ability.constraint(&action, &subject) {
            if !conds.is_empty() {
                let (clause, cbinds) = conditions_to_sql(&conds, idx, Some(T::JSON_COLUMN));
                if !clause.is_empty() {
                    where_parts.push(clause);
                    idx += cbinds.len();
                    binds.extend(cbinds);
                }
            }
        }
        if let (Some(pred), Some(p)) = (&predicate, principal.as_ref()) {
            if let Some(conds) = pred.sql_conditions(p) {
                if !conds.is_empty() {
                    let (clause, cbinds) = conditions_to_sql(&conds, idx, Some(T::JSON_COLUMN));
                    if !clause.is_empty() {
                        where_parts.push(clause);
                        binds.extend(cbinds);
                    }
                }
            }
        }

        let mut sql = format!("SELECT * FROM {}", T::TABLE);
        if !where_parts.is_empty() {
            sql.push_str(&format!(" WHERE {}", where_parts.join(" AND ")));
        }
        sql.push_str(&format!(" ORDER BY {} ASC", T::ID_COLUMN));
        // LIMIT/OFFSET are rendered inline: they're internal i64s (no
        // injection surface) and the Any driver mis-types bound parameters in
        // LIMIT position on some backends.
        if let Some(limit) = params.limit {
            sql.push_str(&format!(" LIMIT {}", limit));
        }
        if let Some(offset) = params.offset {
            sql.push_str(&format!(" OFFSET {}", offset));
        }

        let mut q = sqlx::query(&sql);
        for v in &binds {
            q = bind_json(q, v);
        }
        let rows = q.fetch_all(self.pool.as_ref()).await?;
        filter_rows_authorized(rows, predicate, T::JSON_COLUMN).await
    }
}

impl<T> Repository<T>
where
    T: Entity + Serialize + DeserializeOwned,
{
    /// INSERT an entity via the JSON-blob path and return it with the new id
    /// populated. This is the seeding/constructor primitive: it deliberately
    /// bypasses row-level authz (under `authz-row-level`,
    /// [`CrudService::create`] is deny-closed and requires an `Ability` in
    /// scope — use that in request paths, this in factories/tests).
    pub async fn repo_crud_create(&self, entity: &T) -> Result<T, sqlx::Error> {
        let json = serde_json::to_value(entity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let row = insert_json_blob(self.pool.as_ref(), T::TABLE, &json).await?;
        T::from_row(&row)
    }
}

/// Shared `INSERT INTO {table} (data) VALUES ($1) RETURNING *` used by
/// [`CrudService::create`] and [`Repository::repo_crud_create`].
async fn insert_json_blob(
    pool: &sqlx::AnyPool,
    table: &str,
    json: &serde_json::Value,
) -> Result<sqlx::any::AnyRow, sqlx::Error> {
    let blob = serde_json::to_string(json).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
    let sql = format!("INSERT INTO {} (data) VALUES ($1) RETURNING *", table);
    sqlx::query(&sql).bind(blob).fetch_one(pool).await
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

    /// Deny-closed helper: an `Ability` must be installed in the request
    /// scope (mandatory row-level authorization — no opt-out, no `skip_auth`).
    #[cfg(feature = "authz-row-level")]
    fn required_ability(op: &str) -> Result<Arc<crate::policies::Ability>, sqlx::Error> {
        current_ability().ok_or_else(|| {
            sqlx::Error::Protocol(format!(
                "CrudService::{op} requires an Ability in request scope \
                 (install_policies_middleware / run_in_ws_scope / a transport \
                 data context) — authz-row-level makes row-level authorization \
                 mandatory"
            ))
        })
    }

    /// Deny-closed helper: evaluate the rule predicate against a row,
    /// mapping a predicate rejection to the standard write-denial error.
    #[cfg(feature = "authz-row-level")]
    fn check_row(
        predicate: Option<&Arc<dyn RowPredicate>>,
        row: &serde_json::Value,
        op: &str,
    ) -> Result<(), sqlx::Error> {
        if predicate_allows(predicate, row)? {
            Ok(())
        } else {
            Err(sqlx::Error::Protocol(format!(
                "policy denied: {op} on {} (row predicate)",
                T::TABLE
            )))
        }
    }

    /// INSERT a new row. The id is assigned by the database; the returned
    /// entity has the new id populated.
    #[cfg(not(feature = "authz-row-level"))]
    pub async fn create(&self, value: serde_json::Value) -> Result<T, sqlx::Error> {
        let entity: T =
            serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let json = serde_json::to_value(&entity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let row = insert_json_blob(self.repo.pool.as_ref(), T::TABLE, &json).await?;
        T::from_row(&row)
    }

    /// INSERT a new row with the request-scoped [`Ability`] enforced: the
    /// `create` action must be granted and the row-level predicate must
    /// accept the row being inserted (e.g. `AuthorIsCurrentUser` rejects
    /// rows authored as someone else). No `Ability` in scope → deny-closed
    /// `Err(sqlx::Error::Protocol)`.
    #[cfg(feature = "authz-row-level")]
    pub async fn create(&self, value: serde_json::Value) -> Result<T, sqlx::Error> {
        let ability = Self::required_ability("create")?;
        let subject = Subject::Type(T::TABLE);
        if !ability.can(&Action::Create, &subject) {
            return Err(sqlx::Error::Protocol(format!(
                "policy denied: create on {}",
                T::TABLE
            )));
        }
        let predicate = ability.predicate(&Action::Create, &subject);
        let entity: T =
            serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let json = serde_json::to_value(&entity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        // The predicate judges the CANDIDATE row before it exists.
        Self::check_row(predicate.as_ref(), &json, "create")?;
        let row = insert_json_blob(self.repo.pool.as_ref(), T::TABLE, &json).await?;
        T::from_row(&row)
    }

    #[cfg(not(feature = "authz-row-level"))]
    pub async fn read(&self, id: i64) -> Result<Option<T>, sqlx::Error> {
        self.repo.find_one(id).await
    }

    /// READ a row with the request-scoped [`Ability`] enforced (deny →
    /// `Ok(None)`, same channel as [`Repository::find_one_authorized`]).
    #[cfg(feature = "authz-row-level")]
    pub async fn read(&self, id: i64) -> Result<Option<T>, sqlx::Error> {
        self.repo.find_one_authorized(Action::Read, id).await
    }

    /// UPDATE a row by id (full JSON blob replace).
    #[cfg(not(feature = "authz-row-level"))]
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

    /// UPDATE a row by id with the request-scoped [`Ability`] enforced. The
    /// `update` action must be granted, the current row must be visible to
    /// the update rule's predicate (fetched through the *update* action's
    /// own constraint — invisible rows read as `Ok(None)`), and the
    /// replacement row must satisfy the predicate too (a row can't be moved
    /// out of the principal's scope).
    #[cfg(feature = "authz-row-level")]
    pub async fn update(
        &self,
        id: i64,
        value: serde_json::Value,
    ) -> Result<Option<T>, sqlx::Error> {
        let ability = Self::required_ability("update")?;
        let subject = Subject::Type(T::TABLE);
        if !ability.can(&Action::Update, &subject) {
            return Err(sqlx::Error::Protocol(format!(
                "policy denied: update on {}",
                T::TABLE
            )));
        }
        let predicate = ability.predicate(&Action::Update, &subject);
        // Visibility check runs through the update action's own constraint +
        // predicate: what you can't see for update, you can't update.
        let current = self.repo.find_one_authorized(Action::Update, id).await?;
        let Some(current) = current else {
            return Ok(None);
        };
        if predicate.is_some() {
            let current_json =
                serde_json::to_value(&current).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            Self::check_row(predicate.as_ref(), &current_json, "update")?;
        }
        let entity: T =
            serde_json::from_value(value).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        let json = serde_json::to_value(&entity).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
        // The replacement row must stay inside the principal's scope.
        Self::check_row(predicate.as_ref(), &json, "update")?;
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

    #[cfg(not(feature = "authz-row-level"))]
    pub async fn delete(&self, id: i64) -> Result<bool, sqlx::Error> {
        self.repo.delete(id).await
    }

    /// DELETE a row with the request-scoped [`Ability`] enforced: the
    /// `delete` action must be granted, the row must be visible to the
    /// delete rule's predicate, and the predicate must accept it (deny →
    /// `Err(sqlx::Error::Protocol)`; invisible → `Ok(false)`).
    #[cfg(feature = "authz-row-level")]
    pub async fn delete(&self, id: i64) -> Result<bool, sqlx::Error> {
        let ability = Self::required_ability("delete")?;
        let subject = Subject::Type(T::TABLE);
        if !ability.can(&Action::Delete, &subject) {
            return Err(sqlx::Error::Protocol(format!(
                "policy denied: delete on {}",
                T::TABLE
            )));
        }
        let predicate = ability.predicate(&Action::Delete, &subject);
        let current = self.repo.find_one_authorized(Action::Delete, id).await?;
        let Some(current) = current else {
            return Ok(false);
        };
        if predicate.is_some() {
            let current_json =
                serde_json::to_value(&current).map_err(|e| sqlx::Error::Decode(Box::new(e)))?;
            Self::check_row(predicate.as_ref(), &current_json, "delete")?;
        }
        self.repo.delete(id).await
    }

    #[cfg(not(feature = "authz-row-level"))]
    pub async fn list(&self) -> Result<Vec<T>, sqlx::Error> {
        self.repo.find_all().await
    }

    /// LIST all rows with the request-scoped [`Ability`] enforced (deny →
    /// `Ok(vec![])`, same channel as [`Repository::find_all_authorized`]).
    #[cfg(feature = "authz-row-level")]
    pub async fn list(&self) -> Result<Vec<T>, sqlx::Error> {
        self.repo.find_all_authorized(Action::Read).await
    }
}
