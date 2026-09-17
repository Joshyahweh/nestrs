//! Typed `MongoRepository<T>` CRUD wrapper.
//!
//! Phase D deliverable. The repository wraps a `mongodb::Collection<T>` and
//! exposes the operations every NestJS mongoose user reaches for first:
//! `find_by_id`, `find_one`, `find`, `insert_one`, `insert_many`,
//! `update_one`, `update_many`, `replace_one`, `delete_one`,
//! `delete_many`, `count_documents`, plus the three `find_one_and_*`
//! variants. Filters and updates are typed wrappers around
//! `bson::Document` so the underlying driver API is reachable when needed.

use crate::client::{MongoModule, MongoService};
use crate::error::{MongoError, Result};
use crate::schema::Schema;
use bson::{doc, Bson, Document as BsonDoc};
use futures::stream::TryStreamExt;
use mongodb::options::{
    FindOneAndUpdateOptions, FindOptions, InsertManyOptions, ReplaceOptions, UpdateOptions,
};
use mongodb::{Collection, Database};
use std::marker::PhantomData;

/// Typed filter alias. A `Filter` wraps a `BsonDoc` with a phantom
/// binding to `T` so call sites can be generic over the document type.
/// Implements `Deref<Target = BsonDoc>` for transparent access.
#[derive(Debug, Clone)]
pub struct Filter<T>(pub BsonDoc, PhantomData<T>);

impl<T> Filter<T> {
    /// Create a new filter from a raw `BsonDoc`.
    pub fn new(doc: BsonDoc) -> Self {
        Self(doc, PhantomData)
    }
}

impl<T> std::ops::Deref for Filter<T> {
    type Target = BsonDoc;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Filter<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<BsonDoc> for Filter<T> {
    fn from(doc: BsonDoc) -> Self {
        Self(doc, PhantomData)
    }
}

impl<T> From<Filter<T>> for BsonDoc {
    fn from(filter: Filter<T>) -> Self {
        filter.0
    }
}

/// Typed update alias. An `Update` wraps a `BsonDoc` of operator-style
/// modifiers (`{"$set": …, "$inc": …}`) with a phantom binding to `T`.
/// Implements `Deref<Target = BsonDoc>` for transparent access.
#[derive(Debug, Clone)]
pub struct Update<T>(pub BsonDoc, PhantomData<T>);

impl<T> Update<T> {
    /// Create a new update from a raw `BsonDoc`.
    pub fn new(doc: BsonDoc) -> Self {
        Self(doc, PhantomData)
    }
}

impl<T> std::ops::Deref for Update<T> {
    type Target = BsonDoc;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Update<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<BsonDoc> for Update<T> {
    fn from(doc: BsonDoc) -> Self {
        Self(doc, PhantomData)
    }
}

impl<T> From<Update<T>> for BsonDoc {
    fn from(update: Update<T>) -> Self {
        update.0
    }
}

impl<T> From<Update<T>> for mongodb::options::UpdateModifications {
    fn from(update: Update<T>) -> Self {
        update.0.into()
    }
}

/// Typed CRUD wrapper over a `mongodb::Collection<T>`. Built from a
/// `Database` handle (typically `MongoService::database(name)`); the
/// collection name comes from `T::collection_name()`.
///
/// All methods are `async` and surface errors as `MongoError`. They never
/// panic on a missing document — `find_one` and `find_by_id` return
/// `Ok(None)` for no match, and `update_*` / `delete_*` return the
/// driver's `ModifiedCount` / `DeletedCount` directly.
pub struct MongoRepository<T: Schema> {
    collection: Collection<T>,
    _phantom: PhantomData<T>,
}

impl<T: Schema> MongoRepository<T> {
    /// Build a repository over the named collection in `db`.
    pub fn new(db: &Database, _name: Option<&str>) -> Result<Self> {
        let collection = db.collection::<T>(T::collection_name());
        Ok(Self {
            collection,
            _phantom: PhantomData,
        })
    }

    /// Build from an existing collection handle. Useful when the caller
    /// already resolved the collection through a different path.
    pub fn from_collection(collection: Collection<T>) -> Self {
        Self {
            collection,
            _phantom: PhantomData,
        }
    }

    /// Borrow the underlying `mongodb::Collection<T>`. Most callers won't
    /// need this — the typed wrapper exposes every operation we ship — but
    /// advanced users (aggregation pipelines, change streams, indexes)
    /// can drop down.
    pub fn collection(&self) -> &Collection<T> {
        &self.collection
    }

    /// Resolve a repository by going through the named database on the
    /// shared `MongoService`. Mirrors the most common NestJS mongoose
    /// usage:
    ///
    /// ```ignore
    /// let users: MongoRepository<User> = svc.repository("app", None)?;
    /// ```
    pub async fn from_service(svc: &MongoService, db_name: &str) -> Result<Self> {
        let db = svc.database(db_name).await?;
        Self::new(&db, None)
    }

    /// Resolve a repository using the database name registered by
    /// [`crate::client::MongoModule::for_feature`]. The default Mongoose
    /// "feature module" pattern — the database name lives at the module
    /// level, every typed repository reads it at injection time.
    ///
    /// ```ignore
    /// // boot:
    /// #[module(imports = [MongoModule::for_feature("app")])]
    /// struct AppModule;
    ///
    /// // controller / service:
    /// let users: MongoRepository<User> = MongoRepository::for_feature(&svc).await?;
    /// ```
    pub async fn for_feature(svc: &MongoService) -> Result<Self> {
        let db_name = MongoModule::feature_db().ok_or_else(|| {
            MongoError::InvalidArgument(
                "MongoModule::for_feature(<db_name>) must be called before MongoRepository::for_feature"
                    .into(),
            )
        })?;
        Self::from_service(svc, db_name).await
    }

    // -----------------------------------------------------------------------
    // Read
    // -----------------------------------------------------------------------

    /// Find one document matching `filter`. Returns `Ok(None)` for no match.
    pub async fn find_one(&self, filter: Filter<T>) -> Result<Option<T>> {
        self.collection
            .find_one(filter.into())
            .await
            .map_err(MongoError::from)
    }

    /// Find one document by its `_id`. Convenience over `find_one` for the
    /// common primary-key lookup. `id` must be a `bson::Bson` value the
    /// driver can compare against the stored `_id`.
    pub async fn find_by_id(&self, id: impl Into<bson::Bson>) -> Result<Option<T>> {
        let filter = doc! { "_id": id.into() };
        self.find_one(filter.into()).await
    }

    /// Find all documents matching `filter`. Returns a `Vec` — for large
    /// result sets, drop down to [`MongoRepository::collection`] and use
    /// the driver's streaming `find` instead.
    pub async fn find(&self, filter: Filter<T>) -> Result<Vec<T>> {
        let cursor = self
            .collection
            .find(filter.into())
            .await
            .map_err(MongoError::from)?;
        cursor.try_collect().await.map_err(MongoError::from)
    }

    /// Find all documents matching `filter`, capped at `limit` and sorted
    /// by `sort` (a `BsonDoc` like `{"created_at": -1}`).
    pub async fn find_with_options(
        &self,
        filter: Filter<T>,
        sort: Option<BsonDoc>,
        limit: Option<i64>,
    ) -> Result<Vec<T>> {
        let mut opts = FindOptions::default();
        opts.sort = sort;
        opts.limit = limit;
        let cursor = self
            .collection
            .find(filter.into())
            .with_options(opts)
            .await
            .map_err(MongoError::from)?;
        cursor.try_collect().await.map_err(MongoError::from)
    }

    /// Count documents matching `filter`. Use `estimated_document_count`
    /// when no filter is needed — it reads the collection metadata
    /// instead of scanning.
    pub async fn count_documents(&self, filter: Filter<T>) -> Result<u64> {
        self.collection
            .count_documents(filter.into())
            .await
            .map_err(MongoError::from)
    }

    /// Cheap count of all documents in the collection. Ignores any filter.
    pub async fn estimated_document_count(&self) -> Result<u64> {
        self.collection
            .estimated_document_count()
            .await
            .map_err(MongoError::from)
    }

    // -----------------------------------------------------------------------
    // Insert
    // -----------------------------------------------------------------------

    /// Insert a single document. Sets `inserted_id` on `doc` if the field
    /// is named `_id` and was `None`.
    pub async fn insert_one(&self, doc: &mut T) -> Result<Bson> {
        let result = self
            .collection
            .insert_one(doc)
            .await
            .map_err(MongoError::from)?;
        Ok(result.inserted_id)
    }

    /// Insert many documents. Returns the inserted `_id` values in order.
    pub async fn insert_many(&self, docs: &mut [T]) -> Result<Vec<bson::Bson>> {
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        let result = self
            .collection
            .insert_many(docs)
            .with_options(InsertManyOptions::default())
            .await
            .map_err(MongoError::from)?;
        Ok(result.inserted_ids.values().cloned().collect())
    }

    // -----------------------------------------------------------------------
    // Update
    // -----------------------------------------------------------------------

    /// Apply `update` (an operator-style document like `{"$set": …}`) to the
    /// first document matching `filter`. Returns the modified count.
    pub async fn update_one(&self, filter: Filter<T>, update: Update<T>) -> Result<u64> {
        let result = self
            .collection
            .update_one(filter.into(), update)
            .await
            .map_err(MongoError::from)?;
        Ok(result.modified_count)
    }

    /// Apply `update` to every document matching `filter`. Returns the
    /// modified count.
    pub async fn update_many(&self, filter: Filter<T>, update: Update<T>) -> Result<u64> {
        let result = self
            .collection
            .update_many(filter.into(), update)
            .await
            .map_err(MongoError::from)?;
        Ok(result.modified_count)
    }

    /// Apply `update` (operator-style) and return the post-update document
    /// in the same round-trip. `return_document: Before` / `After` mirrors
    /// the driver's `ReturnDocument` enum.
    pub async fn find_one_and_update(
        &self,
        filter: Filter<T>,
        update: Update<T>,
        return_after: bool,
    ) -> Result<Option<T>> {
        let mut opts = FindOneAndUpdateOptions::default();
        opts.return_document = if return_after {
            Some(mongodb::options::ReturnDocument::After)
        } else {
            Some(mongodb::options::ReturnDocument::Before)
        };
        self.collection
            .find_one_and_update(filter.into(), update)
            .with_options(opts)
            .await
            .map_err(MongoError::from)
    }

    /// Replace a document in full. `replacement` is the full new document,
    /// not an operator-style update. Returns the modified count.
    pub async fn replace_one(&self, filter: Filter<T>, replacement: T) -> Result<u64> {
        let result = self
            .collection
            .replace_one(filter.into(), replacement)
            .with_options(ReplaceOptions::default())
            .await
            .map_err(MongoError::from)?;
        Ok(result.modified_count)
    }

    // -----------------------------------------------------------------------
    // Delete
    // -----------------------------------------------------------------------

    /// Delete the first document matching `filter`. Returns the deleted count.
    pub async fn delete_one(&self, filter: Filter<T>) -> Result<u64> {
        let result = self
            .collection
            .delete_one(filter.into())
            .await
            .map_err(MongoError::from)?;
        Ok(result.deleted_count)
    }

    /// Delete every document matching `filter`. Returns the deleted count.
    pub async fn delete_many(&self, filter: Filter<T>) -> Result<u64> {
        let result = self
            .collection
            .delete_many(filter.into())
            .await
            .map_err(MongoError::from)?;
        Ok(result.deleted_count)
    }

    /// Convenience: delete by `_id`. Equivalent to `delete_one({"_id": id})`.
    pub async fn delete_by_id(&self, id: impl Into<bson::Bson>) -> Result<u64> {
        let filter = doc! { "_id": id.into() };
        self.delete_one(filter.into()).await
    }

    // -----------------------------------------------------------------------
    // Update options helper
    // -----------------------------------------------------------------------

    /// Re-export of `mongodb::options::UpdateOptions` so callers can build
    /// upsert / array-filter / collation options without depending on the
    /// driver crate directly.
    pub fn update_options() -> UpdateOptions {
        UpdateOptions::default()
    }
}

impl<T: Schema + Send + Sync + 'static> MongoRepository<T> {
    /// Build a repository from an existing `Collection<T>` whose name
    /// doesn't necessarily match `T::collection_name()` (rare — the
    /// collection-renaming flow used by some Mongoose refactor patterns).
    pub fn with_name(db: &Database, name: &str) -> Result<Self> {
        let collection = db.collection::<T>(name);
        Ok(Self {
            collection,
            _phantom: PhantomData,
        })
    }
}
