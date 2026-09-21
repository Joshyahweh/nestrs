//! Errors for [`crate::Repo`] and ambient SeaORM transactions.

use sea_orm::DbErr;
use std::fmt;

/// Errors from [`crate::Repo`] authorized helpers and transaction helpers.
#[derive(Debug)]
pub enum RepoError {
    /// Underlying SeaORM / database error.
    Db(DbErr),
    /// Policy denied the operation (no matching ability, or row predicate failed).
    Denied(String),
    /// Authorized helpers require a [`crate::authz::RowAuthz`] context and none was usable.
    MissingAuthz(&'static str),
}

impl fmt::Display for RepoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Db(e) => write!(f, "{e}"),
            Self::Denied(msg) => write!(f, "policy denied: {msg}"),
            Self::MissingAuthz(op) => write!(
                f,
                "Repo::{op} requires a RowAuthz context (pass AbilityAuthz / install policies)"
            ),
        }
    }
}

impl std::error::Error for RepoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Db(e) => Some(e),
            _ => None,
        }
    }
}

impl From<DbErr> for RepoError {
    fn from(value: DbErr) -> Self {
        Self::Db(value)
    }
}
