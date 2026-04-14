//! Prisma-style relation modeling and validation helpers.
//!
//! This module is schema-level infrastructure for relation semantics:
//! - 1-1 / 1-n / m-n (implicit and explicit) relation definitions
//! - self-relation and relation-name validation
//! - referential action defaults + connector/mode support checks
//! - relation mode (`foreignKeys` vs `prisma`) index recommendations
//! - SQL foreign-key DDL rendering (when relation mode is `foreignKeys`)
//!
//! It does not parse `schema.prisma`; callers construct definitions in Rust.

use crate::index_ddl::SqlDialect;
use crate::mapping::{prisma_default_constraint_name, ConstraintKind};
use std::collections::{BTreeMap, HashMap};

/// Prisma relation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationMode {
    ForeignKeys,
    Prisma,
}

/// Prisma relation cardinalities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationKind {
    OneToOne,
    OneToMany,
    ManyToManyImplicit,
    ManyToManyExplicit,
}

/// Referential actions supported by Prisma semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReferentialAction {
    Cascade,
    Restrict,
    NoAction,
    SetNull,
    SetDefault,
}

impl ReferentialAction {
    pub fn as_sql(self) -> &'static str {
        match self {
            ReferentialAction::Cascade => "CASCADE",
            ReferentialAction::Restrict => "RESTRICT",
            ReferentialAction::NoAction => "NO ACTION",
            ReferentialAction::SetNull => "SET NULL",
            ReferentialAction::SetDefault => "SET DEFAULT",
        }
    }
}

/// Default referential action pair (`onDelete`, `onUpdate`) from Prisma docs.
///
/// - Optional relation: `onDelete SetNull`, `onUpdate Cascade`
/// - Mandatory relation: `onDelete Restrict`, `onUpdate Cascade`
pub fn default_referential_actions(
    optional_relation_scalar: bool,
) -> (ReferentialAction, ReferentialAction) {
    if optional_relation_scalar {
        (ReferentialAction::SetNull, ReferentialAction::Cascade)
    } else {
        (ReferentialAction::Restrict, ReferentialAction::Cascade)
    }
}

/// One side of a relation field pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationEndpoint {
    /// Model name that owns this relation field.
    pub model: String,
    /// Prisma relation field name (ORM-level field).
    pub relation_field: String,
    /// Relation scalar FK columns on this side (`fields: [...]`).
    pub scalar_fields: Vec<String>,
    /// Referenced columns on the opposite model (`references: [...]`).
    pub referenced_fields: Vec<String>,
    /// Whether the relation field type is optional (`User?`).
    pub optional: bool,
    /// Whether the relation field is list (`Post[]`).
    pub list: bool,
    /// Whether this endpoint stores the FK (`@relation(fields, references)` side).
    pub annotated: bool,
    /// Whether scalar fields are covered by a unique/PK constraint.
    pub scalar_unique: bool,
    /// Whether scalar fields have an index (important for `relationMode = "prisma"`).
    pub scalar_indexed: bool,
}

impl RelationEndpoint {
    pub fn new(model: impl Into<String>, relation_field: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            relation_field: relation_field.into(),
            scalar_fields: Vec::new(),
            referenced_fields: Vec::new(),
            optional: false,
            list: false,
            annotated: false,
            scalar_unique: false,
            scalar_indexed: false,
        }
    }

    pub fn scalar(mut self, fields: Vec<&str>, refs: Vec<&str>) -> Self {
        self.scalar_fields = fields.into_iter().map(str::to_owned).collect();
        self.referenced_fields = refs.into_iter().map(str::to_owned).collect();
        self.annotated = true;
        self
    }

    pub fn optional(mut self, v: bool) -> Self {
        self.optional = v;
        self
    }

    pub fn list(mut self, v: bool) -> Self {
        self.list = v;
        self
    }

    pub fn unique(mut self, v: bool) -> Self {
        self.scalar_unique = v;
        self
    }

    pub fn indexed(mut self, v: bool) -> Self {
        self.scalar_indexed = v;
        self
    }
}

/// Explicit many-to-many join model metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinModel {
    pub model: String,
    pub has_primary_key: bool,
    pub left_back_relation_present: bool,
    pub right_back_relation_present: bool,
    pub extra_fields: Vec<String>,
}

impl JoinModel {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            has_primary_key: true,
            left_back_relation_present: true,
            right_back_relation_present: true,
            extra_fields: Vec::new(),
        }
    }
}

/// Complete relation definition (two endpoints + relation-level options).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationDefinition {
    pub kind: RelationKind,
    pub name: Option<String>,
    pub left: RelationEndpoint,
    pub right: RelationEndpoint,
    pub on_delete: Option<ReferentialAction>,
    pub on_update: Option<ReferentialAction>,
    pub join_model: Option<JoinModel>,
    /// Optional implicit m-n table label (`@relation("...")` on both sides).
    pub implicit_relation_table_name: Option<String>,
}

impl RelationDefinition {
    pub fn new(kind: RelationKind, left: RelationEndpoint, right: RelationEndpoint) -> Self {
        Self {
            kind,
            name: None,
            left,
            right,
            on_delete: None,
            on_update: None,
            join_model: None,
            implicit_relation_table_name: None,
        }
    }

    pub fn name(mut self, n: impl Into<String>) -> Self {
        self.name = Some(n.into());
        self
    }

    pub fn on_delete(mut self, a: ReferentialAction) -> Self {
        self.on_delete = Some(a);
        self
    }

    pub fn on_update(mut self, a: ReferentialAction) -> Self {
        self.on_update = Some(a);
        self
    }

    pub fn join_model(mut self, join: JoinModel) -> Self {
        self.join_model = Some(join);
        self
    }

    pub fn implicit_relation_table_name(mut self, table_name: impl Into<String>) -> Self {
        self.implicit_relation_table_name = Some(table_name.into());
        self
    }

    pub fn is_self_relation(&self) -> bool {
        self.left.model == self.right.model
    }

    pub fn resolved_on_delete(&self, optional_fk: bool) -> ReferentialAction {
        self.on_delete
            .unwrap_or_else(|| default_referential_actions(optional_fk).0)
    }

    pub fn resolved_on_update(&self, optional_fk: bool) -> ReferentialAction {
        self.on_update
            .unwrap_or_else(|| default_referential_actions(optional_fk).1)
    }
}

/// Model metadata needed for validation of implicit m-n requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelMetadata {
    pub name: String,
    pub single_id: bool,
}

/// Schema-level inputs for relation validation.
#[derive(Debug, Clone)]
pub struct RelationSchema {
    pub relation_mode: RelationMode,
    pub dialect: SqlDialect,
    pub models: HashMap<String, ModelMetadata>,
    pub relations: Vec<RelationDefinition>,
}

impl RelationSchema {
    pub fn new(relation_mode: RelationMode, dialect: SqlDialect) -> Self {
        Self {
            relation_mode,
            dialect,
            models: HashMap::new(),
            relations: Vec::new(),
        }
    }

    pub fn model(mut self, model: ModelMetadata) -> Self {
        self.models.insert(model.name.clone(), model);
        self
    }

    pub fn relation(mut self, rel: RelationDefinition) -> Self {
        self.relations.push(rel);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRecommendation {
    pub model: String,
    pub scalar_fields: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationValidationReport {
    pub warnings: Vec<String>,
    pub index_recommendations: Vec<IndexRecommendation>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RelationValidationError {
    #[error("relation `{0}` must define exactly one annotated side with scalar fields/references")]
    MissingAnnotatedSide(String),
    #[error("relation `{0}` has mismatched scalar fields/references lengths")]
    ScalarReferenceLengthMismatch(String),
    #[error("one-to-one relation `{0}` requires FK side scalar fields to be unique")]
    OneToOneRequiresUnique(String),
    #[error(
        "one-to-one relation `{0}` requires one side to be optional when both are relation fields"
    )]
    OneToOneOptionalityInvalid(String),
    #[error("one-to-many relation `{0}` requires one list side and one non-list side")]
    OneToManyShapeInvalid(String),
    #[error("implicit many-to-many relation `{0}` requires both models to have a single @id")]
    ImplicitManyToManyNeedsSingleId(String),
    #[error(
        "implicit many-to-many relation `{0}` cannot define scalar/references/onDelete/onUpdate"
    )]
    ImplicitManyToManyAnnotatedInvalid(String),
    #[error("explicit many-to-many relation `{0}` requires a join model")]
    ExplicitManyToManyJoinMissing(String),
    #[error(
        "explicit many-to-many relation `{0}` join model must have back-relations on both sides"
    )]
    ExplicitManyToManyBackRelationMissing(String),
    #[error("explicit many-to-many relation `{0}` join model must have a primary key")]
    ExplicitManyToManyPrimaryKeyMissing(String),
    #[error("self relation `{0}` must set a relation name on both sides")]
    SelfRelationRequiresName(String),
    #[error("referential action `{action:?}` on `{relation}` is unsupported for dialect `{dialect:?}` and mode `{mode:?}`")]
    UnsupportedReferentialAction {
        relation: String,
        action: ReferentialAction,
        dialect: SqlDialect,
        mode: RelationMode,
    },
    #[error("`SetNull` on required relation in `{0}` is invalid")]
    SetNullRequiredInvalid(String),
    #[error("SQL Server does not support `Restrict`; use `NoAction` in relation `{0}`")]
    RestrictNotSupportedSqlServer(String),
    #[error("self relation `{0}` on SQL Server/Mongo-like semantics must set onDelete and onUpdate to NoAction on one side")]
    SelfRelationCycleNoActionRequired(String),
    #[error("cascade cycle detected for `{0}`; one relation in the cycle must use NoAction")]
    CascadeCycleDetected(String),
    #[error("multiple cascade paths detected from `{from}` to `{to}` (SQL Server rule)")]
    MultipleCascadePaths { from: String, to: String },
}

fn relation_label(r: &RelationDefinition) -> String {
    r.name.clone().unwrap_or_else(|| {
        format!(
            "{}::{}<->{}::{}",
            r.left.model, r.left.relation_field, r.right.model, r.right.relation_field
        )
    })
}

fn supports_action(dialect: SqlDialect, mode: RelationMode, action: ReferentialAction) -> bool {
    use ReferentialAction::*;
    if mode == RelationMode::Prisma {
        // Prisma relationMode emulation matrix from docs.
        return match dialect {
            SqlDialect::PostgreSql | SqlDialect::Sqlite => !matches!(action, NoAction | SetDefault),
            SqlDialect::MySql | SqlDialect::SqlServer | SqlDialect::CockroachDb => {
                !matches!(action, SetDefault)
            }
        };
    }

    match dialect {
        SqlDialect::PostgreSql | SqlDialect::Sqlite | SqlDialect::CockroachDb => true,
        SqlDialect::MySql => !matches!(action, SetDefault),
        SqlDialect::SqlServer => !matches!(action, Restrict),
    }
}

fn quote_ident(dialect: SqlDialect, id: &str) -> String {
    match dialect {
        SqlDialect::PostgreSql | SqlDialect::Sqlite | SqlDialect::CockroachDb => {
            format!("\"{}\"", id.replace('"', "\"\""))
        }
        SqlDialect::MySql => format!("`{}`", id.replace('`', "``")),
        SqlDialect::SqlServer => format!("[{}]", id.replace(']', "]]")),
    }
}

fn build_cascade_graph(schema: &RelationSchema) -> HashMap<String, Vec<String>> {
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for rel in &schema.relations {
        if matches!(rel.kind, RelationKind::ManyToManyImplicit) {
            continue;
        }
        // Cascade flows from referenced side to FK side.
        let (fk_side, parent_side) = if rel.left.annotated {
            (&rel.left, &rel.right)
        } else {
            (&rel.right, &rel.left)
        };
        let on_delete = rel.resolved_on_delete(fk_side.optional);
        let on_update = rel.resolved_on_update(fk_side.optional);
        if on_delete == ReferentialAction::Cascade || on_update == ReferentialAction::Cascade {
            graph
                .entry(parent_side.model.clone())
                .or_default()
                .push(fk_side.model.clone());
        }
    }
    graph
}

fn has_cycle(graph: &HashMap<String, Vec<String>>) -> bool {
    fn dfs(
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        visiting: &mut BTreeMap<String, bool>,
        visited: &mut BTreeMap<String, bool>,
    ) -> bool {
        if *visiting.get(node).unwrap_or(&false) {
            return true;
        }
        if *visited.get(node).unwrap_or(&false) {
            return false;
        }
        visiting.insert(node.to_string(), true);
        if let Some(nexts) = graph.get(node) {
            for n in nexts {
                if dfs(n, graph, visiting, visited) {
                    return true;
                }
            }
        }
        visiting.insert(node.to_string(), false);
        visited.insert(node.to_string(), true);
        false
    }

    let mut visiting = BTreeMap::new();
    let mut visited = BTreeMap::new();
    for node in graph.keys() {
        if dfs(node, graph, &mut visiting, &mut visited) {
            return true;
        }
    }
    false
}

fn count_paths_limited(
    graph: &HashMap<String, Vec<String>>,
    from: &str,
    to: &str,
    visited: &mut Vec<String>,
    limit: usize,
) -> usize {
    if from == to {
        return 1;
    }
    if visited.iter().any(|v| v == from) {
        return 0;
    }
    visited.push(from.to_string());
    let mut total = 0usize;
    if let Some(nexts) = graph.get(from) {
        for n in nexts {
            total += count_paths_limited(graph, n, to, visited, limit);
            if total >= limit {
                visited.pop();
                return total;
            }
        }
    }
    visited.pop();
    total
}

/// Validates all relation definitions against Prisma-style rules.
pub fn validate_relations(
    schema: &RelationSchema,
) -> Result<RelationValidationReport, RelationValidationError> {
    let mut warnings = Vec::new();
    let mut index_recommendations = Vec::new();

    for rel in &schema.relations {
        let label = relation_label(rel);
        let annotated_count = rel.left.annotated as usize + rel.right.annotated as usize;

        match rel.kind {
            RelationKind::OneToOne | RelationKind::OneToMany => {
                if annotated_count != 1 {
                    return Err(RelationValidationError::MissingAnnotatedSide(label));
                }
            }
            RelationKind::ManyToManyExplicit => {}
            RelationKind::ManyToManyImplicit => {
                if annotated_count != 0
                    || rel.on_delete.is_some()
                    || rel.on_update.is_some()
                    || !rel.left.scalar_fields.is_empty()
                    || !rel.right.scalar_fields.is_empty()
                {
                    return Err(RelationValidationError::ImplicitManyToManyAnnotatedInvalid(
                        label,
                    ));
                }
            }
        }

        let sides = if rel.left.annotated {
            Some((&rel.left, &rel.right))
        } else if rel.right.annotated {
            Some((&rel.right, &rel.left))
        } else {
            None
        };

        if matches!(rel.kind, RelationKind::OneToOne | RelationKind::OneToMany) {
            let (fk_side, _other_side) = sides.expect("validated annotated side exists");
            if fk_side.scalar_fields.len() != fk_side.referenced_fields.len()
                || fk_side.scalar_fields.is_empty()
            {
                return Err(RelationValidationError::ScalarReferenceLengthMismatch(
                    label,
                ));
            }
        }

        if rel.is_self_relation() && rel.name.is_none() {
            return Err(RelationValidationError::SelfRelationRequiresName(label));
        }

        match rel.kind {
            RelationKind::OneToOne => {
                let (fk_side, other_side) = sides.expect("validated annotated side exists");
                if !fk_side.scalar_unique {
                    return Err(RelationValidationError::OneToOneRequiresUnique(label));
                }
                // Side without relation scalar must be optional in Prisma.
                if !other_side.optional && !fk_side.optional {
                    return Err(RelationValidationError::OneToOneOptionalityInvalid(label));
                }
            }
            RelationKind::OneToMany => {
                if !(rel.left.list ^ rel.right.list) {
                    return Err(RelationValidationError::OneToManyShapeInvalid(label));
                }
            }
            RelationKind::ManyToManyImplicit => {
                let left = schema.models.get(&rel.left.model);
                let right = schema.models.get(&rel.right.model);
                if !left.map(|m| m.single_id).unwrap_or(false)
                    || !right.map(|m| m.single_id).unwrap_or(false)
                {
                    return Err(RelationValidationError::ImplicitManyToManyNeedsSingleId(
                        label,
                    ));
                }
                if rel.is_self_relation() && rel.left.relation_field >= rel.right.relation_field {
                    warnings.push(format!(
                        "implicit self m-n `{}`: keep lexicographic field ordering stable to avoid A/B join-column semantic drift",
                        relation_label(rel)
                    ));
                }
            }
            RelationKind::ManyToManyExplicit => {
                let Some(join) = &rel.join_model else {
                    return Err(RelationValidationError::ExplicitManyToManyJoinMissing(
                        label,
                    ));
                };
                if !join.has_primary_key {
                    return Err(
                        RelationValidationError::ExplicitManyToManyPrimaryKeyMissing(label),
                    );
                }
                if !join.left_back_relation_present || !join.right_back_relation_present {
                    return Err(
                        RelationValidationError::ExplicitManyToManyBackRelationMissing(label),
                    );
                }
            }
        }

        if matches!(rel.kind, RelationKind::OneToOne | RelationKind::OneToMany) {
            let (fk_side, _other_side) = sides.expect("validated annotated side exists");
            let on_delete = rel.resolved_on_delete(fk_side.optional);
            let on_update = rel.resolved_on_update(fk_side.optional);
            for a in [on_delete, on_update] {
                if !supports_action(schema.dialect, schema.relation_mode, a) {
                    return Err(RelationValidationError::UnsupportedReferentialAction {
                        relation: relation_label(rel),
                        action: a,
                        dialect: schema.dialect,
                        mode: schema.relation_mode,
                    });
                }
            }

            if schema.dialect == SqlDialect::SqlServer
                && (on_delete == ReferentialAction::Restrict
                    || on_update == ReferentialAction::Restrict)
            {
                return Err(RelationValidationError::RestrictNotSupportedSqlServer(
                    label,
                ));
            }

            if (on_delete == ReferentialAction::SetNull || on_update == ReferentialAction::SetNull)
                && !fk_side.optional
            {
                // Prisma warns on PostgreSQL, rejects elsewhere; we keep strict.
                return Err(RelationValidationError::SetNullRequiredInvalid(label));
            }

            if schema.relation_mode == RelationMode::Prisma && !fk_side.scalar_indexed {
                index_recommendations.push(IndexRecommendation {
                    model: fk_side.model.clone(),
                    scalar_fields: fk_side.scalar_fields.clone(),
                    reason: "relationMode=prisma does not create FK indexes automatically"
                        .to_string(),
                });
            }

            if rel.is_self_relation()
                && matches!(schema.dialect, SqlDialect::SqlServer)
                && !matches!(on_delete, ReferentialAction::NoAction)
                && !matches!(on_update, ReferentialAction::NoAction)
            {
                return Err(RelationValidationError::SelfRelationCycleNoActionRequired(
                    label,
                ));
            }
        }
    }

    // Cascade cycle / multiple-path checks.
    let graph = build_cascade_graph(schema);
    if has_cycle(&graph) {
        return Err(RelationValidationError::CascadeCycleDetected(
            "at least one relation chain".to_string(),
        ));
    }

    if schema.dialect == SqlDialect::SqlServer {
        let nodes: Vec<String> = graph.keys().cloned().collect();
        for from in &nodes {
            for to in &nodes {
                if from == to {
                    continue;
                }
                let mut visited = Vec::new();
                let n = count_paths_limited(&graph, from, to, &mut visited, 2);
                if n >= 2 {
                    return Err(RelationValidationError::MultipleCascadePaths {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
            }
        }
    }

    Ok(RelationValidationReport {
        warnings,
        index_recommendations,
    })
}

/// Render SQL `ALTER TABLE ... ADD CONSTRAINT ... FOREIGN KEY ...` when applicable.
///
/// Returns `Ok(None)` when relation mode is `prisma` (no DB foreign key DDL).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKeyConstraintSqlInput<'a> {
    pub relation_mode: RelationMode,
    pub dialect: SqlDialect,
    pub table: &'a str,
    pub constraint_name: &'a str,
    pub fk_columns: &'a [&'a str],
    pub referenced_table: &'a str,
    pub referenced_columns: &'a [&'a str],
    pub on_delete: ReferentialAction,
    pub on_update: ReferentialAction,
}

pub fn foreign_key_constraint_sql(
    input: ForeignKeyConstraintSqlInput<'_>,
) -> Result<Option<String>, RelationValidationError> {
    if input.relation_mode == RelationMode::Prisma {
        return Ok(None);
    }
    if input.fk_columns.is_empty() || input.fk_columns.len() != input.referenced_columns.len() {
        return Err(RelationValidationError::ScalarReferenceLengthMismatch(
            input.constraint_name.to_string(),
        ));
    }
    for a in [input.on_delete, input.on_update] {
        if !supports_action(input.dialect, input.relation_mode, a) {
            return Err(RelationValidationError::UnsupportedReferentialAction {
                relation: input.constraint_name.to_string(),
                action: a,
                dialect: input.dialect,
                mode: input.relation_mode,
            });
        }
    }

    let t = quote_ident(input.dialect, input.table);
    let rt = quote_ident(input.dialect, input.referenced_table);
    let cn = quote_ident(input.dialect, input.constraint_name);
    let cols = input
        .fk_columns
        .iter()
        .map(|c| quote_ident(input.dialect, c))
        .collect::<Vec<_>>()
        .join(", ");
    let ref_cols = input
        .referenced_columns
        .iter()
        .map(|c| quote_ident(input.dialect, c))
        .collect::<Vec<_>>()
        .join(", ");

    Ok(Some(format!(
        "ALTER TABLE {t} ADD CONSTRAINT {cn} FOREIGN KEY ({cols}) REFERENCES {rt} ({ref_cols}) ON DELETE {} ON UPDATE {}",
        input.on_delete.as_sql(),
        input.on_update.as_sql(),
    )))
}

/// Complete relation deployment plan:
/// - validation warnings/index recommendations
/// - generated foreign-key DDL statements (if `foreignKeys` mode)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationDeploymentPlan {
    pub validation: RelationValidationReport,
    pub foreign_key_sql: Vec<String>,
}

/// Build a deployable relation plan with validation + FK DDL.
pub fn build_relation_deployment_plan(
    schema: &RelationSchema,
    max_identifier_len: usize,
) -> Result<RelationDeploymentPlan, RelationValidationError> {
    let validation = validate_relations(schema)?;
    let mut foreign_key_sql = Vec::new();

    if schema.relation_mode == RelationMode::Prisma {
        return Ok(RelationDeploymentPlan {
            validation,
            foreign_key_sql,
        });
    }

    for rel in &schema.relations {
        if !matches!(rel.kind, RelationKind::OneToOne | RelationKind::OneToMany) {
            continue;
        }
        let (fk_side, referenced_side) = if rel.left.annotated {
            (&rel.left, &rel.right)
        } else {
            (&rel.right, &rel.left)
        };

        let cols: Vec<&str> = fk_side.scalar_fields.iter().map(String::as_str).collect();
        let refs: Vec<&str> = fk_side
            .referenced_fields
            .iter()
            .map(String::as_str)
            .collect();
        let default_constraint = prisma_default_constraint_name(
            &fk_side.model,
            &cols,
            ConstraintKind::ForeignKey,
            max_identifier_len,
        );
        let relation_name = relation_label(rel);
        let constraint_name = rel
            .name
            .clone()
            .map(|n| format!("{n}_fkey"))
            .unwrap_or(default_constraint);

        let sql = foreign_key_constraint_sql(ForeignKeyConstraintSqlInput {
            relation_mode: schema.relation_mode,
            dialect: schema.dialect,
            table: &fk_side.model,
            constraint_name: &constraint_name,
            fk_columns: &cols,
            referenced_table: &referenced_side.model,
            referenced_columns: &refs,
            on_delete: rel.resolved_on_delete(fk_side.optional),
            on_update: rel.resolved_on_update(fk_side.optional),
        })?
        .ok_or_else(|| RelationValidationError::MissingAnnotatedSide(relation_name.clone()))?;

        foreign_key_sql.push(sql);
    }

    Ok(RelationDeploymentPlan {
        validation,
        foreign_key_sql,
    })
}

#[cfg(feature = "sqlx")]
impl crate::PrismaService {
    /// Apply generated foreign-key SQL statements sequentially.
    pub async fn apply_relation_deployment_plan(
        &self,
        plan: &RelationDeploymentPlan,
    ) -> Result<Vec<u64>, String> {
        let mut out = Vec::with_capacity(plan.foreign_key_sql.len());
        for stmt in &plan.foreign_key_sql {
            out.push(self.execute(stmt).await?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str) -> ModelMetadata {
        ModelMetadata {
            name: name.to_string(),
            single_id: true,
        }
    }

    #[test]
    fn one_to_one_requires_unique_fk() {
        let rel = RelationDefinition::new(
            RelationKind::OneToOne,
            RelationEndpoint::new("User", "profile").optional(true),
            RelationEndpoint::new("Profile", "user")
                .scalar(vec!["userId"], vec!["id"])
                .optional(false)
                .unique(false),
        );
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("User"))
            .model(model("Profile"))
            .relation(rel);
        let err = validate_relations(&schema).unwrap_err();
        assert!(matches!(
            err,
            RelationValidationError::OneToOneRequiresUnique(_)
        ));
    }

    #[test]
    fn one_to_many_shape_is_valid() {
        let rel = RelationDefinition::new(
            RelationKind::OneToMany,
            RelationEndpoint::new("User", "posts").list(true),
            RelationEndpoint::new("Post", "author")
                .scalar(vec!["authorId"], vec!["id"])
                .optional(false)
                .list(false),
        );
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("User"))
            .model(model("Post"))
            .relation(rel);
        let ok = validate_relations(&schema);
        assert!(ok.is_ok());
    }

    #[test]
    fn implicit_mn_requires_single_ids() {
        let mut m = model("Category");
        m.single_id = false;
        let rel = RelationDefinition::new(
            RelationKind::ManyToManyImplicit,
            RelationEndpoint::new("Post", "categories").list(true),
            RelationEndpoint::new("Category", "posts").list(true),
        );
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("Post"))
            .model(m)
            .relation(rel);
        let err = validate_relations(&schema).unwrap_err();
        assert!(matches!(
            err,
            RelationValidationError::ImplicitManyToManyNeedsSingleId(_)
        ));
    }

    #[test]
    fn explicit_mn_requires_join_pk_and_back_relations() {
        let rel = RelationDefinition::new(
            RelationKind::ManyToManyExplicit,
            RelationEndpoint::new("Post", "postCategories").list(true),
            RelationEndpoint::new("Category", "postCategories").list(true),
        )
        .join_model(JoinModel {
            model: "PostCategories".to_string(),
            has_primary_key: false,
            left_back_relation_present: true,
            right_back_relation_present: false,
            extra_fields: vec![],
        });
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("Post"))
            .model(model("Category"))
            .model(model("PostCategories"))
            .relation(rel);
        let err = validate_relations(&schema).unwrap_err();
        assert!(matches!(
            err,
            RelationValidationError::ExplicitManyToManyPrimaryKeyMissing(_)
        ));
    }

    #[test]
    fn self_relation_requires_name() {
        let rel = RelationDefinition::new(
            RelationKind::OneToMany,
            RelationEndpoint::new("User", "students").list(true),
            RelationEndpoint::new("User", "teacher")
                .scalar(vec!["teacherId"], vec!["id"])
                .optional(true),
        );
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("User"))
            .relation(rel);
        let err = validate_relations(&schema).unwrap_err();
        assert!(matches!(
            err,
            RelationValidationError::SelfRelationRequiresName(_)
        ));
    }

    #[test]
    fn sqlserver_restrict_is_rejected() {
        let rel = RelationDefinition::new(
            RelationKind::OneToMany,
            RelationEndpoint::new("User", "posts").list(true),
            RelationEndpoint::new("Post", "author")
                .scalar(vec!["authorId"], vec!["id"])
                .optional(false),
        )
        .name("UserPosts")
        .on_delete(ReferentialAction::Restrict);
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::SqlServer)
            .model(model("User"))
            .model(model("Post"))
            .relation(rel);
        let err = validate_relations(&schema).unwrap_err();
        assert!(matches!(
            err,
            RelationValidationError::UnsupportedReferentialAction { .. }
                | RelationValidationError::RestrictNotSupportedSqlServer(_)
        ));
    }

    #[test]
    fn relation_mode_prisma_index_recommendation() {
        let rel = RelationDefinition::new(
            RelationKind::OneToMany,
            RelationEndpoint::new("User", "posts").list(true),
            RelationEndpoint::new("Post", "author")
                .scalar(vec!["authorId"], vec!["id"])
                .optional(false)
                .indexed(false),
        )
        .name("UserPosts");
        let schema = RelationSchema::new(RelationMode::Prisma, SqlDialect::PostgreSql)
            .model(model("User"))
            .model(model("Post"))
            .relation(rel);
        let report = validate_relations(&schema).unwrap();
        assert_eq!(report.index_recommendations.len(), 1);
        assert_eq!(report.index_recommendations[0].model, "Post");
    }

    #[test]
    fn set_null_on_required_fk_is_invalid() {
        let rel = RelationDefinition::new(
            RelationKind::OneToMany,
            RelationEndpoint::new("User", "posts").list(true),
            RelationEndpoint::new("Post", "author")
                .scalar(vec!["authorId"], vec!["id"])
                .optional(false),
        )
        .name("UserPosts")
        .on_delete(ReferentialAction::SetNull);
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("User"))
            .model(model("Post"))
            .relation(rel);
        let err = validate_relations(&schema).unwrap_err();
        assert!(matches!(
            err,
            RelationValidationError::SetNullRequiredInvalid(_)
        ));
    }

    #[test]
    fn fk_sql_is_skipped_in_prisma_mode() {
        let sql = foreign_key_constraint_sql(ForeignKeyConstraintSqlInput {
            relation_mode: RelationMode::Prisma,
            dialect: SqlDialect::PostgreSql,
            table: "Post",
            constraint_name: "Post_authorId_fkey",
            fk_columns: &["authorId"],
            referenced_table: "User",
            referenced_columns: &["id"],
            on_delete: ReferentialAction::Cascade,
            on_update: ReferentialAction::Cascade,
        })
        .unwrap();
        assert!(sql.is_none());
    }

    #[test]
    fn fk_sql_renders_foreign_keys_mode() {
        let sql = foreign_key_constraint_sql(ForeignKeyConstraintSqlInput {
            relation_mode: RelationMode::ForeignKeys,
            dialect: SqlDialect::PostgreSql,
            table: "Post",
            constraint_name: "Post_authorId_fkey",
            fk_columns: &["authorId"],
            referenced_table: "User",
            referenced_columns: &["id"],
            on_delete: ReferentialAction::Cascade,
            on_update: ReferentialAction::Cascade,
        })
        .unwrap()
        .unwrap();
        assert!(sql.contains("ALTER TABLE"));
        assert!(sql.contains("ON DELETE CASCADE"));
        assert!(sql.contains("ON UPDATE CASCADE"));
    }

    #[test]
    fn implicit_self_mn_emits_ordering_warning() {
        let rel = RelationDefinition::new(
            RelationKind::ManyToManyImplicit,
            RelationEndpoint::new("Animal", "b_eats").list(true),
            RelationEndpoint::new("Animal", "a_eatenBy").list(true),
        )
        .name("FoodChain");
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("Animal"))
            .relation(rel);
        let report = validate_relations(&schema).unwrap();
        assert!(!report.warnings.is_empty());
    }

    #[test]
    fn deployment_plan_generates_fk_sql() {
        let rel = RelationDefinition::new(
            RelationKind::OneToMany,
            RelationEndpoint::new("User", "posts").list(true),
            RelationEndpoint::new("Post", "author")
                .scalar(vec!["authorId"], vec!["id"])
                .optional(false)
                .indexed(true),
        )
        .name("UserPosts");
        let schema = RelationSchema::new(RelationMode::ForeignKeys, SqlDialect::PostgreSql)
            .model(model("User"))
            .model(model("Post"))
            .relation(rel);
        let plan = build_relation_deployment_plan(&schema, 63).unwrap();
        assert_eq!(plan.foreign_key_sql.len(), 1);
        assert!(plan.foreign_key_sql[0].contains("FOREIGN KEY"));
        assert!(plan.foreign_key_sql[0].contains("REFERENCES"));
    }
}
