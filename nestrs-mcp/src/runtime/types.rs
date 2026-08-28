//! Response shapes for the admin port. Mirrored here so we can decode
//! the wire JSON without depending on the `nestrs` crate (which would
//! create a heavier dep graph than necessary for v1).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::introspection::registry::{LiveProviderSummary, LiveRouteSummary};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AdminHealth {
    pub status: String,
    pub uptime_ms: u64,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct AdminProviders(pub Vec<LiveProviderSummary>);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct AdminRoutes(pub Vec<LiveRouteSummary>);
