//! Plain substring + token-weighted scoring over the in-memory corpus.
//!
//! Good enough for a repo-sized corpus (CHANGELOG + mdBook + READMEs =
//! usually <500 docs). If latency becomes a problem, swap in Tantivy
//! without changing the public API.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::index::DocSource;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    Changelog,
    Book,
    Readme,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocHit {
    pub path: String,
    pub kind: String,
    pub score: f64,
    /// Up to 3 surrounding lines (the line itself, one before, one after).
    pub context: Vec<String>,
}

#[derive(Debug)]
pub struct DocSearcher;

impl DocSearcher {
    /// Search the corpus for `query`. Returns up to `limit` hits ordered
    /// by score descending.
    pub fn search(
        sources: &[DocSource],
        query: &str,
        scope: SearchScope,
        limit: usize,
    ) -> Vec<DocHit> {
        let terms: Vec<String> = query
            .split_whitespace()
            .map(|s| s.to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if terms.is_empty() {
            return Vec::new();
        }

        let mut hits: Vec<DocHit> = sources
            .iter()
            .filter(|s| match scope {
                SearchScope::All => true,
                SearchScope::Changelog => matches!(s.kind, super::index::DocKind::Changelog),
                SearchScope::Book => matches!(s.kind, super::index::DocKind::Book),
                SearchScope::Readme => matches!(s.kind, super::index::DocKind::Readme),
            })
            .filter_map(|s| score(s, &terms))
            .collect();

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(limit);
        hits
    }
}

fn score(source: &DocSource, terms: &[String]) -> Option<DocHit> {
    let mut total = 0f64;
    let mut best_line: Option<(usize, f64)> = None;
    let lower = source.content.to_ascii_lowercase();
    for (i, line) in source.content.lines().enumerate() {
        let line_lower = line.to_ascii_lowercase();
        let mut line_score = 0f64;
        for t in terms {
            let occurrences = line_lower.matches(t.as_str()).count();
            if occurrences > 0 {
                // Title-like lines (start with `#`) count more.
                let bonus = if line_lower.trim_start().starts_with('#') {
                    3.0
                } else {
                    1.0
                };
                line_score += occurrences as f64 * bonus;
            }
        }
        if line_score > 0.0 {
            total += line_score;
            if best_line.as_ref().map(|(_, s)| line_score > *s).unwrap_or(true) {
                best_line = Some((i, line_score));
            }
        }
    }

    // Also count document-level matches (so a doc that mentions the term
    // in a heading but no body still scores something).
    for t in terms {
        total += (lower.matches(t.as_str()).count() as f64) * 0.1;
    }

    if total <= 0.0 {
        return None;
    }

    let lines: Vec<&str> = source.content.lines().collect();
    let context = match best_line {
        Some((i, _)) => {
            let start = i.saturating_sub(1);
            let end = (i + 2).min(lines.len());
            lines[start..end].iter().map(|s| s.to_string()).collect()
        }
        None => Vec::new(),
    };

    Some(DocHit {
        path: source.path.to_string_lossy().into_owned(),
        kind: format!("{:?}", source.kind).to_ascii_lowercase(),
        score: total,
        context,
    })
}
