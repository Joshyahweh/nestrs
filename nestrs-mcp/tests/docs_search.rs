//! Docs search integration test. Builds a `DocStore` over a tiny
//! fixture workspace, then runs `DocSearcher::search` and asserts the
//! expected hit shape.

use std::fs;
use std::path::Path;

use nestrs_mcp::docs::{DocSearcher, DocStore, SearchScope};

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

#[test]
fn finds_hits_in_changelog_and_readme() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    write(
        root,
        "CHANGELOG.md",
        "# Changelog\n\n## 0.4.0 — sentinel\n\n- added sentinel middleware\n- added admin port\n",
    );
    write(
        root,
        "README.md",
        "# Sample\n\nA small project for testing the docs search index.\n",
    );
    write(
        root,
        "docs/src/book/intro.md",
        "# Intro\n\nWelcome. This page mentions the **sentinel** pattern.\n",
    );

    let store = DocStore::new();
    let n = store.build(root).expect("build should succeed");
    assert!(n >= 3, "expected at least 3 indexed files, got {n}");

    let sources = store.sources();
    let hits = DocSearcher::search(&sources, "sentinel", SearchScope::All, 10);
    assert!(!hits.is_empty(), "expected at least one hit for 'sentinel'");
    // The top hit should mention sentinel.
    assert!(
        hits.iter().any(|h| h.context.iter().any(|l| l.contains("sentinel"))),
        "expected at least one hit context line containing 'sentinel'"
    );
}
