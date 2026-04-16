// Internal helpers for `prisma_model!` (must stay in this crate for `$crate::` paths).

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_create_input {
    ($Model:ident; id: i64, $( $f:ident : $t:ty ),+ $(,)?) => {
        $crate::paste::paste! {
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
            pub struct [< $Model CreateInput >] {
                $( pub $f : $t ),+
            }
        }
    };
    ($Model:ident; $( $f:ident : $t:ty ),+ $(,)?) => {
        $crate::paste::paste! {
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
            pub struct [< $Model CreateInput >] {
                $( pub $f : $t ),+
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_insert_returning {
    (
        $table:literal;
        $Model:ident;
        $pool:ident;
        $data:ident;
        id: i64,
        $( $f:ident : $t:ty ),+
    ) => {{
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )+
        qb.push(") VALUES (");
        let mut sep2 = qb.separated(", ");
        $(
            sep2.push_bind($data.$f.clone());
        )+
        qb.push(") RETURNING *");
        let row = qb
            .build_query_as::<$Model>()
            .fetch_one($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(row)
    }};
    (
        $table:literal;
        $Model:ident;
        $pool:ident;
        $data:ident;
        $( $f:ident : $t:ty ),+
    ) => {{
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )*
        qb.push(") VALUES (");
        let mut sep2 = qb.separated(", ");
        $(
            sep2.push_bind($data.$f.clone());
        )*
        qb.push(") RETURNING *");
        let row = qb
            .build_query_as::<$Model>()
            .fetch_one($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(row)
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_insert_many {
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        id: i64,
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {{
        if $rows.is_empty() {
            return std::result::Result::Ok(0u64);
        }
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )+
        qb.push(") ");
        qb.push_values($rows.iter(), |mut b, row| {
            $(
                b.push_bind(row.$f.clone());
            )+
        });
        let res = qb
            .build()
            .execute($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(res.rows_affected())
    }};
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {{
        if $rows.is_empty() {
            return std::result::Result::Ok(0u64);
        }
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )+
        qb.push(") ");
        qb.push_values($rows.iter(), |mut b, row| {
            $(
                b.push_bind(row.$f.clone());
            )+
        });
        let res = qb
            .build()
            .execute($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(res.rows_affected())
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_insert_many_dispatch {
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        id: i64,
        $($f:ident : $t:ty),+ $(,)?
    ) => {
        $crate::__prisma_insert_many!($table; $pool; $rows; id: i64, $($f : $t),+)
    };
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        $($f:ident : $t:ty),+ $(,)?
    ) => {
        $crate::__prisma_insert_many!($table; $pool; $rows; $($f : $t),+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_insert_many_with_options {
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        id: i64,
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {{
        if $rows.is_empty() {
            return std::result::Result::Ok(0u64);
        }
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )+
        qb.push(") ");
        qb.push_values($rows.iter(), |mut b, row| {
            $(
                b.push_bind(row.$f.clone());
            )+
        });
        if $skip_duplicates {
            // Works on PostgreSQL and modern SQLite. Other dialects may reject this syntax.
            qb.push(" ON CONFLICT DO NOTHING");
        }
        let res = qb
            .build()
            .execute($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(res.rows_affected())
    }};
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {{
        if $rows.is_empty() {
            return std::result::Result::Ok(0u64);
        }
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )+
        qb.push(") ");
        qb.push_values($rows.iter(), |mut b, row| {
            $(
                b.push_bind(row.$f.clone());
            )+
        });
        if $skip_duplicates {
            // Works on PostgreSQL and modern SQLite. Other dialects may reject this syntax.
            qb.push(" ON CONFLICT DO NOTHING");
        }
        let res = qb
            .build()
            .execute($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(res.rows_affected())
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_insert_many_with_options_dispatch {
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        id: i64,
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {
        $crate::__prisma_insert_many_with_options!($table; $pool; $rows; $skip_duplicates; id: i64, $( $f : $t ),+)
    };
    (
        $table:literal;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {
        $crate::__prisma_insert_many_with_options!($table; $pool; $rows; $skip_duplicates; $( $f : $t ),+)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_insert_many_returning {
    (
        $table:literal;
        $Model:ident;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        id: i64,
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {{
        if $rows.is_empty() {
            return std::result::Result::Ok(::std::vec::Vec::<$Model>::new());
        }
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )+
        qb.push(") ");
        qb.push_values($rows.iter(), |mut b, row| {
            $(
                b.push_bind(row.$f.clone());
            )+
        });
        if $skip_duplicates {
            qb.push(" ON CONFLICT DO NOTHING");
        }
        qb.push(" RETURNING *");
        let out = qb
            .build_query_as::<$Model>()
            .fetch_all($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(out)
    }};
    (
        $table:literal;
        $Model:ident;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {{
        if $rows.is_empty() {
            return std::result::Result::Ok(::std::vec::Vec::<$Model>::new());
        }
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($f), "\""));
        )+
        qb.push(") ");
        qb.push_values($rows.iter(), |mut b, row| {
            $(
                b.push_bind(row.$f.clone());
            )+
        });
        if $skip_duplicates {
            qb.push(" ON CONFLICT DO NOTHING");
        }
        qb.push(" RETURNING *");
        let out = qb
            .build_query_as::<$Model>()
            .fetch_all($pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(out)
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_insert_many_returning_dispatch {
    (
        $table:literal;
        $Model:ident;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        id: i64,
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {
        $crate::__prisma_insert_many_returning!(
            $table;
            $Model;
            $pool;
            $rows;
            $skip_duplicates;
            id: i64,
            $( $f : $t ),+
        )
    };
    (
        $table:literal;
        $Model:ident;
        $pool:ident;
        $rows:ident;
        $skip_duplicates:expr;
        $( $f:ident : $t:ty ),+ $(,)?
    ) => {
        $crate::__prisma_insert_many_returning!(
            $table;
            $Model;
            $pool;
            $rows;
            $skip_duplicates;
            $( $f : $t ),+
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_upsert_dispatch {
    (
        $Model:ident,
        $table:literal,
        $self:ident,
        $w:ident,
        $create:ident,
        $update:ident;
        id: String
    ) => {
        $crate::__prisma_upsert_impl!(
            conflict_id,
            $Model,
            $table,
            $self,
            $w,
            $create,
            $update,
            id: String
        )
    };
    (
        $Model:ident,
        $table:literal,
        $self:ident,
        $w:ident,
        $create:ident,
        $update:ident;
        id: String,
        $($f:ident : $t:ty),+ $(,)?
    ) => {
        $crate::__prisma_upsert_impl!(
            conflict_id,
            $Model,
            $table,
            $self,
            $w,
            $create,
            $update,
            id: String,
            $($f : $t),+
        )
    };
    (
        $Model:ident,
        $table:literal,
        $self:ident,
        $w:ident,
        $create:ident,
        $update:ident;
        $($f:ident : $t:ty),+ $(,)?
    ) => {
        $crate::__prisma_upsert_impl!(autoinc, $Model, $table, $self, $w, $create, $update)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_upsert_impl {
    (
        autoinc,
        $Model:ident,
        $table:literal,
        $self:ident,
        $w:ident,
        $create:ident,
        $update:ident
    ) => {
        $crate::paste::paste! {{
            if [< Prisma $Model Repository >]::find_unique($self, $w.clone())
                .await?
                .is_some()
            {
                [< Prisma $Model Repository >]::update($self, $w, $update).await
            } else {
                [< Prisma $Model Repository >]::create($self, $create).await
            }
        }}
    };
    (
        conflict_id,
        $Model:ident,
        $table:literal,
        $self:ident,
        $w:ident,
        $create:ident,
        $update:ident,
        $( $col:ident : $ct:ty ),+ $(,)?
    ) => {{
        let pool = $crate::sqlx_pool().await?;
        let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("INSERT INTO \"", $table, "\" ("));
        let mut sep = qb.separated(", ");
        $(
            sep.push(concat!("\"", stringify!($col), "\""));
        )+
        qb.push(") VALUES (");
        let mut sep2 = qb.separated(", ");
        $(
            sep2.push_bind($create.$col.clone());
        )+
        qb.push(") ON CONFLICT (\"id\") DO UPDATE SET ");
        let mut any_set = false;
        $(
            if let ::std::option::Option::Some(_) = $update.$col {
                if any_set {
                    qb.push(", ");
                }
                any_set = true;
                qb.push(concat!("\"", stringify!($col), "\" = EXCLUDED.\"", stringify!($col), "\""));
            }
        )+
        if !any_set {
            qb.push("\"id\" = EXCLUDED.\"id\"");
        }
        qb.push(" RETURNING *");
        let row = qb
            .build_query_as::<$Model>()
            .fetch_one(pool)
            .await
            .map_err($crate::PrismaError::from_sqlx)?;
        std::result::Result::Ok(row)
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_to_f64_opt {
    ($row:expr, $field:ident, i64) => {
        Some($row.$field as f64)
    };
    ($row:expr, $field:ident, Option<i64>) => {
        $row.$field.map(|v| v as f64)
    };
    ($row:expr, $field:ident, ::std::option::Option<i64>) => {
        $row.$field.map(|v| v as f64)
    };
    ($row:expr, $field:ident, $other:ty) => {
        Option::<f64>::None
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __prisma_non_null_value {
    ($row:expr, $field:ident, Option<$inner:ty>) => {
        $row.$field.is_some()
    };
    ($row:expr, $field:ident, ::std::option::Option<$inner:ty>) => {
        $row.$field.is_some()
    };
    ($row:expr, $field:ident, $other:ty) => {
        true
    };
}

/// Declares a Prisma-style model: struct + `where` helpers + [`crate::client::ModelRepository`] methods
/// + extension trait on [`std::sync::Arc<crate::PrismaService>`] (e.g. `prisma.user().find_many(...)`).
///
/// **Requires** the `sqlx` feature on `nestrs-prisma`.
///
/// Supported field types today: `i64`, `String`, `bool`, `Option<…>` of those, `chrono::DateTime<Utc>` and
/// `uuid::Uuid` when crate features `chrono` / `uuid` are enabled.
///
/// Generated models execute against the configured concrete SQLx backend (`Sqlite` by default, or
/// `sqlx-postgres` / `sqlx-mysql` feature selections).
///
/// `Where` filters are **Eq / Ne** today; additional operators are defined in `macros_where.rs` for a future
/// non-`paste!` enum splice or proc-macro generator.
#[macro_export]
macro_rules! prisma_model {
    (
        $Model:ident => $table:literal,
        {
            $( $field:ident : $ftype:ty ),* $(,)?
        }
    ) => {
        #[cfg(not(feature = "sqlx"))]
        ::core::compile_error!("prisma_model! requires the `sqlx` feature on nestrs-prisma");

        #[cfg(feature = "sqlx")]
        $crate::paste::paste! {
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize, $crate::sqlx::FromRow)]
            pub struct $Model {
                $( pub $field : $ftype ),*
            }
        }

        #[cfg(feature = "sqlx")]
        $crate::__prisma_where_types!($Model; $( ($field, $ftype) ),*);

        #[cfg(feature = "sqlx")]
        $crate::paste::paste! {

            #[derive(Debug, Clone, Default)]
            pub struct [< $Model Update >] {
                $( pub $field : std::option::Option<$ftype> ),*
            }

            impl [< $Model Update >] {
                fn __push_set(
                    &self,
                    qb: &mut $crate::sqlx::QueryBuilder<'_, $crate::SqlxDb>,
                ) -> std::result::Result<(), $crate::PrismaError> {
                    let mut any = false;
                    $(
                        if let std::option::Option::Some(ref v) = self.$field {
                            if any {
                                qb.push(", ");
                            }
                            any = true;
                            qb.push(concat!("\"", stringify!($field), "\" = "));
                            qb.push_bind(v.clone());
                        }
                    )*
                    if !any {
                        return std::result::Result::Err($crate::PrismaError::other("update: no fields set"));
                    }
                    Ok(())
                }
            }

            $crate::__prisma_create_input!($Model; $( $field : $ftype ),*);

            #[derive(Debug, Clone, Default)]
            pub struct [< $Model FindManyOptions >] {
                pub r#where: [< $Model Where >],
                pub order_by: std::option::Option<Vec<[< $Model OrderBy >]>>,
                pub take: std::option::Option<i64>,
                pub skip: std::option::Option<i64>,
                pub distinct: std::option::Option<Vec<[< $Model ScalarField >]>>,
            }

            /// Prisma-style `createMany` options (currently `skip_duplicates`).
            #[derive(Debug, Clone, Copy, Default)]
            pub struct [< $Model CreateManyOptions >] {
                pub skip_duplicates: bool,
            }

            /// Scalar fields available for `distinct`, aggregate selection and `group_by.by`.
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
            pub enum [< $Model ScalarField >] {
                $( [< $field:camel >], )*
            }

            impl [< $Model ScalarField >] {
                fn as_name(self) -> &'static str {
                    match self {
                        $( Self::[< $field:camel >] => stringify!($field), )*
                    }
                }
            }

            /// Which aggregations should be computed.
            #[derive(Debug, Clone, Default)]
            pub struct [< $Model AggregateSelection >] {
                pub count_all: bool,
                pub count: Vec<[< $Model ScalarField >]>,
                pub avg: Vec<[< $Model ScalarField >]>,
                pub sum: Vec<[< $Model ScalarField >]>,
                pub min: Vec<[< $Model ScalarField >]>,
                pub max: Vec<[< $Model ScalarField >]>,
            }

            #[derive(Debug, Clone, Default)]
            pub struct [< $Model AggregateOptions >] {
                pub r#where: [< $Model Where >],
                pub order_by: std::option::Option<Vec<[< $Model OrderBy >]>>,
                pub take: std::option::Option<i64>,
                pub skip: std::option::Option<i64>,
            }

            #[derive(Debug, Clone, Default)]
            pub struct [< $Model AggregateResult >] {
                pub count: std::collections::BTreeMap<String, i64>,
                pub avg: std::collections::BTreeMap<String, std::option::Option<f64>>,
                pub sum: std::collections::BTreeMap<String, std::option::Option<f64>>,
                pub min: std::collections::BTreeMap<String, std::option::Option<String>>,
                pub max: std::collections::BTreeMap<String, std::option::Option<String>>,
            }

            #[derive(Debug, Clone, Copy)]
            pub enum [< $Model HavingOp >] {
                Eq,
                Ne,
                Gt,
                Gte,
                Lt,
                Lte,
            }

            #[derive(Debug, Clone, Copy)]
            pub enum [< $Model AggregateMetric >] {
                Count,
                Avg,
                Sum,
                Min,
                Max,
            }

            #[derive(Debug, Clone)]
            pub struct [< $Model HavingCondition >] {
                pub field: [< $Model ScalarField >],
                pub metric: [< $Model AggregateMetric >],
                pub op: [< $Model HavingOp >],
                pub value: f64,
            }

            #[derive(Debug, Clone, Default)]
            pub struct [< $Model GroupByOptions >] {
                pub by: Vec<[< $Model ScalarField >]>,
                pub r#where: [< $Model Where >],
                pub order_by: std::option::Option<Vec<[< $Model OrderBy >]>>,
                pub take: std::option::Option<i64>,
                pub skip: std::option::Option<i64>,
                pub having: Vec<[< $Model HavingCondition >]>,
            }

            #[derive(Debug, Clone, Default)]
            pub struct [< $Model GroupByRow >] {
                pub by: std::collections::BTreeMap<String, String>,
                pub aggregates: [< $Model AggregateResult >],
            }

            #[derive(Debug, Clone, Copy)]
            pub enum [< $Model OrderBy >] {
                $( [< $field:camel >]($crate::client::SortOrder), )*
            }

            impl [< $Model OrderBy >] {
                fn __push_order(&self, qb: &mut $crate::sqlx::QueryBuilder<'_, $crate::SqlxDb>) {
                    match self {
                        $(
                            Self::[< $field:camel >](ord) => {
                                qb.push(concat!("\"", stringify!($field), "\""));
                                qb.push(" ");
                                qb.push(ord.as_sql());
                            }
                        )*
                    }
                }
            }

            fn [< __prisma_ $Model:snake _field_string >](row: &$Model, field: [< $Model ScalarField >]) -> String {
                match field {
                    $( [< $Model ScalarField >]::[< $field:camel >] => format!("{:?}", row.$field), )*
                }
            }

            fn [< __prisma_ $Model:snake _distinct_key >](
                row: &$Model,
                fields: &[[< $Model ScalarField >]],
            ) -> String {
                let mut key = String::new();
                for f in fields {
                    key.push_str(f.as_name());
                    key.push('=');
                    key.push_str(&[< __prisma_ $Model:snake _field_string >](row, *f));
                    key.push('|');
                }
                key
            }

            fn [< __prisma_ $Model:snake _compute_aggregate >](
                rows: &[$Model],
                sel: &[< $Model AggregateSelection >],
            ) -> [< $Model AggregateResult >] {
                let mut out = [< $Model AggregateResult >]::default();

                if sel.count_all {
                    out.count.insert("_all".to_string(), rows.len() as i64);
                }

                for field in &sel.count {
                    let mut c: i64 = 0;
                    for row in rows {
                        match field {
                            $(
                                [< $Model ScalarField >]::[< $field:camel >] => {
                                    if $crate::__prisma_non_null_value!(row, $field, $ftype) {
                                        c += 1;
                                    }
                                }
                            )*
                        }
                    }
                    out.count.insert(field.as_name().to_string(), c);
                }

                for field in &sel.avg {
                    let mut total = 0.0_f64;
                    let mut n = 0_u64;
                    for row in rows {
                        match field {
                            $(
                                [< $Model ScalarField >]::[< $field:camel >] => {
                                    if let Some(v) = $crate::__prisma_to_f64_opt!(row, $field, $ftype) {
                                        total += v;
                                        n += 1;
                                    }
                                }
                            )*
                        }
                    }
                    out.avg.insert(
                        field.as_name().to_string(),
                        if n == 0 { None } else { Some(total / n as f64) },
                    );
                }

                for field in &sel.sum {
                    let mut total = 0.0_f64;
                    let mut n = 0_u64;
                    for row in rows {
                        match field {
                            $(
                                [< $Model ScalarField >]::[< $field:camel >] => {
                                    if let Some(v) = $crate::__prisma_to_f64_opt!(row, $field, $ftype) {
                                        total += v;
                                        n += 1;
                                    }
                                }
                            )*
                        }
                    }
                    out.sum.insert(
                        field.as_name().to_string(),
                        if n == 0 { None } else { Some(total) },
                    );
                }

                for field in &sel.min {
                    let mut min_num: Option<f64> = None;
                    let mut min_text: Option<String> = None;
                    let mut saw_num = false;
                    for row in rows {
                        match field {
                            $(
                                [< $Model ScalarField >]::[< $field:camel >] => {
                                    if let Some(v) = $crate::__prisma_to_f64_opt!(row, $field, $ftype) {
                                        saw_num = true;
                                        min_num = Some(match min_num {
                                            Some(curr) => curr.min(v),
                                            None => v,
                                        });
                                    } else {
                                        let s = [< __prisma_ $Model:snake _field_string >](row, *field);
                                        min_text = Some(match min_text {
                                            Some(ref curr) if curr <= &s => curr.clone(),
                                            _ => s,
                                        });
                                    }
                                }
                            )*
                        }
                    }
                    out.min.insert(
                        field.as_name().to_string(),
                        if saw_num {
                            min_num.map(|v| v.to_string())
                        } else {
                            min_text
                        },
                    );
                }

                for field in &sel.max {
                    let mut max_num: Option<f64> = None;
                    let mut max_text: Option<String> = None;
                    let mut saw_num = false;
                    for row in rows {
                        match field {
                            $(
                                [< $Model ScalarField >]::[< $field:camel >] => {
                                    if let Some(v) = $crate::__prisma_to_f64_opt!(row, $field, $ftype) {
                                        saw_num = true;
                                        max_num = Some(match max_num {
                                            Some(curr) => curr.max(v),
                                            None => v,
                                        });
                                    } else {
                                        let s = [< __prisma_ $Model:snake _field_string >](row, *field);
                                        max_text = Some(match max_text {
                                            Some(ref curr) if curr >= &s => curr.clone(),
                                            _ => s,
                                        });
                                    }
                                }
                            )*
                        }
                    }
                    out.max.insert(
                        field.as_name().to_string(),
                        if saw_num {
                            max_num.map(|v| v.to_string())
                        } else {
                            max_text
                        },
                    );
                }

                out
            }

            pub mod [< $Model:snake >] {
                use super::[< $Model Where >];
                use super::[< $Model OrderBy >];
                use super::[< $Model Update >];
                use $crate::client::SortOrder;

                /// Nest/Prisma-style alias for the generated `Where` enum (`UserWhere::or` matches `WhereParam::Or`).
                pub type WhereParam = [< $Model Where >];

                $(
                    pub mod $field {
                        use super::[< $Model Where >];
                        use super::[< $Model OrderBy >];
                        use super::[< $Model Update >];
                        use $crate::client::SortOrder;

                        pub fn equals(v: $ftype) -> [< $Model Where >] {
                            [< $Model Where >]::[< $field:camel Eq >](v)
                        }
                        pub fn not(v: $ftype) -> [< $Model Where >] {
                            [< $Model Where >]::[< $field:camel Ne >](v)
                        }

                        pub fn order(o: SortOrder) -> [< $Model OrderBy >] {
                            [< $Model OrderBy >]::[< $field:camel >](o)
                        }

                        pub fn set(v: $ftype) -> [< $Model Update >] {
                            let mut u = [< $Model Update >]::default();
                            u.$field = std::option::Option::Some(v);
                            u
                        }
                    }
                )*
            }

            #[::async_trait::async_trait]
            pub trait [< Prisma $Model Repository >]: Send + Sync {
                async fn find_unique(
                    &self,
                    w: [< $Model Where >],
                ) -> std::result::Result<std::option::Option<$Model>, $crate::PrismaError>;

                async fn find_first(
                    &self,
                    w: [< $Model Where >],
                    order_by: std::option::Option<Vec<[< $Model OrderBy >]>>,
                ) -> std::result::Result<std::option::Option<$Model>, $crate::PrismaError>;

                async fn find_many(
                    &self,
                    w: [< $Model Where >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError>;

                async fn find_many_with_options(
                    &self,
                    opts: [< $Model FindManyOptions >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError>;

                async fn count(&self, w: [< $Model Where >]) -> std::result::Result<i64, $crate::PrismaError>;

                async fn count_selected(
                    &self,
                    w: [< $Model Where >],
                    include_all: bool,
                    fields: Vec<[< $Model ScalarField >]>,
                ) -> std::result::Result<std::collections::BTreeMap<String, i64>, $crate::PrismaError>;

                async fn aggregate(
                    &self,
                    opts: [< $Model AggregateOptions >],
                    selection: [< $Model AggregateSelection >],
                ) -> std::result::Result<[< $Model AggregateResult >], $crate::PrismaError>;

                async fn group_by(
                    &self,
                    opts: [< $Model GroupByOptions >],
                    selection: [< $Model AggregateSelection >],
                ) -> std::result::Result<Vec<[< $Model GroupByRow >]>, $crate::PrismaError>;

                async fn create(
                    &self,
                    data: [< $Model CreateInput >],
                ) -> std::result::Result<$Model, $crate::PrismaError>;

                async fn create_many(
                    &self,
                    rows: Vec<[< $Model CreateInput >]>,
                ) -> std::result::Result<u64, $crate::PrismaError>;

                async fn create_many_with_options(
                    &self,
                    rows: Vec<[< $Model CreateInput >]>,
                    opts: [< $Model CreateManyOptions >],
                ) -> std::result::Result<u64, $crate::PrismaError>;

                async fn create_many_and_return(
                    &self,
                    rows: Vec<[< $Model CreateInput >]>,
                    opts: [< $Model CreateManyOptions >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError>;

                async fn update(
                    &self,
                    w: [< $Model Where >],
                    data: [< $Model Update >],
                ) -> std::result::Result<$Model, $crate::PrismaError>;

                async fn update_many(
                    &self,
                    w: [< $Model Where >],
                    data: [< $Model Update >],
                ) -> std::result::Result<u64, $crate::PrismaError>;

                async fn update_many_and_return(
                    &self,
                    w: [< $Model Where >],
                    data: [< $Model Update >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError>;

                async fn upsert(
                    &self,
                    w: [< $Model Where >],
                    create: [< $Model CreateInput >],
                    update: [< $Model Update >],
                ) -> std::result::Result<$Model, $crate::PrismaError>;

                async fn delete(&self, w: [< $Model Where >]) -> std::result::Result<$Model, $crate::PrismaError>;

                async fn delete_many(&self, w: [< $Model Where >]) -> std::result::Result<u64, $crate::PrismaError>;
            }

            #[::async_trait::async_trait]
            impl [< Prisma $Model Repository >] for $crate::client::ModelRepository<$Model> {
                async fn find_unique(
                    &self,
                    w: [< $Model Where >],
                ) -> std::result::Result<std::option::Option<$Model>, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!(
                        "SELECT * FROM \"",
                        $table,
                        "\" WHERE "
                    ));
                    w.__push_where(&mut qb)?;
                    qb.push(" LIMIT 2");
                    let rows: Vec<$Model> = qb
                        .build_query_as()
                        .fetch_all(pool)
                        .await
                        .map_err($crate::PrismaError::from_sqlx)?;
                    if rows.len() > 1 {
                        return std::result::Result::Err($crate::PrismaError::other(
                            "find_unique: multiple rows matched",
                        ));
                    }
                    Ok(rows.into_iter().next())
                }

                async fn find_first(
                    &self,
                    w: [< $Model Where >],
                    order_by: std::option::Option<Vec<[< $Model OrderBy >]>>,
                ) -> std::result::Result<std::option::Option<$Model>, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!(
                        "SELECT * FROM \"",
                        $table,
                        "\" WHERE "
                    ));
                    w.__push_where(&mut qb)?;
                    if let Some(orders) = order_by {
                        if !orders.is_empty() {
                            qb.push(" ORDER BY ");
                            for (i, o) in orders.iter().enumerate() {
                                if i > 0 {
                                    qb.push(", ");
                                }
                                o.__push_order(&mut qb);
                            }
                        }
                    }
                    qb.push(" LIMIT 1");
                    let row = qb
                        .build_query_as()
                        .fetch_optional(pool)
                        .await
                        .map_err($crate::PrismaError::from_sqlx)?;
                    Ok(row)
                }

                async fn find_many(
                    &self,
                    w: [< $Model Where >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError> {
                    [< Prisma $Model Repository >]::find_many_with_options(
                        self,
                        [< $Model FindManyOptions >] {
                            r#where: w,
                            order_by: std::option::Option::None,
                            take: std::option::Option::None,
                            skip: std::option::Option::None,
                            distinct: std::option::Option::None,
                        },
                    )
                    .await
                }

                async fn find_many_with_options(
                    &self,
                    opts: [< $Model FindManyOptions >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError> {
                    let [< $Model FindManyOptions >] {
                        r#where,
                        order_by,
                        take,
                        skip,
                        distinct,
                    } = opts;
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!(
                        "SELECT * FROM \"",
                        $table,
                        "\" WHERE "
                    ));
                    r#where.__push_where(&mut qb)?;
                    if let Some(orders) = order_by {
                        if !orders.is_empty() {
                            qb.push(" ORDER BY ");
                            for (i, o) in orders.iter().enumerate() {
                                if i > 0 {
                                    qb.push(", ");
                                }
                                o.__push_order(&mut qb);
                            }
                        }
                    }
                    if let Some(sk) = skip {
                        qb.push(" OFFSET ");
                        qb.push_bind(sk);
                    }
                    if let Some(lim) = take {
                        qb.push(" LIMIT ");
                        qb.push_bind(lim);
                    }
                    let mut rows = qb
                        .build_query_as()
                        .fetch_all(pool)
                        .await
                        .map_err($crate::PrismaError::from_sqlx)?;
                    if let Some(distinct_fields) = distinct {
                        let mut seen = std::collections::BTreeSet::new();
                        let mut dedup = Vec::new();
                        for row in rows.drain(..) {
                            let key = [< __prisma_ $Model:snake _distinct_key >](
                                &row,
                                &distinct_fields,
                            );
                            if seen.insert(key) {
                                dedup.push(row);
                            }
                        }
                        Ok(dedup)
                    } else {
                        Ok(rows)
                    }
                }

                async fn count(&self, w: [< $Model Where >]) -> std::result::Result<i64, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb = $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!(
                        "SELECT COUNT(*) FROM \"",
                        $table,
                        "\" WHERE "
                    ));
                    w.__push_where(&mut qb)?;
                    let c: i64 = qb
                        .build_query_scalar()
                        .fetch_one(pool)
                        .await
                        .map_err($crate::PrismaError::from_sqlx)?;
                    Ok(c)
                }

                async fn count_selected(
                    &self,
                    w: [< $Model Where >],
                    include_all: bool,
                    fields: Vec<[< $Model ScalarField >]>,
                ) -> std::result::Result<std::collections::BTreeMap<String, i64>, $crate::PrismaError> {
                    let rows = [< Prisma $Model Repository >]::find_many_with_options(
                        self,
                        [< $Model FindManyOptions >] {
                            r#where: w,
                            order_by: std::option::Option::None,
                            take: std::option::Option::None,
                            skip: std::option::Option::None,
                            distinct: std::option::Option::None,
                        },
                    )
                    .await?;

                    let mut out = std::collections::BTreeMap::new();
                    if include_all {
                        out.insert("_all".to_string(), rows.len() as i64);
                    }

                    for field in fields {
                        let mut c: i64 = 0;
                        for row in &rows {
                            match field {
                                $(
                                    [< $Model ScalarField >]::[< $field:camel >] => {
                                        if $crate::__prisma_non_null_value!(row, $field, $ftype) {
                                            c += 1;
                                        }
                                    }
                                )*
                            }
                        }
                        out.insert(field.as_name().to_string(), c);
                    }
                    Ok(out)
                }

                async fn aggregate(
                    &self,
                    opts: [< $Model AggregateOptions >],
                    selection: [< $Model AggregateSelection >],
                ) -> std::result::Result<[< $Model AggregateResult >], $crate::PrismaError> {
                    let rows = [< Prisma $Model Repository >]::find_many_with_options(
                        self,
                        [< $Model FindManyOptions >] {
                            r#where: opts.r#where,
                            order_by: opts.order_by,
                            take: opts.take,
                            skip: opts.skip,
                            distinct: std::option::Option::None,
                        },
                    )
                    .await?;
                    Ok([< __prisma_ $Model:snake _compute_aggregate >](&rows, &selection))
                }

                async fn group_by(
                    &self,
                    opts: [< $Model GroupByOptions >],
                    selection: [< $Model AggregateSelection >],
                ) -> std::result::Result<Vec<[< $Model GroupByRow >]>, $crate::PrismaError> {
                    let [< $Model GroupByOptions >] {
                        by,
                        r#where,
                        order_by,
                        take,
                        skip,
                        having,
                    } = opts;

                    if by.is_empty() {
                        return std::result::Result::Err($crate::PrismaError::other("group_by: `by` cannot be empty"));
                    }

                    let rows = [< Prisma $Model Repository >]::find_many_with_options(
                        self,
                        [< $Model FindManyOptions >] {
                            r#where,
                            order_by,
                            take,
                            skip,
                            distinct: std::option::Option::None,
                        },
                    )
                    .await?;

                    let mut grouped: std::collections::BTreeMap<String, (std::collections::BTreeMap<String, String>, Vec<$Model>)> =
                        std::collections::BTreeMap::new();

                    for row in rows {
                        let mut by_values = std::collections::BTreeMap::new();
                        for field in &by {
                            by_values.insert(
                                field.as_name().to_string(),
                                [< __prisma_ $Model:snake _field_string >](&row, *field),
                            );
                        }
                        let mut key = String::new();
                        for (k, v) in &by_values {
                            key.push_str(k);
                            key.push('=');
                            key.push_str(v);
                            key.push('|');
                        }
                        let entry = grouped
                            .entry(key)
                            .or_insert_with(|| (by_values, Vec::new()));
                        entry.1.push(row);
                    }

                    let mut out = Vec::new();
                    for (_k, (by_values, group_rows)) in grouped {
                        let aggregates =
                            [< __prisma_ $Model:snake _compute_aggregate >](&group_rows, &selection);

                        let mut pass_having = true;
                        for cond in &having {
                            let field_name = cond.field.as_name().to_string();
                            let current: Option<f64> = match cond.metric {
                                [< $Model AggregateMetric >]::Count => {
                                    aggregates.count.get(&field_name).copied().map(|v| v as f64)
                                }
                                [< $Model AggregateMetric >]::Avg => {
                                    aggregates.avg.get(&field_name).copied().flatten()
                                }
                                [< $Model AggregateMetric >]::Sum => {
                                    aggregates.sum.get(&field_name).copied().flatten()
                                }
                                [< $Model AggregateMetric >]::Min => {
                                    aggregates.min.get(&field_name).and_then(|v| v.as_ref()).and_then(|s| s.parse::<f64>().ok())
                                }
                                [< $Model AggregateMetric >]::Max => {
                                    aggregates.max.get(&field_name).and_then(|v| v.as_ref()).and_then(|s| s.parse::<f64>().ok())
                                }
                            };

                            let ok = match current {
                                Some(v) => match cond.op {
                                    [< $Model HavingOp >]::Eq => v == cond.value,
                                    [< $Model HavingOp >]::Ne => v != cond.value,
                                    [< $Model HavingOp >]::Gt => v > cond.value,
                                    [< $Model HavingOp >]::Gte => v >= cond.value,
                                    [< $Model HavingOp >]::Lt => v < cond.value,
                                    [< $Model HavingOp >]::Lte => v <= cond.value,
                                },
                                None => false,
                            };
                            if !ok {
                                pass_having = false;
                                break;
                            }
                        }

                        if pass_having {
                            out.push([< $Model GroupByRow >] {
                                by: by_values,
                                aggregates,
                            });
                        }
                    }

                    Ok(out)
                }

                async fn create(
                    &self,
                    data: [< $Model CreateInput >],
                ) -> std::result::Result<$Model, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    $crate::__prisma_insert_returning!(
                        $table;
                        $Model;
                        pool;
                        data;
                        $( $field : $ftype ),*
                    )
                }

                async fn create_many(
                    &self,
                    rows: Vec<[< $Model CreateInput >]>,
                ) -> std::result::Result<u64, $crate::PrismaError> {
                    [< Prisma $Model Repository >]::create_many_with_options(
                        self,
                        rows,
                        [< $Model CreateManyOptions >]::default(),
                    )
                    .await
                }

                async fn create_many_with_options(
                    &self,
                    rows: Vec<[< $Model CreateInput >]>,
                    opts: [< $Model CreateManyOptions >],
                ) -> std::result::Result<u64, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    $crate::__prisma_insert_many_with_options_dispatch!(
                        $table;
                        pool;
                        rows;
                        opts.skip_duplicates;
                        $( $field : $ftype ),*
                    )
                }

                async fn create_many_and_return(
                    &self,
                    rows: Vec<[< $Model CreateInput >]>,
                    opts: [< $Model CreateManyOptions >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    $crate::__prisma_insert_many_returning_dispatch!(
                        $table;
                        $Model;
                        pool;
                        rows;
                        opts.skip_duplicates;
                        $( $field : $ftype ),*
                    )
                }

                async fn update(
                    &self,
                    w: [< $Model Where >],
                    data: [< $Model Update >],
                ) -> std::result::Result<$Model, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb =
                        $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("UPDATE \"", $table, "\" SET "));
                    data.__push_set(&mut qb)?;
                    qb.push(" WHERE ");
                    w.__push_where(&mut qb)?;
                    qb.push(" RETURNING *");
                    let row = qb
                        .build_query_as()
                        .fetch_one(pool)
                        .await
                        .map_err($crate::PrismaError::from_sqlx)?;
                    Ok(row)
                }

                async fn update_many(
                    &self,
                    w: [< $Model Where >],
                    data: [< $Model Update >],
                ) -> std::result::Result<u64, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb =
                        $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("UPDATE \"", $table, "\" SET "));
                    data.__push_set(&mut qb)?;
                    qb.push(" WHERE ");
                    w.__push_where(&mut qb)?;
                    let res = qb.build().execute(pool).await.map_err($crate::PrismaError::from_sqlx)?;
                    Ok(res.rows_affected())
                }

                async fn update_many_and_return(
                    &self,
                    w: [< $Model Where >],
                    data: [< $Model Update >],
                ) -> std::result::Result<Vec<$Model>, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb =
                        $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("UPDATE \"", $table, "\" SET "));
                    data.__push_set(&mut qb)?;
                    qb.push(" WHERE ");
                    w.__push_where(&mut qb)?;
                    qb.push(" RETURNING *");
                    qb.build_query_as()
                        .fetch_all(pool)
                        .await
                        .map_err($crate::PrismaError::from_sqlx)
                }

                async fn upsert(
                    &self,
                    w: [< $Model Where >],
                    create: [< $Model CreateInput >],
                    update: [< $Model Update >],
                ) -> std::result::Result<$Model, $crate::PrismaError> {
                    $crate::__prisma_upsert_dispatch!(
                        $Model,
                        $table,
                        self,
                        w,
                        create,
                        update;
                        $( $field : $ftype ),*
                    )
                }

                async fn delete(&self, w: [< $Model Where >]) -> std::result::Result<$Model, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb =
                        $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("DELETE FROM \"", $table, "\" WHERE "));
                    w.__push_where(&mut qb)?;
                    qb.push(" RETURNING *");
                    let row = qb
                        .build_query_as()
                        .fetch_optional(pool)
                        .await
                        .map_err($crate::PrismaError::from_sqlx)?;
                    row.ok_or($crate::PrismaError::RowNotFound)
                }

                async fn delete_many(&self, w: [< $Model Where >]) -> std::result::Result<u64, $crate::PrismaError> {
                    let pool = $crate::sqlx_pool().await?;
                    let mut qb =
                        $crate::sqlx::QueryBuilder::<$crate::SqlxDb>::new(concat!("DELETE FROM \"", $table, "\" WHERE "));
                    w.__push_where(&mut qb)?;
                    let res = qb.build().execute(pool).await.map_err($crate::PrismaError::from_sqlx)?;
                    Ok(res.rows_affected())
                }
            }

            pub trait [< Prisma $Model ClientExt >] {
                fn [< $Model:snake >](&self) -> $crate::client::ModelRepository<$Model>;
            }

            impl [< Prisma $Model ClientExt >] for std::sync::Arc<$crate::PrismaService> {
                fn [< $Model:snake >](&self) -> $crate::client::ModelRepository<$Model> {
                    $crate::client::ModelRepository::new(std::sync::Arc::clone(self))
                }
            }
        }
    };
}
