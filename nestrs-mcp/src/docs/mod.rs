//! Local-file docs search (CHANGELOG, mdBook, READMEs).
//!
//! Loaded lazily on first query per `workspace_path`; cached for the
//! lifetime of the server process. No external index, no network — fast
//! enough for a repo-sized corpus.

pub mod index;
pub mod search;

pub use index::{changelog_entries, ChangelogEntry, DocKind, DocSource, DocStore};
pub use search::{DocHit, DocSearcher, SearchScope};
