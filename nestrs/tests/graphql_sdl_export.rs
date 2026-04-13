#![cfg(feature = "graphql")]

use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema};
use nestrs::graphql::export_schema_sdl;

#[derive(Default)]
struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn version(&self) -> &str {
        "1"
    }
}

#[test]
fn export_schema_sdl_includes_query_type() {
    let schema = Schema::build(QueryRoot, EmptyMutation, EmptySubscription).finish();
    let sdl = export_schema_sdl(&schema);
    assert!(sdl.contains("type Query"));
    assert!(sdl.contains("version"));
}
