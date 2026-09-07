//! Per-request batching loaders (Wave 4.1, `dataloader` feature).
//!
//! Wraps async-graphql's `DataLoader` in the wiring a NestJS-style app
//! needs: a registry of loader *factories* that is consulted once per
//! request and installs a **fresh** `DataLoader` instance into the
//! request's data map. Freshness matters — a `DataLoader` caches
//! internally for its whole lifetime, so a schema-global instance would
//! wrongly memoize results (and share batch windows) across requests.
//!
//! ## Usage
//!
//! ```ignore
//! #[dataloader(key = i64, value = User, error = MyError)]
//! struct UserLoader { pool: Arc<AnyPool> }
//!
//! impl UserLoader {
//!     async fn batch_load(&self, keys: &[i64]) -> Result<HashMap<i64, User>, MyError> {
//!         /* one SELECT ... WHERE id IN (...) */
//!     }
//! }
//!
//! let ctx = GqlDataContext::new()
//!     .with_loaders(DataLoaderRegistry::new()
//!         .with_loader(move || user_loader.clone().into_data_loader()));
//! ```
//!
//! Resolvers fetch it back with
//! `ctx.data_unchecked::<DataLoader<UserLoader>>().load_one(id).await`
//! (or `load_many`). A resolver making two `load_one` calls inside one
//! request batch resolves in a single `batch_load` — that is the N+1 fix.

use std::marker::PhantomData;
use std::sync::Arc;

/// The async-graphql batching primitives, re-exported so downstream
/// crates (and the `#[dataloader]` macro's generated code) can name
/// `DataLoader` / `Loader` without adding async-graphql as a direct
/// dependency.
pub use async_graphql::dataloader::{DataLoader, Loader};

/// Constructs the per-request loader instances. Object-safe so a
/// registry can hold a heterogeneous list.
pub trait DataLoaderFactory: Send + Sync + 'static {
    /// Build the loader and insert it into `request`'s data map, keyed
    /// by the `DataLoader<L>` type.
    fn install(&self, request: &mut async_graphql::Request);
}

/// Closure wrapper. (A blanket `impl<F, L> DataLoaderFactory for F where
/// F: Fn() -> DataLoader<L>` would leave `L` unconstrained — a
/// return-type position does not constrain a type parameter — so the
/// factory closure is wrapped and `L` pinned down by the struct.)
struct LoaderFactory<F, L> {
    make: F,
    _marker: PhantomData<fn() -> L>,
}

impl<F, L> DataLoaderFactory for LoaderFactory<F, L>
where
    F: Fn() -> DataLoader<L> + Send + Sync + 'static,
    L: Send + Sync + 'static,
{
    fn install(&self, request: &mut async_graphql::Request) {
        request.data.insert((self.make)());
    }
}

/// A set of loader factories consulted once per request by the
/// `graphql-authz` hook path (`GqlDataContext::prepare`). Cloning is
/// cheap (`Arc` list share).
#[derive(Clone, Default)]
pub struct DataLoaderRegistry {
    factories: Vec<Arc<dyn DataLoaderFactory>>,
}

impl DataLoaderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a factory that builds one loader type per request. The
    /// closure runs once per incoming request (not once at startup),
    /// which is what keeps the `DataLoader`'s internal cache and batch
    /// window request-scoped.
    pub fn with_loader<F, L>(mut self, make: F) -> Self
    where
        F: Fn() -> DataLoader<L> + Send + Sync + 'static,
        L: Send + Sync + 'static,
    {
        self.factories.push(Arc::new(LoaderFactory {
            make,
            _marker: PhantomData,
        }));
        self
    }

    /// True when no factories are registered (the hook's `prepare` then
    /// does nothing).
    pub fn is_empty(&self) -> bool {
        self.factories.is_empty()
    }

    /// Insert one fresh instance of every registered loader into
    /// `request`'s data map.
    pub fn install(&self, request: &mut async_graphql::Request) {
        for factory in &self.factories {
            factory.install(request);
        }
    }
}

/// Fresh `DataLoader` around `loader`: 1 ms batch window, batch tasks
/// spawned on the current tokio runtime, **no** internal cache (the
/// default async-graphql constructor — per-request instances make
/// memoization redundant; batching is what fixes N+1). This is what
/// `#[dataloader]`'s generated `into_data_loader` returns, so resolvers
/// can uniformly write `DataLoader<MyLoader>`.
pub fn data_loader<L>(loader: L) -> DataLoader<L>
where
    L: Send + Sync + 'static,
{
    DataLoader::new(loader, |fut| tokio::spawn(fut))
}

/// Like [`data_loader`] but with a per-instance LRU cache of `cap`
/// entries — for loaders where resolvers may repeat keys inside one
/// request beyond what the 1 ms batch window already deduplicates.
/// Note the concrete type changes (`DataLoader<L, LruCache>`), so
/// resolvers must name `DataLoader<L, LruCache>` when fetching it back.
pub fn data_loader_cached<L>(loader: L, cap: usize) -> DataLoader<L, async_graphql::dataloader::LruCache>
where
    L: Send + Sync + 'static,
{
    DataLoader::with_cache(
        loader,
        |fut| tokio::spawn(fut),
        async_graphql::dataloader::LruCache::new(cap),
    )
}
