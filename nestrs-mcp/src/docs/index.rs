//! In-memory docs corpus. Built once per workspace, queried many times.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

/// One source of docs (one file).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocSource {
    pub path: PathBuf,
    pub kind: DocKind,
    pub bytes: usize,
    pub content: String,
}

/// What kind of source this is — used by the searcher for scope filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    Changelog,
    Book,
    Readme,
    Other,
}

#[derive(Debug, Default)]
pub struct DocStore {
    inner: RwLock<DocStoreInner>,
}

#[derive(Debug, Default)]
struct DocStoreInner {
    sources: Vec<DocSource>,
    /// When the index was built (so callers can show "indexed N files" in
    /// the `search_docs` response).
    file_count: usize,
}

impl DocStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the index for the given workspace, replacing any prior index.
    pub fn build(&self, root: &Path) -> crate::Result<usize> {
        let mut sources = Vec::new();
        let mut changelog = None;
        let mut book = Vec::new();
        let mut readmes = Vec::new();
        let other = Vec::new();

        if !root.join("CHANGELOG.md").exists()
            && !root.join("docs").exists()
            && !walkdir_has_readme(root)
        {
            return Err(crate::Error::WorkspaceNotFound(format!(
                "{} is not a nestrs workspace (no CHANGELOG.md, docs/, or README.md found)",
                root.display()
            )));
        }

        if root.join("CHANGELOG.md").exists() {
            changelog = Some(load(root.join("CHANGELOG.md"), DocKind::Changelog)?);
        }

        let docs_dir = root.join("docs");
        if docs_dir.is_dir() {
            for entry in WalkDir::new(&docs_dir).into_iter().filter_map(Result::ok) {
                let p = entry.path();
                if p.extension().and_then(|s| s.to_str()) == Some("md") {
                    if let Ok(src) = load(p.to_path_buf(), DocKind::Book) {
                        book.push(src);
                    }
                }
            }
        }

        for entry in WalkDir::new(root)
            .max_depth(3)
            .into_iter()
            .filter_map(Result::ok)
        {
            let p = entry.path();
            if p.is_file() && p.file_name().and_then(|s| s.to_str()) == Some("README.md") {
                if let Ok(src) = load(p.to_path_buf(), DocKind::Readme) {
                    readmes.push(src);
                }
            }
        }

        sources.extend(changelog);
        sources.extend(book);
        sources.extend(readmes);
        sources.extend(other);

        let count = sources.len();
        let mut inner = self.inner.write().unwrap();
        inner.sources = sources;
        inner.file_count = count;
        Ok(count)
    }

    pub fn sources(&self) -> Vec<DocSource> {
        self.inner.read().unwrap().sources.clone()
    }

    pub fn file_count(&self) -> usize {
        self.inner.read().unwrap().file_count
    }
}

fn load(path: PathBuf, kind: DocKind) -> std::io::Result<DocSource> {
    let content = fs::read_to_string(&path)?;
    Ok(DocSource {
        bytes: content.len(),
        content,
        path,
        kind,
    })
}

fn walkdir_has_readme(root: &Path) -> bool {
    WalkDir::new(root)
        .max_depth(3)
        .into_iter()
        .filter_map(Result::ok)
        .any(|e| e.file_type().is_file() && e.file_name().to_str() == Some("README.md"))
}

/// Convenience: return the changelog section headings (the lines starting
/// with `## [`) as a `Vec<{ version, date }>`. Used by `get_changelog`.
pub fn changelog_entries(source: &DocSource) -> Vec<ChangelogEntry> {
    if source.kind != DocKind::Changelog {
        return Vec::new();
    }
    let mut out = Vec::new();
    for line in source.content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## [") {
            if let Some(end) = rest.find(']') {
                let version = rest[..end].to_string();
                let date = rest[end + 1..]
                    .trim()
                    .trim_start_matches('-')
                    .trim()
                    .to_string();
                if !version.is_empty() {
                    out.push(ChangelogEntry { version, date });
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChangelogEntry {
    pub version: String,
    pub date: String,
}

/// A path → kind lookup, mostly used by the `get_doc` tool.
pub fn kind_lookup(sources: &[DocSource]) -> BTreeMap<PathBuf, DocKind> {
    sources.iter().map(|s| (s.path.clone(), s.kind)).collect()
}
