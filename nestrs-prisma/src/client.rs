//! Shared types for the declarative `prisma_model!` client (sort order, repository handle).

#[cfg(feature = "sqlx")]
use std::marker::PhantomData;
#[cfg(feature = "sqlx")]
use std::sync::Arc;

#[cfg(feature = "sqlx")]
use crate::PrismaService;

/// SQL `ASC` / `DESC` (used by generated `order` helpers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub fn as_sql(self) -> &'static str {
        match self {
            SortOrder::Asc => "ASC",
            SortOrder::Desc => "DESC",
        }
    }
}

/// Per-model repository bound to a table (constructed via the `prisma_model!` extension trait on [`Arc<PrismaService>`]).
#[cfg(feature = "sqlx")]
pub struct ModelRepository<M> {
    /// Keeps [`PrismaService`] (and thus pool options) alive for the lifetime of this handle.
    #[allow(dead_code)]
    pub(crate) prisma: Arc<PrismaService>,
    pub(crate) _marker: PhantomData<M>,
}

#[cfg(feature = "sqlx")]
impl<M> ModelRepository<M> {
    /// Wraps a [`PrismaService`] for typed queries (normally constructed via `prisma_model!` extension traits).
    pub fn new(prisma: Arc<PrismaService>) -> Self {
        Self {
            prisma,
            _marker: PhantomData,
        }
    }

    /// Accesses the underlying shared [`PrismaService`] handle.
    pub fn prisma(&self) -> Arc<PrismaService> {
        Arc::clone(&self.prisma)
    }
}
