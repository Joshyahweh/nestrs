#![cfg(feature = "mvc")]

use nestrs::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("nestrs-{label}-{unique}"));
        fs::create_dir_all(&path).expect("temp dir should be creatable");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn mvc_second_for_root_call_must_not_succeed_with_stale_templates() {
    let first = TempDir::new("mvc-first");
    let second = TempDir::new("mvc-second");
    fs::write(first.path().join("index.html"), "first").expect("first template should be written");
    fs::write(second.path().join("index.html"), "second")
        .expect("second template should be written");

    MvcModule::for_root(first.path()).expect("first root should load");
    let service = MvcService;
    assert_eq!(
        service
            .render("index.html", ())
            .expect("first template should render"),
        "first"
    );

    match MvcModule::for_root(second.path()) {
        Err(err) => {
            assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
            assert_eq!(
                err.to_string(),
                "MvcModule::for_root has already been called for this process"
            );
        }
        Ok(_) => {
            let rendered = service
                .render("index.html", ())
                .expect("template should render after second root call");
            assert_eq!(
                rendered,
                "second",
                "returning Ok from a second for_root call must not leave the first template directory active"
            );
        }
    }
}
