//! `generate_crud`: a resource across multiple transports in one call.
//! Composes `create_resource` per transport.

use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Result;

use super::dto::DtoFieldSpec;
use super::resource::{create_resource, ResourceTransport};
use super::ScaffoldReport;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CrudSpec {
    pub resource: String,
    pub fields: Vec<DtoFieldSpec>,
    /// One entry per transport to generate.
    pub transports: Vec<ResourceTransport>,
}

pub fn generate_crud(path: &Path, spec: &CrudSpec) -> Result<ScaffoldReport> {
    let mut combined = ScaffoldReport::new();
    for t in &spec.transports {
        let r = create_resource(path, &spec.resource, &spec.fields, *t)?;
        for f in r.files_created {
            // The DTO is created once per transport; mark it as a
            // duplicate so the model can dedupe in its summary.
            if combined.files_created.contains(&f) {
                continue;
            }
            combined.created(f);
        }
        for f in r.files_modified {
            if !combined.files_modified.contains(&f) {
                combined.modified(f);
            }
        }
    }
    Ok(combined)
}
