//! Prisma-aligned **index / unique / PK** DDL helpers for apps that run raw SQL (e.g. [`crate::PrismaService::execute`]).
//!
//! This is **not** Prisma Migrate: there is no `schema.prisma` parser here. Use these builders to generate
//! strings that match what Prisma documents for each database, then execute them in migrations or bootstrap.
//!
//! | Prisma concept | API |
//! |----------------|-----|
//! | `@@index`, `@unique`, `map`, `sort`, `length` | [`IndexDefinition`], [`create_index_sql`] |
//! | `type: Hash \| Gin \| …` (PostgreSQL) | [`PgIndexMethod`], [`IndexDefinition::using`] |
//! | `ops:` (GIN / GiST / BRIN operator classes) | [`IndexColumn::ops`] |
//! | `where:` partial indexes | [`WherePredicate`], [`create_index_sql`] |
//! | `clustered:` (SQL Server) | [`IndexDefinition::clustered`], [`create_index_sql`] |
//! | `@@fulltext` (MySQL) | [`create_fulltext_index_sql`] |
//! | `@@id` / `@@unique` composite with `map` | [`create_unique_index_sql`], [`primary_key_constraint_sql`] |
//!
//! ## Not represented as generated SQL here
//!
//! - **MongoDB** `@@fulltext` and indexes are defined in Prisma’s Mongo provider; this crate targets SQL drivers.
//!   Use the official MongoDB driver or Prisma’s JS client for Mongo index DDL.
//! - **Prisma Migrate / `db push` / introspection** — use the Prisma CLI against `schema.prisma`; these helpers only build strings.
//! - **MySQL partial indexes** — not supported by MySQL; [`create_index_sql`] returns [`IndexDdlError::PartialIndexNotSupportedMySql`].
//! - **CockroachDB** only allows `USING BTREE` or `USING GIN` in this helper (aligned with Prisma’s Cockroach support).
//! - **PostgreSQL `CONCURRENTLY`** — emitted when [`IndexDefinition::concurrent`] is true; invalid inside a transaction block (same as PostgreSQL).
//! - **Operator classes** (full Prisma tables for GIN/GiST/BRIN) — pass any class name via [`IndexColumn::ops`]; validation is left to the database.

use crate::client::SortOrder;
use std::fmt::Write as _;

/// Target database (controls quoting, unsupported features, and `USING` / `FULLTEXT` syntax).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SqlDialect {
    PostgreSql,
    Sqlite,
    MySql,
    SqlServer,
    CockroachDb,
}

/// PostgreSQL index access method (`@@index(..., type: …)` in Prisma).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PgIndexMethod {
    BTree,
    Hash,
    Gist,
    Gin,
    SpGist,
    Brin,
}

impl PgIndexMethod {
    pub fn as_sql(self) -> &'static str {
        match self {
            PgIndexMethod::BTree => "BTREE",
            PgIndexMethod::Hash => "HASH",
            PgIndexMethod::Gist => "GIST",
            PgIndexMethod::Gin => "GIN",
            PgIndexMethod::SpGist => "SPGIST",
            PgIndexMethod::Brin => "BRIN",
        }
    }
}

/// One indexed column (Prisma field in `@@index([...])` / `@unique`).
#[derive(Debug, Clone, PartialEq)]
pub struct IndexColumn {
    pub name: String,
    pub sort: Option<SortOrder>,
    /// MySQL prefix length on `String` / `Bytes` (`length:` in Prisma).
    pub length: Option<u32>,
    /// PostgreSQL operator class name, e.g. `jsonb_path_ops` → rendered as `col jsonb_path_ops`.
    pub ops: Option<String>,
}

impl IndexColumn {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sort: None,
            length: None,
            ops: None,
        }
    }

    pub fn sort(mut self, s: SortOrder) -> Self {
        self.sort = Some(s);
        self
    }

    pub fn length(mut self, n: u32) -> Self {
        self.length = Some(n);
        self
    }

    pub fn ops(mut self, op: impl Into<String>) -> Self {
        self.ops = Some(op.into());
        self
    }
}

/// Predicate for partial / filtered indexes (`where:` in Prisma).
#[derive(Debug, Clone, PartialEq)]
pub enum WherePredicate {
    /// `where: raw("…")` — pass **trusted** SQL fragments only.
    Raw(String),
    /// Type-safe object style (`where: { published: true, deletedAt: null }`).
    Fields(Vec<(String, WhereValue)>),
}

/// Values allowed in [`WherePredicate::Fields`] (Prisma object `where` for indexes).
#[derive(Debug, Clone, PartialEq)]
pub enum WhereValue {
    Bool(bool),
    Str(String),
    Int(i64),
    Float(f64),
    Null,
    Not(Box<WhereValue>),
}

impl WherePredicate {
    pub fn raw(s: impl Into<String>) -> Self {
        Self::Raw(s.into())
    }
}

/// Index or unique index definition (`@@index` / `@unique` / `@@unique` with optional `map`).
#[derive(Debug, Clone, PartialEq)]
pub struct IndexDefinition {
    pub table: String,
    pub columns: Vec<IndexColumn>,
    pub unique: bool,
    /// Physical name in DB (`map:`); if `None`, `name` or a default is used.
    pub map: Option<String>,
    /// Logical name (`name:` in Prisma); falls back to `{table}_{col}_idx` style when unset.
    pub name: Option<String>,
    pub r#where: Option<WherePredicate>,
    /// PostgreSQL / CockroachDB `USING` (ignored on other dialects unless error).
    pub using: Option<PgIndexMethod>,
    /// SQL Server `CLUSTERED` / `NONCLUSTERED` on indexes.
    pub clustered: Option<bool>,
    /// PostgreSQL `CREATE INDEX CONCURRENTLY` (migration-only; not valid in all contexts).
    pub concurrent: bool,
}

impl IndexDefinition {
    pub fn on(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            columns: Vec::new(),
            unique: false,
            map: None,
            name: None,
            r#where: None,
            using: None,
            clustered: None,
            concurrent: false,
        }
    }

    pub fn column(mut self, c: IndexColumn) -> Self {
        self.columns.push(c);
        self
    }

    pub fn unique(mut self, u: bool) -> Self {
        self.unique = u;
        self
    }

    pub fn map(mut self, m: impl Into<String>) -> Self {
        self.map = Some(m.into());
        self
    }

    pub fn name(mut self, n: impl Into<String>) -> Self {
        self.name = Some(n.into());
        self
    }

    pub fn r#where(mut self, w: WherePredicate) -> Self {
        self.r#where = Some(w);
        self
    }

    pub fn using(mut self, m: PgIndexMethod) -> Self {
        self.using = Some(m);
        self
    }

    pub fn clustered(mut self, c: bool) -> Self {
        self.clustered = Some(c);
        self
    }

    pub fn concurrent(mut self, c: bool) -> Self {
        self.concurrent = c;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexDdlError {
    #[error("partial index (where) is not supported on MySQL")]
    PartialIndexNotSupportedMySql,
    #[error("index type {0:?} is only supported on PostgreSQL and CockroachDB")]
    IndexTypeWrongDialect(PgIndexMethod),
    #[error("FULLTEXT indexes are only supported on MySQL in this helper")]
    FullTextWrongDialect,
    #[error("operator class (ops) requires PostgreSQL-family dialect")]
    OpsWrongDialect,
    #[error("clustered option on @@index is only supported on SQL Server")]
    ClusteredIndexWrongDialect,
    #[error("primary_key_constraint_sql is only defined for SQL Server in this helper")]
    PrimaryKeyConstraintWrongDialect,
    #[error("no columns in index definition")]
    NoColumns,
}

fn quote_ident(dialect: SqlDialect, id: &str) -> String {
    match dialect {
        SqlDialect::PostgreSql | SqlDialect::Sqlite | SqlDialect::CockroachDb => {
            format!("\"{}\"", id.replace('"', "\"\""))
        }
        SqlDialect::MySql => {
            format!("`{}`", id.replace('`', "``"))
        }
        SqlDialect::SqlServer => {
            format!("[{}]", id.replace(']', "]]"))
        }
    }
}

fn render_column_list(
    dialect: SqlDialect,
    columns: &[IndexColumn],
) -> Result<String, IndexDdlError> {
    let mut out = String::new();
    for (i, c) in columns.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        let q = quote_ident(dialect, &c.name);
        match dialect {
            SqlDialect::MySql => {
                write!(&mut out, "{}", q).unwrap();
                if let Some(len) = c.length {
                    write!(&mut out, "({len})").unwrap();
                }
            }
            SqlDialect::PostgreSql | SqlDialect::CockroachDb => {
                write!(&mut out, "{}", q).unwrap();
                if let Some(len) = c.length {
                    write!(&mut out, "({len})").unwrap();
                }
                if let Some(ref ops) = c.ops {
                    write!(&mut out, " {ops}").unwrap();
                }
            }
            SqlDialect::Sqlite => {
                write!(&mut out, "{}", q).unwrap();
            }
            SqlDialect::SqlServer => {
                write!(&mut out, "{}", q).unwrap();
                // Prisma `length:` on indexes is MySQL-oriented; ignore on SQL Server.
            }
        }
        if let Some(sort) = c.sort {
            write!(&mut out, " {}", sort.as_sql()).unwrap();
        }
    }
    Ok(out)
}

/// Render `WHERE` for partial indexes (PostgreSQL, SQLite, SQL Server; not MySQL).
fn render_where(dialect: SqlDialect, w: &WherePredicate) -> String {
    match w {
        WherePredicate::Raw(s) => s.clone(),
        WherePredicate::Fields(pairs) => {
            let mut parts = Vec::new();
            for (field, val) in pairs {
                let qf = quote_ident(dialect, field);
                let frag = match val {
                    WhereValue::Bool(b) => match dialect {
                        SqlDialect::SqlServer => format!("{qf} = {}", if *b { 1 } else { 0 }),
                        _ => format!("{qf} = {b}"),
                    },
                    WhereValue::Str(s) => format!("{qf} = '{}'", s.replace('\'', "''")),
                    WhereValue::Int(n) => format!("{qf} = {n}"),
                    WhereValue::Float(f) => format!("{qf} = {f}"),
                    WhereValue::Null => format!("{qf} IS NULL"),
                    WhereValue::Not(inner) => match inner.as_ref() {
                        WhereValue::Null => format!("{qf} IS NOT NULL"),
                        WhereValue::Str(s) => format!("{qf} <> '{}'", s.replace('\'', "''")),
                        WhereValue::Bool(b) => match dialect {
                            SqlDialect::SqlServer => format!("{qf} <> {}", if *b { 1 } else { 0 }),
                            _ => format!("{qf} <> {b}"),
                        },
                        WhereValue::Int(n) => format!("{qf} <> {n}"),
                        WhereValue::Float(f) => format!("{qf} <> {f}"),
                        WhereValue::Not(_) => format!(
                            "NOT ({})",
                            render_where(
                                dialect,
                                &WherePredicate::Fields(vec![(field.clone(), *inner.clone())])
                            )
                        ),
                    },
                };
                parts.push(frag);
            }
            parts.join(" AND ")
        }
    }
}

fn default_index_name(table: &str, columns: &[IndexColumn], unique: bool) -> String {
    let cols = if columns.is_empty() {
        "idx".to_string()
    } else {
        columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
            .join("_")
    };
    if unique {
        format!("{}_{}_key", table, cols)
    } else {
        format!("{}_{}_idx", table, cols)
    }
}

/// `CREATE [UNIQUE] INDEX ...` (and variants) matching Prisma’s migration output as closely as dialect allows.
pub fn create_index_sql(
    dialect: SqlDialect,
    def: &IndexDefinition,
) -> Result<String, IndexDdlError> {
    if def.columns.is_empty() {
        return Err(IndexDdlError::NoColumns);
    }
    if def.r#where.is_some() && dialect == SqlDialect::MySql {
        return Err(IndexDdlError::PartialIndexNotSupportedMySql);
    }
    if let Some(u) = def.using {
        if !matches!(dialect, SqlDialect::PostgreSql | SqlDialect::CockroachDb) {
            return Err(IndexDdlError::IndexTypeWrongDialect(u));
        }
        if dialect == SqlDialect::CockroachDb
            && !matches!(u, PgIndexMethod::BTree | PgIndexMethod::Gin)
        {
            return Err(IndexDdlError::IndexTypeWrongDialect(u));
        }
    }
    if def.columns.iter().any(|c| c.ops.is_some())
        && !matches!(dialect, SqlDialect::PostgreSql | SqlDialect::CockroachDb)
    {
        return Err(IndexDdlError::OpsWrongDialect);
    }
    if def.clustered.is_some() && dialect != SqlDialect::SqlServer {
        return Err(IndexDdlError::ClusteredIndexWrongDialect);
    }

    let idx_name = def
        .map
        .clone()
        .or_else(|| def.name.clone())
        .unwrap_or_else(|| default_index_name(&def.table, &def.columns, def.unique));

    let table_q = quote_ident(dialect, &def.table);
    let col_list = render_column_list(dialect, &def.columns)?;

    let mut sql = String::new();

    match dialect {
        SqlDialect::PostgreSql => {
            // `CREATE [UNIQUE] INDEX [CONCURRENTLY] name ON ...` — see PostgreSQL `CREATE INDEX`.
            write!(&mut sql, "CREATE ").unwrap();
            if def.unique {
                write!(&mut sql, "UNIQUE ").unwrap();
            }
            write!(&mut sql, "INDEX ").unwrap();
            if def.concurrent {
                write!(&mut sql, "CONCURRENTLY ").unwrap();
            }
            write!(
                &mut sql,
                "\"{}\" ON {} ",
                idx_name.replace('"', "\"\""),
                table_q
            )
            .unwrap();
            if let Some(m) = def.using {
                write!(&mut sql, "USING {} ", m.as_sql()).unwrap();
            }
            write!(&mut sql, "({col_list})").unwrap();
            if let Some(ref w) = def.r#where {
                write!(&mut sql, " WHERE ({})", render_where(dialect, w)).unwrap();
            }
        }
        SqlDialect::CockroachDb => {
            write!(
                &mut sql,
                "CREATE {}INDEX \"{}\" ON {} ",
                if def.unique { "UNIQUE " } else { "" },
                idx_name.replace('"', "\"\""),
                table_q
            )
            .unwrap();
            if let Some(m) = def.using {
                write!(&mut sql, "USING {} ", m.as_sql()).unwrap();
            }
            write!(&mut sql, "({col_list})").unwrap();
            if let Some(ref w) = def.r#where {
                write!(&mut sql, " WHERE ({})", render_where(dialect, w)).unwrap();
            }
        }
        SqlDialect::Sqlite => {
            write!(
                &mut sql,
                "CREATE {}INDEX \"{}\" ON {} ({col_list})",
                if def.unique { "UNIQUE " } else { "" },
                idx_name.replace('"', "\"\""),
                table_q
            )
            .unwrap();
            if let Some(ref w) = def.r#where {
                write!(&mut sql, " WHERE {}", render_where(dialect, w)).unwrap();
            }
        }
        SqlDialect::MySql => {
            write!(
                &mut sql,
                "CREATE {}INDEX `{}` ON {} ({col_list})",
                if def.unique { "UNIQUE " } else { "" },
                idx_name.replace('`', "``"),
                table_q
            )
            .unwrap();
        }
        SqlDialect::SqlServer => {
            let clustered = def.clustered.unwrap_or(false);
            write!(&mut sql, "CREATE ").unwrap();
            if def.unique {
                write!(&mut sql, "UNIQUE ").unwrap();
            }
            write!(
                &mut sql,
                "{}INDEX [{}] ON {} ({})",
                if clustered {
                    "CLUSTERED "
                } else {
                    "NONCLUSTERED "
                },
                idx_name.replace(']', "]]"),
                table_q,
                col_list
            )
            .unwrap();
            if let Some(ref w) = def.r#where {
                write!(&mut sql, " WHERE {}", render_where(dialect, w)).unwrap();
            }
        }
    }

    Ok(sql)
}

/// Convenience: unique index / constraint name style used by Prisma (`@unique`, `@@unique`).
pub fn create_unique_index_sql(
    dialect: SqlDialect,
    def: &IndexDefinition,
) -> Result<String, IndexDdlError> {
    let mut d = def.clone();
    d.unique = true;
    create_index_sql(dialect, &d)
}

/// MySQL `FULLTEXT` index (`@@fulltext([...])` with Prisma preview feature).
pub fn create_fulltext_index_sql(
    dialect: SqlDialect,
    table: &str,
    columns: &[&str],
    map: Option<&str>,
) -> Result<String, IndexDdlError> {
    if dialect != SqlDialect::MySql {
        return Err(IndexDdlError::FullTextWrongDialect);
    }
    if columns.is_empty() {
        return Err(IndexDdlError::NoColumns);
    }
    let name = map.unwrap_or("fulltext_idx");
    let t = quote_ident(dialect, table);
    let cols: String = columns
        .iter()
        .map(|c| quote_ident(dialect, c))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "CREATE FULLTEXT INDEX `{}` ON {} ({})",
        name.replace('`', "``"),
        t,
        cols
    ))
}

/// `ALTER TABLE … ADD CONSTRAINT … PRIMARY KEY` (SQL Server: `clustered: false` → `NONCLUSTERED`).
pub fn primary_key_constraint_sql(
    dialect: SqlDialect,
    table: &str,
    columns: &[&str],
    map: Option<&str>,
    clustered: bool,
) -> Result<String, IndexDdlError> {
    if dialect != SqlDialect::SqlServer {
        return Err(IndexDdlError::PrimaryKeyConstraintWrongDialect);
    }
    if columns.is_empty() {
        return Err(IndexDdlError::NoColumns);
    }
    let cn = map.unwrap_or("PK__constraint");
    let t = quote_ident(dialect, table);
    let cols: String = columns
        .iter()
        .map(|c| quote_ident(dialect, c))
        .collect::<Vec<_>>()
        .join(", ");
    let cl = if clustered {
        "CLUSTERED"
    } else {
        "NONCLUSTERED"
    };
    Ok(format!(
        "ALTER TABLE {} ADD CONSTRAINT [{}] PRIMARY KEY {} ({})",
        t,
        cn.replace(']', "]]"),
        cl,
        cols
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::SortOrder;

    #[test]
    fn pg_hash_index() {
        let def = IndexDefinition::on("Example")
            .column(IndexColumn::new("value"))
            .using(PgIndexMethod::Hash)
            .map("Example_value_idx");
        let sql = create_index_sql(SqlDialect::PostgreSql, &def).unwrap();
        assert!(sql.contains("USING HASH"));
        assert!(sql.contains("\"value\""));
    }

    #[test]
    fn pg_gin_jsonb_path_ops() {
        let def = IndexDefinition::on("Example")
            .column(IndexColumn::new("value").ops("jsonb_path_ops"))
            .using(PgIndexMethod::Gin)
            .map("Example_value_idx");
        let sql = create_index_sql(SqlDialect::PostgreSql, &def).unwrap();
        assert!(sql.contains("USING GIN"));
        assert!(sql.contains("jsonb_path_ops"));
    }

    #[test]
    fn composite_sort_mysql_style_pg() {
        let def = IndexDefinition::on("CompoundUnique")
            .column(IndexColumn::new("unique_1").sort(SortOrder::Desc))
            .column(IndexColumn::new("unique_2"))
            .map("compound_uq");
        let sql = create_index_sql(SqlDialect::PostgreSql, &def).unwrap();
        assert!(sql.contains("DESC"));
        assert!(sql.contains("\"unique_1\""));
    }

    #[test]
    fn pg_partial_raw() {
        let def = IndexDefinition::on("User")
            .column(IndexColumn::new("email"))
            .map("User_email_idx")
            .r#where(WherePredicate::raw(r#"\"deletedAt\" IS NULL"#.to_string()));
        let sql = create_index_sql(SqlDialect::PostgreSql, &def).unwrap();
        assert!(sql.contains("WHERE"));
    }

    #[test]
    fn pg_partial_object() {
        let def = IndexDefinition::on("Post")
            .column(IndexColumn::new("title"))
            .r#where(WherePredicate::Fields(vec![(
                "published".into(),
                WhereValue::Bool(true),
            )]));
        let sql = create_index_sql(SqlDialect::PostgreSql, &def).unwrap();
        assert!(sql.contains("\"published\" = true"));
    }

    #[test]
    fn mysql_prefix_length() {
        let def = IndexDefinition::on("Id")
            .unique(true)
            .column(IndexColumn::new("id").length(100))
            .map("Id_pkey");
        let sql = create_unique_index_sql(SqlDialect::MySql, &def).unwrap();
        assert!(sql.contains("`id`(100)"));
    }

    #[test]
    fn mysql_fulltext() {
        let sql = create_fulltext_index_sql(SqlDialect::MySql, "Post", &["title", "content"], None)
            .unwrap();
        assert!(sql.contains("FULLTEXT"));
        assert!(sql.contains("`title`"));
    }

    #[test]
    fn mssql_clustered_index() {
        let def = IndexDefinition::on("Example")
            .column(IndexColumn::new("value"))
            .map("idx_val")
            .clustered(true);
        let sql = create_index_sql(SqlDialect::SqlServer, &def).unwrap();
        assert!(sql.contains("CLUSTERED"));
    }

    #[test]
    fn sqlite_partial() {
        let def = IndexDefinition::on("User")
            .column(IndexColumn::new("email"))
            .r#where(WherePredicate::raw("deletedAt IS NULL"));
        let sql = create_index_sql(SqlDialect::Sqlite, &def).unwrap();
        assert!(sql.contains("WHERE"));
    }
}
