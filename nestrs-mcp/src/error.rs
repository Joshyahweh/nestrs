//! Crate-level error type.

use thiserror::Error;

use crate::introspection::registry::SnapshotError;
use crate::introspection::source::SourceParserError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse error: {0}")]
    Parse(String),

    #[error("workspace not found: {0}")]
    WorkspaceNotFound(String),

    #[error("file not found: {0}")]
    FileNotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("scaffolding error: {0}")]
    Scaffold(String),

    #[error("docs error: {0}")]
    Docs(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("admin error: {0}")]
    Admin(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<SourceParserError> for Error {
    fn from(e: SourceParserError) -> Self {
        match e {
            SourceParserError::WorkspaceNotFound(s) => Error::WorkspaceNotFound(s.to_string_lossy().into_owned()),
            SourceParserError::NotADirectory(s) => {
                Error::InvalidArgument(format!("{} is not a directory", s.to_string_lossy()))
            }
            SourceParserError::NoCargoToml(s) => {
                Error::InvalidArgument(format!("no Cargo.toml in {}", s.to_string_lossy()))
            }
            SourceParserError::Io { source, .. } => Error::Io(source),
            SourceParserError::Syn { file, message } => {
                Error::Parse(format!("{file}: {message}"))
            }
        }
    }
}

impl From<SnapshotError> for Error {
    fn from(e: SnapshotError) -> Self {
        match e {
            SnapshotError::Http { status, body } => Error::Admin(format!("HTTP {status}: {body}")),
            SnapshotError::Parse(s) => Error::Parse(s.to_string()),
            SnapshotError::MissingField(s) => Error::Parse(s.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

