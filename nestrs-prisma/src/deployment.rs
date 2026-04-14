//! Deployment helpers for Prisma schema/database rollout in CI/CD.
//!
//! Focus:
//! - `prisma migrate deploy` for relational databases
//! - `prisma db push` fallback for MongoDB deployments
//! - GitHub Actions workflow generation with optional `pgfence` safety step

/// Supported package managers for command hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    fn exec_prefix(self) -> &'static str {
        match self {
            PackageManager::Npm => "npx",
            PackageManager::Pnpm => "pnpm dlx",
            PackageManager::Yarn => "yarn dlx",
            PackageManager::Bun => "bunx",
        }
    }
}

/// Database family relevant for migration deployment semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DatabaseKind {
    PostgreSql,
    MySql,
    SqlServer,
    Sqlite,
    CockroachDb,
    MongoDb,
}

/// Deployment command strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeployCommand {
    /// Relational deploy command: `prisma migrate deploy`.
    MigrateDeploy(String),
    /// Mongo deploy strategy: `prisma db push`.
    DbPush(String),
}

impl DeployCommand {
    pub fn as_str(&self) -> &str {
        match self {
            DeployCommand::MigrateDeploy(s) | DeployCommand::DbPush(s) => s,
        }
    }
}

/// Builds the right deployment command for the target database.
///
/// - Relational DBs => `prisma migrate deploy`
/// - MongoDB => `prisma db push`
pub fn prisma_deploy_command(package_manager: PackageManager, db: DatabaseKind) -> DeployCommand {
    let prefix = package_manager.exec_prefix();
    if db == DatabaseKind::MongoDb {
        DeployCommand::DbPush(format!("{prefix} prisma db push"))
    } else {
        DeployCommand::MigrateDeploy(format!("{prefix} prisma migrate deploy"))
    }
}

/// Optional migration safety checks with `pgfence` before deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PgfenceOptions {
    pub enabled: bool,
    /// One of: `low`, `medium`, `high`.
    pub max_risk: &'static str,
}

impl Default for PgfenceOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            max_risk: "medium",
        }
    }
}

/// Generates a GitHub Actions deployment workflow for Prisma migrations.
///
/// The generated workflow:
/// - triggers on `main` pushes touching `prisma/migrations/**`
/// - installs dependencies
/// - optionally runs pgfence
/// - runs `prisma migrate deploy` (or `db push` for MongoDB)
pub fn github_actions_deploy_workflow(
    package_manager: PackageManager,
    db: DatabaseKind,
    pgfence: PgfenceOptions,
) -> String {
    let install_cmd = match package_manager {
        PackageManager::Npm => "npm install",
        PackageManager::Pnpm => "pnpm install --frozen-lockfile",
        PackageManager::Yarn => "yarn install --frozen-lockfile",
        PackageManager::Bun => "bun install --frozen-lockfile",
    };

    let deploy = prisma_deploy_command(package_manager, db);
    let deploy_step_name = match deploy {
        DeployCommand::MigrateDeploy(_) => "Apply all pending migrations to the database",
        DeployCommand::DbPush(_) => "Push schema changes to the database (MongoDB)",
    };

    let mut extra_steps = String::new();
    if pgfence.enabled && matches!(db, DatabaseKind::PostgreSql | DatabaseKind::CockroachDb) {
        let prefix = package_manager.exec_prefix();
        extra_steps.push_str(&format!(
            "      - name: Run migration safety check\n        run: {prefix} @flvmnt/pgfence analyze --ci --max-risk {} prisma/migrations/**/migration.sql\n",
            pgfence.max_risk
        ));
    }

    format!(
        "name: Deploy\n\
         on:\n\
           push:\n\
             paths:\n\
               - prisma/migrations/**\n\
             branches:\n\
               - main\n\
         jobs:\n\
           deploy:\n\
             runs-on: ubuntu-latest\n\
             steps:\n\
               - name: Checkout repo\n\
                 uses: actions/checkout@v4\n\
               - name: Setup Node\n\
                 uses: actions/setup-node@v4\n\
               - name: Install dependencies\n\
                 run: {install_cmd}\n\
         {extra_steps}      - name: {deploy_step_name}\n\
                 run: {deploy_cmd}\n\
                 env:\n\
                   DATABASE_URL: ${{{{ secrets.DATABASE_URL }}}}\n",
        deploy_cmd = deploy.as_str()
    )
}

/// Basic source-control safety check from Prisma deploy guidance.
pub fn migration_folder_should_be_committed(path: &str) -> bool {
    path.ends_with("prisma/migrations") || path.ends_with("prisma/migrations/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relational_deploy_uses_migrate_deploy() {
        let cmd = prisma_deploy_command(PackageManager::Npm, DatabaseKind::PostgreSql);
        assert!(matches!(cmd, DeployCommand::MigrateDeploy(_)));
        assert_eq!(cmd.as_str(), "npx prisma migrate deploy");
    }

    #[test]
    fn mongodb_deploy_uses_db_push() {
        let cmd = prisma_deploy_command(PackageManager::Pnpm, DatabaseKind::MongoDb);
        assert!(matches!(cmd, DeployCommand::DbPush(_)));
        assert_eq!(cmd.as_str(), "pnpm dlx prisma db push");
    }

    #[test]
    fn workflow_contains_deploy_and_secret() {
        let wf = github_actions_deploy_workflow(
            PackageManager::Npm,
            DatabaseKind::PostgreSql,
            PgfenceOptions::default(),
        );
        assert!(wf.contains("prisma/migrations/**"));
        assert!(wf.contains("npx prisma migrate deploy"));
        assert!(wf.contains("DATABASE_URL: ${{ secrets.DATABASE_URL }}"));
    }

    #[test]
    fn workflow_can_include_pgfence_for_postgres() {
        let wf = github_actions_deploy_workflow(
            PackageManager::Yarn,
            DatabaseKind::PostgreSql,
            PgfenceOptions {
                enabled: true,
                max_risk: "medium",
            },
        );
        assert!(wf.contains("@flvmnt/pgfence analyze"));
        assert!(wf.contains("--max-risk medium"));
        assert!(wf.contains("yarn dlx prisma migrate deploy"));
    }

    #[test]
    fn migration_folder_check_is_strict() {
        assert!(migration_folder_should_be_committed("prisma/migrations"));
        assert!(!migration_folder_should_be_committed("prisma/migration"));
    }
}
