//! Prisma-like relation query helpers (`include`-style loads + connect/disconnect mutations).

use crate::client::SortOrder;

/// Scalar relation key value used in connect/disconnect/include helpers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RelationIdValue {
    Int(i64),
    Text(String),
}

/// Options for relation include queries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncludeOptions {
    pub order_by: Option<(String, SortOrder)>,
    pub take: Option<i64>,
    pub skip: Option<i64>,
}

/// One-to-many include spec: load children by `child_fk_column = parent_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneToManyIncludeSpec {
    pub child_table: String,
    pub child_fk_column: String,
}

impl OneToManyIncludeSpec {
    pub fn new(child_table: impl Into<String>, child_fk_column: impl Into<String>) -> Self {
        Self {
            child_table: child_table.into(),
            child_fk_column: child_fk_column.into(),
        }
    }
}

/// One-to-one include spec: load one record by `table.fk_column = owner_id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneToOneIncludeSpec {
    pub table: String,
    pub fk_column: String,
}

impl OneToOneIncludeSpec {
    pub fn new(table: impl Into<String>, fk_column: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            fk_column: fk_column.into(),
        }
    }
}

/// Many-to-many include spec through an explicit join table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManyToManyIncludeSpec {
    pub related_table: String,
    pub related_pk_column: String,
    pub join_table: String,
    pub join_left_column: String,
    pub join_right_column: String,
}

impl ManyToManyIncludeSpec {
    pub fn new(
        related_table: impl Into<String>,
        related_pk_column: impl Into<String>,
        join_table: impl Into<String>,
        join_left_column: impl Into<String>,
        join_right_column: impl Into<String>,
    ) -> Self {
        Self {
            related_table: related_table.into(),
            related_pk_column: related_pk_column.into(),
            join_table: join_table.into(),
            join_left_column: join_left_column.into(),
            join_right_column: join_right_column.into(),
        }
    }
}

/// Mutation spec for FK-based relations (`connect`/`disconnect`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyMutationSpec {
    pub table: String,
    pub record_pk_column: String,
    pub fk_column: String,
    pub nullable_fk: bool,
}

impl ForeignKeyMutationSpec {
    pub fn new(
        table: impl Into<String>,
        record_pk_column: impl Into<String>,
        fk_column: impl Into<String>,
        nullable_fk: bool,
    ) -> Self {
        Self {
            table: table.into(),
            record_pk_column: record_pk_column.into(),
            fk_column: fk_column.into(),
            nullable_fk,
        }
    }
}

/// Mutation spec for explicit many-to-many join tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinMutationSpec {
    pub join_table: String,
    pub left_column: String,
    pub right_column: String,
}

impl JoinMutationSpec {
    pub fn new(
        join_table: impl Into<String>,
        left_column: impl Into<String>,
        right_column: impl Into<String>,
    ) -> Self {
        Self {
            join_table: join_table.into(),
            left_column: left_column.into(),
            right_column: right_column.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationQueryError {
    #[error("sql error: {0}")]
    Sql(String),
    #[error("disconnect requires nullable foreign key")]
    DisconnectRequiresNullableFk,
}

#[cfg(feature = "sqlx")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DbProvider {
    PostgreSql,
    MySql,
    SqlServer,
    Sqlite,
    CockroachDb,
    Unknown,
}

#[cfg(feature = "sqlx")]
fn detect_provider(database_url: &str) -> DbProvider {
    let lower = database_url.to_ascii_lowercase();
    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        if lower.contains("cockroach") || lower.contains("cockroachdb") {
            DbProvider::CockroachDb
        } else {
            DbProvider::PostgreSql
        }
    } else if lower.starts_with("mysql://") || lower.starts_with("mariadb://") {
        DbProvider::MySql
    } else if lower.starts_with("sqlserver://") || lower.starts_with("mssql://") {
        DbProvider::SqlServer
    } else if lower.starts_with("sqlite:") || lower.starts_with("file:") {
        DbProvider::Sqlite
    } else {
        DbProvider::Unknown
    }
}

#[cfg(feature = "sqlx")]
fn quote_ident(provider: DbProvider, id: &str) -> String {
    match provider {
        DbProvider::PostgreSql
        | DbProvider::Sqlite
        | DbProvider::CockroachDb
        | DbProvider::Unknown => {
            format!("\"{}\"", id.replace('"', "\"\""))
        }
        DbProvider::MySql => format!("`{}`", id.replace('`', "``")),
        DbProvider::SqlServer => format!("[{}]", id.replace(']', "]]")),
    }
}

#[cfg(feature = "sqlx")]
fn bind_relation_id<'a>(qb: &mut sqlx::QueryBuilder<'a, sqlx::Any>, value: &RelationIdValue) {
    match value {
        RelationIdValue::Int(v) => {
            qb.push_bind(*v);
        }
        RelationIdValue::Text(v) => {
            qb.push_bind(v.clone());
        }
    }
}

#[cfg(feature = "sqlx")]
impl crate::PrismaService {
    /// Include-like load for one-to-many relations.
    pub async fn include_one_to_many_as<T>(
        &self,
        spec: &OneToManyIncludeSpec,
        parent_id: RelationIdValue,
        opts: IncludeOptions,
    ) -> Result<Vec<T>, RelationQueryError>
    where
        for<'r> T: sqlx::FromRow<'r, sqlx::any::AnyRow> + Send + Unpin,
    {
        let pool = crate::sqlx_pool()
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))?;
        let provider = detect_provider(&self.client().database_url);
        let table = quote_ident(provider, &spec.child_table);
        let fk = quote_ident(provider, &spec.child_fk_column);

        let mut qb =
            sqlx::QueryBuilder::<sqlx::Any>::new(format!("SELECT * FROM {table} WHERE {fk} = "));
        bind_relation_id(&mut qb, &parent_id);

        if let Some((ref col, ord)) = opts.order_by {
            qb.push(" ORDER BY ");
            qb.push(quote_ident(provider, col));
            qb.push(" ");
            qb.push(ord.as_sql());
        }
        if let Some(take) = opts.take {
            qb.push(" LIMIT ");
            qb.push_bind(take);
        }
        if let Some(skip) = opts.skip {
            if opts.take.is_none() && provider == DbProvider::Sqlite {
                // SQLite requires LIMIT before OFFSET.
                qb.push(" LIMIT -1");
            }
            qb.push(" OFFSET ");
            qb.push_bind(skip);
        }

        qb.build_query_as()
            .fetch_all(pool)
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))
    }

    /// Include-like load for one-to-one relations (`LIMIT 1`).
    pub async fn include_one_to_one_as<T>(
        &self,
        spec: &OneToOneIncludeSpec,
        owner_id: RelationIdValue,
    ) -> Result<Option<T>, RelationQueryError>
    where
        for<'r> T: sqlx::FromRow<'r, sqlx::any::AnyRow> + Send + Unpin,
    {
        let pool = crate::sqlx_pool()
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))?;
        let provider = detect_provider(&self.client().database_url);
        let table = quote_ident(provider, &spec.table);
        let fk = quote_ident(provider, &spec.fk_column);

        let mut qb =
            sqlx::QueryBuilder::<sqlx::Any>::new(format!("SELECT * FROM {table} WHERE {fk} = "));
        bind_relation_id(&mut qb, &owner_id);
        qb.push(" LIMIT 1");

        qb.build_query_as()
            .fetch_optional(pool)
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))
    }

    /// Include-like load for explicit many-to-many relations using an explicit join table.
    pub async fn include_many_to_many_as<T>(
        &self,
        spec: &ManyToManyIncludeSpec,
        left_id: RelationIdValue,
        opts: IncludeOptions,
    ) -> Result<Vec<T>, RelationQueryError>
    where
        for<'r> T: sqlx::FromRow<'r, sqlx::any::AnyRow> + Send + Unpin,
    {
        let pool = crate::sqlx_pool()
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))?;
        let provider = detect_provider(&self.client().database_url);
        let related_table = quote_ident(provider, &spec.related_table);
        let related_pk = quote_ident(provider, &spec.related_pk_column);
        let join_table = quote_ident(provider, &spec.join_table);
        let join_left = quote_ident(provider, &spec.join_left_column);
        let join_right = quote_ident(provider, &spec.join_right_column);

        let mut qb = sqlx::QueryBuilder::<sqlx::Any>::new(format!(
            "SELECT r.* FROM {related_table} r INNER JOIN {join_table} j ON j.{join_right} = r.{related_pk} WHERE j.{join_left} = "
        ));
        bind_relation_id(&mut qb, &left_id);

        if let Some((ref col, ord)) = opts.order_by {
            qb.push(" ORDER BY r.");
            qb.push(quote_ident(provider, col));
            qb.push(" ");
            qb.push(ord.as_sql());
        }
        if let Some(take) = opts.take {
            qb.push(" LIMIT ");
            qb.push_bind(take);
        }
        if let Some(skip) = opts.skip {
            if opts.take.is_none() && provider == DbProvider::Sqlite {
                qb.push(" LIMIT -1");
            }
            qb.push(" OFFSET ");
            qb.push_bind(skip);
        }

        qb.build_query_as()
            .fetch_all(pool)
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))
    }

    /// Connect a record by setting a foreign key (`UPDATE ... SET fk = target WHERE pk = record`).
    pub async fn connect_fk(
        &self,
        spec: &ForeignKeyMutationSpec,
        record_id: RelationIdValue,
        target_id: RelationIdValue,
    ) -> Result<u64, RelationQueryError> {
        let pool = crate::sqlx_pool()
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))?;
        let provider = detect_provider(&self.client().database_url);
        let table = quote_ident(provider, &spec.table);
        let pk = quote_ident(provider, &spec.record_pk_column);
        let fk = quote_ident(provider, &spec.fk_column);

        let mut qb = sqlx::QueryBuilder::<sqlx::Any>::new(format!("UPDATE {table} SET {fk} = "));
        bind_relation_id(&mut qb, &target_id);
        qb.push(" WHERE ");
        qb.push(pk);
        qb.push(" = ");
        bind_relation_id(&mut qb, &record_id);

        qb.build()
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(|e| RelationQueryError::Sql(e.to_string()))
    }

    /// Disconnect a record by setting a nullable foreign key to `NULL`.
    pub async fn disconnect_fk(
        &self,
        spec: &ForeignKeyMutationSpec,
        record_id: RelationIdValue,
    ) -> Result<u64, RelationQueryError> {
        if !spec.nullable_fk {
            return Err(RelationQueryError::DisconnectRequiresNullableFk);
        }

        let pool = crate::sqlx_pool()
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))?;
        let provider = detect_provider(&self.client().database_url);
        let table = quote_ident(provider, &spec.table);
        let pk = quote_ident(provider, &spec.record_pk_column);
        let fk = quote_ident(provider, &spec.fk_column);

        let mut qb = sqlx::QueryBuilder::<sqlx::Any>::new(format!(
            "UPDATE {table} SET {fk} = NULL WHERE {pk} = "
        ));
        bind_relation_id(&mut qb, &record_id);

        qb.build()
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(|e| RelationQueryError::Sql(e.to_string()))
    }

    /// Connect two records through explicit join table (`INSERT INTO join (A,B) VALUES (...)`).
    pub async fn connect_many_to_many(
        &self,
        spec: &JoinMutationSpec,
        left_id: RelationIdValue,
        right_id: RelationIdValue,
    ) -> Result<u64, RelationQueryError> {
        let pool = crate::sqlx_pool()
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))?;
        let provider = detect_provider(&self.client().database_url);
        let table = quote_ident(provider, &spec.join_table);
        let left = quote_ident(provider, &spec.left_column);
        let right = quote_ident(provider, &spec.right_column);

        let mut qb = sqlx::QueryBuilder::<sqlx::Any>::new(format!(
            "INSERT INTO {table} ({left}, {right}) VALUES ("
        ));
        bind_relation_id(&mut qb, &left_id);
        qb.push(", ");
        bind_relation_id(&mut qb, &right_id);
        qb.push(")");

        qb.build()
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(|e| RelationQueryError::Sql(e.to_string()))
    }

    /// Disconnect two records through explicit join table (`DELETE FROM join WHERE A=? AND B=?`).
    pub async fn disconnect_many_to_many(
        &self,
        spec: &JoinMutationSpec,
        left_id: RelationIdValue,
        right_id: RelationIdValue,
    ) -> Result<u64, RelationQueryError> {
        let pool = crate::sqlx_pool()
            .await
            .map_err(|e| RelationQueryError::Sql(e.to_string()))?;
        let provider = detect_provider(&self.client().database_url);
        let table = quote_ident(provider, &spec.join_table);
        let left = quote_ident(provider, &spec.left_column);
        let right = quote_ident(provider, &spec.right_column);

        let mut qb =
            sqlx::QueryBuilder::<sqlx::Any>::new(format!("DELETE FROM {table} WHERE {left} = "));
        bind_relation_id(&mut qb, &left_id);
        qb.push(" AND ");
        qb.push(right);
        qb.push(" = ");
        bind_relation_id(&mut qb, &right_id);

        qb.build()
            .execute(pool)
            .await
            .map(|r| r.rows_affected())
            .map_err(|e| RelationQueryError::Sql(e.to_string()))
    }
}
