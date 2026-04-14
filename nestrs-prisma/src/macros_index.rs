//! Macros for [`crate::index_ddl::WherePredicate`].

/// Builds [`crate::index_ddl::WherePredicate::Raw`] (Prisma `where: raw("…")` for partial indexes).
#[macro_export]
macro_rules! prisma_where_raw {
    ($sql:expr) => {
        $crate::index_ddl::WherePredicate::raw($sql)
    };
}
