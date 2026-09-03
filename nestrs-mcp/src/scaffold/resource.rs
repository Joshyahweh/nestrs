//! `create_resource`: DTO + controller + service + module, wired up.

use std::fs;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Result;

use super::dto::{create_dto, DtoFieldSpec};
use super::module::create_module;
use super::ScaffoldReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResourceTransport {
    Http,
    Graphql,
    Ws,
    Tcp,
}

pub fn create_resource(
    path: &Path,
    name: &str,
    dto_fields: &[DtoFieldSpec],
    transport: ResourceTransport,
) -> Result<ScaffoldReport> {
    let mut report = ScaffoldReport::new();
    // 1. DTO
    let dto_report = create_dto(path, name, dto_fields)?;
    for f in dto_report.files_created {
        report.created(f);
    }
    // 2. Module (controller + service)
    let transport_str = match transport {
        ResourceTransport::Http => "http",
        ResourceTransport::Graphql => "graphql",
        ResourceTransport::Ws => "ws",
        ResourceTransport::Tcp => "tcp",
    };
    let module_report = create_module(path, name, &[transport_str.into()])?;
    for f in module_report.files_created {
        report.created(f);
    }
    for f in module_report.files_modified {
        report.modified(f);
    }
    // 3. Drop a small `resource.rs` note in the module dir pointing at
    // the generated DTO + service.
    let note_path = path.join("src").join(name).join("resource.rs");
    let note = render_resource_note(name, transport);
    fs::write(&note_path, &note)?;
    report.created(note_path.to_string_lossy());
    Ok(report)
}

fn render_resource_note(name: &str, transport: ResourceTransport) -> String {
    let transport_name = match transport {
        ResourceTransport::Http => "REST (HTTP)",
        ResourceTransport::Graphql => "GraphQL",
        ResourceTransport::Ws => "WebSocket",
        ResourceTransport::Tcp => "TCP microservice",
    };
    format!(
        r#"//! `{name}` resource scaffold — transport: {transport_name}.
//!
//! Wire this module into your root module:
//!
//! ```ignore
//! #[derive(Default, Module)]
//! #[module(imports = [{name}Module])]
//! pub struct AppModule;
//! ```

pub use super::controller::{{ create_{name}, list_{name} }};
pub use super::service::{name}Service;
"#
    )
}
