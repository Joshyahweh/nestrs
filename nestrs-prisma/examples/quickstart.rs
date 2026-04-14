use std::path::Path;
use std::sync::Arc;

use nestrs_prisma::schema_bridge::SchemaSyncOptions;
use nestrs_prisma::{
    prisma_db_push_command, prisma_generate_command, prisma_migrate_deploy_command, prisma_model,
    PrismaModule, PrismaOptions, PrismaService,
};

prisma_model!(User => "users", {
    id: i64,
    email: String,
    name: String,
});

prisma_model!(Post => "posts", {
    id: i64,
    title: String,
    author_id: i64,
});

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:quickstart.db".to_string());
    let schema_path = "prisma/schema.prisma";

    let _ = PrismaModule::for_root_with_options(
        PrismaOptions::from_url(database_url.clone())
            .pool_min(1)
            .pool_max(5)
            .schema_path(schema_path),
    );
    let prisma = Arc::new(PrismaService::default());

    println!("Generate command: {}", prisma_generate_command(schema_path));
    println!(
        "Relational deploy command: {}",
        prisma_migrate_deploy_command()
    );
    println!("Mongo deploy command: {}", prisma_db_push_command());

    if Path::new(schema_path).exists() {
        let report = prisma
            .sync_from_prisma_schema(
                schema_path,
                SchemaSyncOptions {
                    output_file: "src/models/prisma_generated.rs".to_string(),
                    max_identifier_len: 63,
                    apply_foreign_keys: false,
                },
            )
            .await?;
        println!(
            "Schema sync: models={}, relations={}, generated={}",
            report.model_count, report.relation_count, report.written_file
        );
        if !report.warnings.is_empty() {
            println!("Validation warnings:");
            for w in &report.warnings {
                println!("  - {w}");
            }
        }
    } else {
        println!("No prisma/schema.prisma found; skipped schema sync.");
    }

    // Demo CRUD only when using SQLite quickstart DB so this example can run out-of-the-box.
    if database_url.starts_with("sqlite:") {
        prisma
            .execute(
                r#"
CREATE TABLE IF NOT EXISTS "users" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "email" TEXT NOT NULL UNIQUE,
  "name" TEXT NOT NULL
)
"#,
            )
            .await?;
        prisma
            .execute(
                r#"
CREATE TABLE IF NOT EXISTS "posts" (
  "id" INTEGER PRIMARY KEY AUTOINCREMENT,
  "title" TEXT NOT NULL,
  "author_id" INTEGER NOT NULL,
  CONSTRAINT "posts_author_fkey" FOREIGN KEY ("author_id") REFERENCES "users"("id") ON DELETE CASCADE ON UPDATE CASCADE
)
"#,
            )
            .await?;

        let user = prisma
            .user()
            .create(UserCreateInput {
                email: "demo@example.com".into(),
                name: "Demo User".into(),
            })
            .await?;

        let _post = prisma
            .post()
            .create(PostCreateInput {
                title: "Hello from nestrs-prisma".into(),
                author_id: user.id,
            })
            .await?;

        let fetched = prisma
            .user()
            .find_unique(user::email::equals("demo@example.com".into()))
            .await?;
        let posts = prisma
            .post()
            .find_many(PostWhere::and(vec![post::author_id::equals(user.id)]))
            .await?;

        println!("User: {:?}", fetched.map(|u| (u.id, u.email, u.name)));
        println!("Posts count for user {}: {}", user.id, posts.len());
    } else {
        println!("Non-SQLite DATABASE_URL detected; skipping local CRUD bootstrap demo.");
    }

    Ok(())
}
