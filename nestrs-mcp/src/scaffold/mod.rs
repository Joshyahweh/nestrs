//! Scaffolding tools: `new_project`, `create_module`, `create_resource`,
//! `create_dto`, `generate_crud`. All write actions return a structured
//! `{ files_created, files_modified }` report so the model can show the
//! user exactly what changed.

pub mod crud;
pub mod dto;
pub mod module;
pub mod project;
pub mod resource;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What a scaffolding tool returns: a list of files created and a list
/// of files modified, so the model can summarize and the user can audit.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ScaffoldReport {
    pub files_created: Vec<String>,
    pub files_modified: Vec<String>,
}

impl ScaffoldReport {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn created(&mut self, path: impl Into<String>) {
        self.files_created.push(path.into());
    }
    pub fn modified(&mut self, path: impl Into<String>) {
        self.files_modified.push(path.into());
    }
}
