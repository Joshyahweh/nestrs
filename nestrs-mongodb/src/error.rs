//! MongoDB error type. Wraps the upstream `mongodb::error::Error` so callers
//! don't have to depend on the driver directly to surface errors.

use std::fmt;

/// Errors that can originate from `nestrs-mongodb` operations.
#[derive(Debug)]
pub enum MongoError {
    /// Wrapped upstream driver error (connection, command, decode, …).
    Driver(mongodb::error::Error),
    /// `MongoModule::for_root` was not called before a service operation.
    NotConfigured,
    /// BSON serialization/deserialization failed.
    Bson(bson::ser::Error),
    /// BSON deserialization failed (separate variant — upstream splits these).
    BsonDe(bson::de::Error),
    /// Operation timed out before completing.
    Timeout,
    /// A caller-supplied filter / update document was invalid.
    InvalidArgument(String),
}

impl fmt::Display for MongoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MongoError::Driver(e) => write!(f, "mongodb driver error: {e}"),
            MongoError::NotConfigured => write!(
                f,
                "MongoModule::for_root must be called before using MongoService"
            ),
            MongoError::Bson(e) => write!(f, "bson encode error: {e}"),
            MongoError::BsonDe(e) => write!(f, "bson decode error: {e}"),
            MongoError::Timeout => write!(f, "mongodb operation timed out"),
            MongoError::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
        }
    }
}

impl std::error::Error for MongoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            MongoError::Driver(e) => Some(e),
            MongoError::Bson(e) => Some(e),
            MongoError::BsonDe(e) => Some(e),
            _ => None,
        }
    }
}

impl From<mongodb::error::Error> for MongoError {
    fn from(e: mongodb::error::Error) -> Self {
        MongoError::Driver(e)
    }
}

impl From<bson::ser::Error> for MongoError {
    fn from(e: bson::ser::Error) -> Self {
        MongoError::Bson(e)
    }
}

impl From<bson::de::Error> for MongoError {
    fn from(e: bson::de::Error) -> Self {
        MongoError::BsonDe(e)
    }
}

/// Convenience type alias for `nestrs-mongodb` results.
pub type Result<T> = std::result::Result<T, MongoError>;
