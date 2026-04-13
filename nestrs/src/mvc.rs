//! Server-rendered HTML via [MiniJinja](https://github.com/mitsuhiko/minijinja) (feature: **`mvc`**).

use crate::{injectable, module};
use minijinja::Environment;
use std::path::Path;
use std::sync::{Arc, OnceLock};

static MVC_ENV: OnceLock<Arc<Environment<'static>>> = OnceLock::new();

fn load_templates_from_dir(dir: &Path) -> Result<Environment<'static>, std::io::Error> {
    let mut env = Environment::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "html" | "htm" | "j2" | "jinja" | "mjinja") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad template name")
        })?;
        let src = std::fs::read_to_string(&path)?;
        env.add_template_owned(name.to_string(), src)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    }
    Ok(env)
}

/// Renders named templates registered by [`MvcModule::for_root`].
#[injectable]
pub struct MvcService;

impl MvcService {
    pub fn render(
        &self,
        name: &str,
        ctx: impl serde::Serialize,
    ) -> Result<String, minijinja::Error> {
        let env = MVC_ENV.get().ok_or_else(|| {
            minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "MvcModule::for_root must be called before MvcService::render",
            )
        })?;
        env.get_template(name)?.render(ctx)
    }

    /// Same as [`Self::render`], wrapped as Axum [`axum::response::Html`].
    pub fn render_html(
        &self,
        name: &str,
        ctx: impl serde::Serialize,
    ) -> Result<axum::response::Html<String>, minijinja::Error> {
        Ok(axum::response::Html(self.render(name, ctx)?))
    }
}

#[module(providers = [MvcService], exports = [MvcService])]
pub struct MvcModule;

impl MvcModule {
    /// Loads `*.html`, `*.htm`, `*.j2`, `*.jinja`, `*.mjinja` from `dir` into the template environment.
    pub fn for_root(dir: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        let env = load_templates_from_dir(dir.as_ref())?;
        let _ = MVC_ENV.set(Arc::new(env));
        Ok(Self)
    }
}
