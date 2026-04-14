//! Typed errors for the generated Prisma-style client and mapping to [`nestrs::HttpException`].

use nestrs::HttpException;
#[cfg(not(feature = "sqlx"))]
use nestrs::InternalServerErrorException;
#[cfg(feature = "sqlx")]
use nestrs::{
    ConflictException, InternalServerErrorException, NotFoundException, ServiceUnavailableException,
};

#[cfg(feature = "sqlx")]
#[derive(Debug, thiserror::Error)]
pub enum PrismaError {
    #[error("database pool: {0}")]
    PoolInit(String),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error("record not found")]
    RowNotFound,
    #[error("unique constraint violated: {0}")]
    UniqueViolation(String),
    #[error("{0}")]
    Other(String),
}

#[cfg(feature = "sqlx")]
impl PrismaError {
    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    pub fn from_sqlx(e: sqlx::Error) -> Self {
        if matches!(e, sqlx::Error::RowNotFound) {
            return Self::RowNotFound;
        }
        if let Some(db) = e.as_database_error() {
            if db.is_unique_violation() {
                return Self::UniqueViolation(db.message().to_string());
            }
        }
        Self::Sqlx(e)
    }
}

#[cfg(feature = "sqlx")]
impl From<PrismaError> for HttpException {
    fn from(value: PrismaError) -> Self {
        match value {
            PrismaError::RowNotFound => NotFoundException::new("record not found"),
            PrismaError::UniqueViolation(msg) => ConflictException::new(msg),
            PrismaError::PoolInit(msg) => ServiceUnavailableException::new(msg),
            PrismaError::Sqlx(e) => {
                if let Some(db) = e.as_database_error() {
                    if db.is_unique_violation() {
                        return ConflictException::new(db.message());
                    }
                }
                match &e {
                    sqlx::Error::PoolClosed | sqlx::Error::Protocol(_) => {
                        ServiceUnavailableException::new(e.to_string())
                    }
                    _ => InternalServerErrorException::new(e.to_string()),
                }
            }
            PrismaError::Other(msg) => InternalServerErrorException::new(msg),
        }
    }
}

#[cfg(not(feature = "sqlx"))]
#[derive(Debug, thiserror::Error)]
pub enum PrismaError {
    #[error("enable feature `sqlx` on nestrs-prisma for the generated Prisma client")]
    ClientDisabled,
}

#[cfg(not(feature = "sqlx"))]
impl From<PrismaError> for HttpException {
    fn from(e: PrismaError) -> Self {
        InternalServerErrorException::new(e.to_string())
    }
}
