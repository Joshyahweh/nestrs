//! Bridge from Prisma `schema.prisma` text to `nestrs-prisma` Rust bindings.
//!
//! This lets teams keep Prisma schema as the source of truth while generating
//! Rust-side model/relation declarations for `prisma_model!` and relation helpers.

use crate::index_ddl::SqlDialect;
use crate::relations::{
    build_relation_deployment_plan, IndexRecommendation, ReferentialAction, RelationDefinition,
    RelationDeploymentPlan, RelationEndpoint, RelationKind, RelationMode, RelationSchema,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSchema {
    pub provider: Option<String>,
    pub relation_mode: Option<String>,
    pub models: Vec<ParsedModel>,
    pub enums: Vec<ParsedEnum>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEnum {
    pub name: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedModel {
    pub name: String,
    pub fields: Vec<ParsedField>,
    pub model_attributes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedField {
    pub name: String,
    pub type_name: String,
    pub optional: bool,
    pub list: bool,
    pub attributes: String,
    pub relation: Option<ParsedRelationAttr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRelationAttr {
    pub name: Option<String>,
    pub fields: Vec<String>,
    pub references: Vec<String>,
    pub on_delete: Option<ReferentialAction>,
    pub on_update: Option<ReferentialAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SchemaBridgeError {
    #[error("parse error: {0}")]
    Parse(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("relation validation error: {0}")]
    Relation(String),
    #[error("sql execution error: {0}")]
    Sql(String),
}

/// Options for generating/writing Rust bindings from `schema.prisma`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSyncOptions {
    /// Destination Rust file (for generated bindings).
    pub output_file: String,
    /// DB identifier max length used for FK name planning.
    pub max_identifier_len: usize,
    /// If true, execute generated FK DDL statements immediately.
    pub apply_foreign_keys: bool,
}

impl Default for SchemaSyncOptions {
    fn default() -> Self {
        Self {
            output_file: "src/models/prisma_generated.rs".to_string(),
            max_identifier_len: 63,
            apply_foreign_keys: false,
        }
    }
}

/// Full result of schema->Rust sync + relation deployment planning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaSyncReport {
    pub written_file: String,
    pub model_count: usize,
    pub relation_count: usize,
    pub warnings: Vec<String>,
    pub index_recommendations: Vec<IndexRecommendation>,
    pub foreign_key_sql: Vec<String>,
    pub applied_foreign_key_count: usize,
}

fn strip_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or("").trim()
}

fn parse_quoted_name(s: &str) -> Option<String> {
    let start = s.find('"')?;
    let end = s[start + 1..].find('"')?;
    Some(s[start + 1..start + 1 + end].to_string())
}

fn parse_list_arg(source: &str, key: &str) -> Vec<String> {
    if let Some(pos) = source.find(key) {
        let after = &source[pos + key.len()..];
        if let Some(open) = after.find('[') {
            let inner = &after[open + 1..];
            if let Some(close) = inner.find(']') {
                return inner[..close]
                    .split(',')
                    .map(|x| x.trim())
                    .filter(|x| !x.is_empty())
                    .map(str::to_string)
                    .collect();
            }
        }
    }
    Vec::new()
}

fn parse_action(source: &str, key: &str) -> Option<ReferentialAction> {
    let pos = source.find(key)?;
    let after = source[pos + key.len()..].trim_start();
    let val = after
        .split(|c: char| c == ',' || c == ')' || c.is_whitespace())
        .next()?;
    match val {
        "Cascade" => Some(ReferentialAction::Cascade),
        "Restrict" => Some(ReferentialAction::Restrict),
        "NoAction" => Some(ReferentialAction::NoAction),
        "SetNull" => Some(ReferentialAction::SetNull),
        "SetDefault" => Some(ReferentialAction::SetDefault),
        _ => None,
    }
}

fn parse_relation_attr(attrs: &str) -> Option<ParsedRelationAttr> {
    let rel_pos = attrs.find("@relation")?;
    let rel_src = &attrs[rel_pos..];
    let name = parse_quoted_name(rel_src);
    let fields = parse_list_arg(rel_src, "fields:");
    let references = parse_list_arg(rel_src, "references:");
    let on_delete = parse_action(rel_src, "onDelete:");
    let on_update = parse_action(rel_src, "onUpdate:");
    Some(ParsedRelationAttr {
        name,
        fields,
        references,
        on_delete,
        on_update,
    })
}

fn parse_field_line(line: &str) -> Option<ParsedField> {
    if line.starts_with("@@") {
        return None;
    }
    let mut parts = line.split_whitespace();
    let name = parts.next()?.to_string();
    let raw_ty = parts.next()?.to_string();
    let attrs = parts.collect::<Vec<_>>().join(" ");
    let list = raw_ty.ends_with("[]");
    let optional = raw_ty.ends_with('?');
    let type_name = raw_ty
        .trim_end_matches("[]")
        .trim_end_matches('?')
        .to_string();
    let relation = parse_relation_attr(&attrs);
    Some(ParsedField {
        name,
        type_name,
        optional,
        list,
        attributes: attrs,
        relation,
    })
}

/// Parse Prisma schema text into a simplified AST used by Rust codegen.
pub fn parse_prisma_schema(schema: &str) -> Result<ParsedSchema, SchemaBridgeError> {
    let mut provider = None;
    let mut relation_mode = None;
    let mut models = Vec::new();
    let mut enums = Vec::new();

    let lines: Vec<String> = schema
        .lines()
        .map(|l| strip_comment(l).to_string())
        .collect();
    let mut i = 0usize;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() {
            i += 1;
            continue;
        }

        if line.starts_with("datasource ") {
            i += 1;
            while i < lines.len() {
                let l = lines[i].trim();
                if l == "}" {
                    break;
                }
                if l.starts_with("provider") {
                    provider = parse_quoted_name(l).or_else(|| {
                        l.split('=')
                            .nth(1)
                            .map(|v| v.trim().trim_matches('"').to_string())
                    });
                } else if l.starts_with("relationMode") {
                    relation_mode = parse_quoted_name(l).or_else(|| {
                        l.split('=')
                            .nth(1)
                            .map(|v| v.trim().trim_matches('"').to_string())
                    });
                }
                i += 1;
            }
        } else if let Some(rest) = line.strip_prefix("model ") {
            let name = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| SchemaBridgeError::Parse("missing model name".to_string()))?
                .to_string();
            let mut fields = Vec::new();
            let mut model_attributes = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i].trim();
                if l == "}" {
                    break;
                }
                if l.starts_with("@@") {
                    model_attributes.push(l.to_string());
                } else if !l.is_empty() {
                    if let Some(f) = parse_field_line(l) {
                        fields.push(f);
                    }
                }
                i += 1;
            }
            models.push(ParsedModel {
                name,
                fields,
                model_attributes,
            });
        } else if let Some(rest) = line.strip_prefix("enum ") {
            let name = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| SchemaBridgeError::Parse("missing enum name".to_string()))?
                .to_string();
            let mut values = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i].trim();
                if l == "}" {
                    break;
                }
                if !l.is_empty() {
                    let v = l.split_whitespace().next().unwrap_or("").trim();
                    if !v.is_empty() {
                        values.push(v.to_string());
                    }
                }
                i += 1;
            }
            enums.push(ParsedEnum { name, values });
        }
        i += 1;
    }

    Ok(ParsedSchema {
        provider,
        relation_mode,
        models,
        enums,
    })
}

fn infer_dialect(provider: Option<&str>) -> SqlDialect {
    match provider
        .unwrap_or("postgresql")
        .to_ascii_lowercase()
        .as_str()
    {
        "postgresql" | "postgres" => SqlDialect::PostgreSql,
        "mysql" => SqlDialect::MySql,
        "sqlserver" => SqlDialect::SqlServer,
        "sqlite" => SqlDialect::Sqlite,
        "cockroachdb" => SqlDialect::CockroachDb,
        _ => SqlDialect::PostgreSql,
    }
}

fn infer_relation_mode(mode: Option<&str>) -> RelationMode {
    match mode.unwrap_or("foreignKeys") {
        "prisma" => RelationMode::Prisma,
        _ => RelationMode::ForeignKeys,
    }
}

fn is_single_id(model: &ParsedModel) -> bool {
    let field_ids = model
        .fields
        .iter()
        .filter(|f| f.attributes.contains("@id"))
        .count();
    let has_composite_id = model
        .model_attributes
        .iter()
        .any(|a| a.starts_with("@@id("));
    field_ids == 1 && !has_composite_id
}

#[derive(Debug, Clone)]
struct RelCandidate {
    model: String,
    field_name: String,
    target_model: String,
    list: bool,
    optional: bool,
    relation_name: Option<String>,
    scalar_fields: Vec<String>,
    referenced_fields: Vec<String>,
    on_delete: Option<ReferentialAction>,
    on_update: Option<ReferentialAction>,
}

fn collect_relation_candidates(parsed: &ParsedSchema) -> Vec<RelCandidate> {
    let model_names: HashMap<String, ()> =
        parsed.models.iter().map(|m| (m.name.clone(), ())).collect();
    let mut out = Vec::new();
    for m in &parsed.models {
        for f in &m.fields {
            if !model_names.contains_key(&f.type_name) {
                continue;
            }
            let rel = f.relation.clone();
            out.push(RelCandidate {
                model: m.name.clone(),
                field_name: f.name.clone(),
                target_model: f.type_name.clone(),
                list: f.list,
                optional: f.optional,
                relation_name: rel.as_ref().and_then(|r| r.name.clone()),
                scalar_fields: rel.as_ref().map(|r| r.fields.clone()).unwrap_or_default(),
                referenced_fields: rel
                    .as_ref()
                    .map(|r| r.references.clone())
                    .unwrap_or_default(),
                on_delete: rel.as_ref().and_then(|r| r.on_delete),
                on_update: rel.as_ref().and_then(|r| r.on_update),
            });
        }
    }
    out
}

/// Build validated `RelationSchema` from parsed Prisma schema.
pub fn build_relation_schema(parsed: &ParsedSchema) -> RelationSchema {
    let mut schema = RelationSchema::new(
        infer_relation_mode(parsed.relation_mode.as_deref()),
        infer_dialect(parsed.provider.as_deref()),
    );
    for m in &parsed.models {
        schema = schema.model(crate::relations::ModelMetadata {
            name: m.name.clone(),
            single_id: is_single_id(m),
        });
    }

    let candidates = collect_relation_candidates(parsed);
    let mut used = vec![false; candidates.len()];

    for i in 0..candidates.len() {
        if used[i] {
            continue;
        }
        let c = &candidates[i];
        let mut pair_idx = None;
        for j in (i + 1)..candidates.len() {
            if used[j] {
                continue;
            }
            let d = &candidates[j];
            if c.model == d.target_model
                && c.target_model == d.model
                && c.relation_name == d.relation_name
            {
                pair_idx = Some(j);
                break;
            }
            if c.model == d.target_model
                && c.target_model == d.model
                && c.relation_name.is_none()
                && d.relation_name.is_none()
            {
                pair_idx = Some(j);
                break;
            }
        }
        let Some(j) = pair_idx else {
            continue;
        };
        let d = &candidates[j];
        used[i] = true;
        used[j] = true;

        let left = RelationEndpoint::new(c.model.clone(), c.field_name.clone())
            .list(c.list)
            .optional(c.optional)
            .indexed(!c.scalar_fields.is_empty())
            .unique(false);
        let mut left = if !c.scalar_fields.is_empty() {
            let mut ep = left.scalar(
                c.scalar_fields.iter().map(String::as_str).collect(),
                c.referenced_fields.iter().map(String::as_str).collect(),
            );
            // Heuristic: one-to-one FK fields in parsed schema usually include @unique.
            ep.scalar_unique = false;
            ep
        } else {
            left
        };

        let right = RelationEndpoint::new(d.model.clone(), d.field_name.clone())
            .list(d.list)
            .optional(d.optional)
            .indexed(!d.scalar_fields.is_empty())
            .unique(false);
        let mut right = if !d.scalar_fields.is_empty() {
            let mut ep = right.scalar(
                d.scalar_fields.iter().map(String::as_str).collect(),
                d.referenced_fields.iter().map(String::as_str).collect(),
            );
            ep.scalar_unique = false;
            ep
        } else {
            right
        };

        // Read @unique hints for scalar relation fields.
        if let Some(m) = parsed.models.iter().find(|m| m.name == c.model) {
            if c.scalar_fields.iter().all(|sf| {
                m.fields.iter().any(|f| {
                    f.name == *sf
                        && (f.attributes.contains("@unique") || f.attributes.contains("@id"))
                })
            }) {
                left.scalar_unique = !c.scalar_fields.is_empty();
            }
        }
        if let Some(m) = parsed.models.iter().find(|m| m.name == d.model) {
            if d.scalar_fields.iter().all(|sf| {
                m.fields.iter().any(|f| {
                    f.name == *sf
                        && (f.attributes.contains("@unique") || f.attributes.contains("@id"))
                })
            }) {
                right.scalar_unique = !d.scalar_fields.is_empty();
            }
        }

        let kind = if c.list && d.list {
            RelationKind::ManyToManyImplicit
        } else if c.list ^ d.list {
            RelationKind::OneToMany
        } else {
            RelationKind::OneToOne
        };

        let mut rel = RelationDefinition::new(kind, left, right);
        if let Some(name) = c.relation_name.clone().or_else(|| d.relation_name.clone()) {
            rel = rel.name(name);
        }
        if let Some(a) = c.on_delete.or(d.on_delete) {
            rel = rel.on_delete(a);
        }
        if let Some(a) = c.on_update.or(d.on_update) {
            rel = rel.on_update(a);
        }
        schema = schema.relation(rel);
    }

    infer_explicit_many_to_many_relations(parsed, schema)
}

fn infer_explicit_many_to_many_relations(
    parsed: &ParsedSchema,
    mut schema: RelationSchema,
) -> RelationSchema {
    for join in &parsed.models {
        let join_rel_fields: Vec<&ParsedField> = join
            .fields
            .iter()
            .filter(|f| f.relation.is_some() && !f.list)
            .collect();
        if join_rel_fields.len() != 2 {
            continue;
        }

        let left_fk = join_rel_fields[0];
        let right_fk = join_rel_fields[1];
        if left_fk.type_name == right_fk.type_name {
            continue;
        }

        let left_model = left_fk.type_name.clone();
        let right_model = right_fk.type_name.clone();

        let left_back_field = parsed
            .models
            .iter()
            .find(|m| m.name == left_model)
            .and_then(|m| {
                m.fields
                    .iter()
                    .find(|f| f.list && f.type_name == join.name)
                    .map(|f| f.name.clone())
            });
        let right_back_field = parsed
            .models
            .iter()
            .find(|m| m.name == right_model)
            .and_then(|m| {
                m.fields
                    .iter()
                    .find(|f| f.list && f.type_name == join.name)
                    .map(|f| f.name.clone())
            });

        let has_primary_key = join.fields.iter().any(|f| f.attributes.contains("@id"))
            || join.model_attributes.iter().any(|a| a.starts_with("@@id("));
        let relation_name = left_fk
            .relation
            .as_ref()
            .and_then(|r| r.name.clone())
            .or_else(|| right_fk.relation.as_ref().and_then(|r| r.name.clone()))
            .unwrap_or_else(|| format!("{left_model}{right_model}Explicit"));

        let left_back_present = left_back_field.is_some();
        let right_back_present = right_back_field.is_some();

        let left_ep = RelationEndpoint::new(
            left_model.clone(),
            left_back_field.unwrap_or_else(|| format!("{}_links", join.name.to_lowercase())),
        )
        .list(true);
        let right_ep = RelationEndpoint::new(
            right_model.clone(),
            right_back_field.unwrap_or_else(|| format!("{}_links", join.name.to_lowercase())),
        )
        .list(true);

        let rel = RelationDefinition::new(RelationKind::ManyToManyExplicit, left_ep, right_ep)
            .name(relation_name)
            .join_model(crate::relations::JoinModel {
                model: join.name.clone(),
                has_primary_key,
                left_back_relation_present: left_back_present,
                right_back_relation_present: right_back_present,
                extra_fields: join
                    .fields
                    .iter()
                    .filter(|f| {
                        f.name != left_fk.name
                            && f.name != right_fk.name
                            && !left_fk
                                .relation
                                .as_ref()
                                .map(|r| r.fields.contains(&f.name))
                                .unwrap_or(false)
                            && !right_fk
                                .relation
                                .as_ref()
                                .map(|r| r.fields.contains(&f.name))
                                .unwrap_or(false)
                    })
                    .map(|f| f.name.clone())
                    .collect(),
            });

        if !schema.relations.iter().any(|r| {
            matches!(r.kind, RelationKind::ManyToManyExplicit)
                && ((r.left.model == left_model && r.right.model == right_model)
                    || (r.left.model == right_model && r.right.model == left_model))
        }) {
            schema = schema.relation(rel);
        }
    }

    schema
}

fn to_rust_type(field: &ParsedField) -> Option<String> {
    if field.list {
        return None;
    }
    let base = match field.type_name.as_str() {
        "Int" => "i64",
        "BigInt" => "i64",
        "String" => "String",
        "Boolean" => "bool",
        "Float" => "f64",
        "Decimal" => "f64",
        _ => return None,
    };
    if field.optional {
        Some(format!("Option<{base}>"))
    } else {
        Some(base.to_string())
    }
}

fn to_struct_name(model_name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for ch in model_name.chars() {
        if ch == '_' || ch == '-' {
            upper = true;
            continue;
        }
        if upper {
            out.push(ch.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

/// Generate Rust bindings text (`prisma_model!` declarations + relation schema snippet).
pub fn generate_rust_bindings(parsed: &ParsedSchema) -> String {
    let mut out = String::new();
    out.push_str("// Generated from schema.prisma via schema_bridge\n");
    out.push_str("use nestrs_prisma::{prisma_model, prisma_relation, prisma_relation_schema};\n\n");

    for m in &parsed.models {
        let struct_name = to_struct_name(&m.name);
        out.push_str(&format!(
            "prisma_model!({struct_name} => \"{}\", {{\n",
            m.name
        ));
        for f in &m.fields {
            if f.relation.is_some() {
                continue;
            }
            if let Some(rt) = to_rust_type(f) {
                out.push_str(&format!("    {}: {},\n", f.name, rt));
            }
        }
        out.push_str("});\n\n");
    }

    out.push_str("// Build relation schema from parsed Prisma models\n");
    out.push_str(
        "// let parsed = nestrs_prisma::schema_bridge::parse_prisma_schema(schema_text)?;\n",
    );
    out.push_str(
        "// let relation_schema = nestrs_prisma::schema_bridge::build_relation_schema(&parsed);\n",
    );
    out.push_str(&generate_relation_schema_snippet(parsed));
    out
}

/// Generate a relation-schema builder snippet that mirrors parsed Prisma relations.
pub fn generate_relation_schema_snippet(parsed: &ParsedSchema) -> String {
    let schema = build_relation_schema(parsed);
    let relation_mode = match schema.relation_mode {
        RelationMode::ForeignKeys => "ForeignKeys",
        RelationMode::Prisma => "Prisma",
    };
    let dialect = match schema.dialect {
        SqlDialect::PostgreSql => "PostgreSql",
        SqlDialect::Sqlite => "Sqlite",
        SqlDialect::MySql => "MySql",
        SqlDialect::SqlServer => "SqlServer",
        SqlDialect::CockroachDb => "CockroachDb",
    };

    let mut out = String::new();
    out.push_str(&format!(
        "\n// relation schema snippet\nlet relation_schema = nestrs_prisma::relations::RelationSchema::new(\n    nestrs_prisma::relations::RelationMode::{relation_mode},\n    nestrs_prisma::index_ddl::SqlDialect::{dialect},\n)\n"
    ));
    for m in schema.models.values() {
        out.push_str(&format!(
            "    .model(nestrs_prisma::relations::ModelMetadata {{ name: \"{}\".to_string(), single_id: {} }})\n",
            m.name, m.single_id
        ));
    }
    for r in &schema.relations {
        let kind = match r.kind {
            RelationKind::OneToOne => "OneToOne",
            RelationKind::OneToMany => "OneToMany",
            RelationKind::ManyToManyImplicit => "ManyToManyImplicit",
            RelationKind::ManyToManyExplicit => "ManyToManyExplicit",
        };
        out.push_str("    .relation(\n");
        out.push_str(&format!(
            "        nestrs_prisma::relations::RelationDefinition::new(\n            nestrs_prisma::relations::RelationKind::{kind},\n            nestrs_prisma::relations::RelationEndpoint::new(\"{}\", \"{}\").list({}).optional({}),\n            nestrs_prisma::relations::RelationEndpoint::new(\"{}\", \"{}\").list({}).optional({}),\n        )",
            r.left.model,
            r.left.relation_field,
            r.left.list,
            r.left.optional,
            r.right.model,
            r.right.relation_field,
            r.right.list,
            r.right.optional
        ));
        if let Some(name) = &r.name {
            out.push_str(&format!(".name(\"{}\")", name));
        }
        out.push_str("\n    )\n");
    }
    out.push_str(";\n");
    out
}

/// Write generated Rust bindings to disk, creating parent directory when needed.
pub fn write_generated_bindings(path: &str, contents: &str) -> Result<(), SchemaBridgeError> {
    if let Some(parent) = Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| {
                SchemaBridgeError::Io(format!("create dir `{}`: {e}", parent.display()))
            })?;
        }
    }
    fs::write(path, contents).map_err(|e| SchemaBridgeError::Io(format!("write `{path}`: {e}")))?;
    Ok(())
}

#[cfg(feature = "sqlx")]
impl crate::PrismaService {
    /// Load and parse a Prisma schema file from disk.
    pub fn parse_schema_file(&self, path: &str) -> Result<ParsedSchema, SchemaBridgeError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| SchemaBridgeError::Parse(format!("read schema `{path}`: {e}")))?;
        parse_prisma_schema(&text)
    }

    /// Generate Rust binding snippet text from a Prisma schema path.
    pub fn generate_bindings_from_schema_file(
        &self,
        path: &str,
    ) -> Result<String, SchemaBridgeError> {
        let parsed = self.parse_schema_file(path)?;
        Ok(generate_rust_bindings(&parsed))
    }

    /// End-to-end sync:
    /// 1) parse schema
    /// 2) generate Rust bindings and write to `output_file`
    /// 3) validate/build relation deployment plan
    /// 4) optionally apply FK DDL statements
    pub async fn sync_from_prisma_schema(
        &self,
        schema_path: &str,
        opts: SchemaSyncOptions,
    ) -> Result<SchemaSyncReport, SchemaBridgeError> {
        let parsed = self.parse_schema_file(schema_path)?;
        let generated = generate_rust_bindings(&parsed);
        write_generated_bindings(&opts.output_file, &generated)?;

        let relation_schema = build_relation_schema(&parsed);
        let plan: RelationDeploymentPlan =
            build_relation_deployment_plan(&relation_schema, opts.max_identifier_len)
                .map_err(|e| SchemaBridgeError::Relation(e.to_string()))?;

        let applied_foreign_key_count = if opts.apply_foreign_keys {
            self.apply_relation_deployment_plan(&plan)
                .await
                .map_err(SchemaBridgeError::Sql)?
                .len()
        } else {
            0
        };

        Ok(SchemaSyncReport {
            written_file: opts.output_file,
            model_count: parsed.models.len(),
            relation_count: relation_schema.relations.len(),
            warnings: plan.validation.warnings.clone(),
            index_recommendations: plan.validation.index_recommendations.clone(),
            foreign_key_sql: plan.foreign_key_sql.clone(),
            applied_foreign_key_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
datasource db {
  provider = "postgresql"
  relationMode = "foreignKeys"
}

model users {
  id Int @id @default(autoincrement())
  email String @unique
  profile profiles? @relation("UserProfile")
  posts posts[] @relation("UserPosts")
}

model profiles {
  id Int @id @default(autoincrement())
  user_id Int @unique
  user users @relation("UserProfile", fields: [user_id], references: [id], onDelete: Cascade)
}

model posts {
  id Int @id @default(autoincrement())
  author_id Int
  author users @relation("UserPosts", fields: [author_id], references: [id], onDelete: Cascade)
}

model categories {
  id Int @id @default(autoincrement())
  posts posts[] @relation("PostCategories")
}

model posts_categories {
  post_id Int
  category_id Int
  post posts @relation(fields: [post_id], references: [id])
  category categories @relation(fields: [category_id], references: [id])
  @@id([post_id, category_id])
}
"#;

    #[test]
    fn parse_schema_extracts_models_and_relations() {
        let parsed = parse_prisma_schema(SAMPLE).unwrap();
        assert_eq!(parsed.provider.as_deref(), Some("postgresql"));
        assert_eq!(parsed.models.len(), 5);
        let users = parsed.models.iter().find(|m| m.name == "users").unwrap();
        assert!(users.fields.iter().any(|f| f.relation.is_some()));
    }

    #[test]
    fn build_relation_schema_infers_core_relations() {
        let parsed = parse_prisma_schema(SAMPLE).unwrap();
        let schema = build_relation_schema(&parsed);
        assert!(schema.relations.len() >= 3);
        assert!(schema
            .relations
            .iter()
            .any(|r| matches!(r.kind, RelationKind::OneToOne)));
        assert!(schema
            .relations
            .iter()
            .any(|r| matches!(r.kind, RelationKind::OneToMany)));
        assert!(schema
            .relations
            .iter()
            .any(|r| matches!(r.kind, RelationKind::ManyToManyExplicit)));
    }

    #[test]
    fn generate_bindings_contains_prisma_model_blocks() {
        let parsed = parse_prisma_schema(SAMPLE).unwrap();
        let code = generate_rust_bindings(&parsed);
        assert!(code.contains("prisma_model!(Users => \"users\""));
        assert!(code.contains("prisma_model!(Profiles => \"profiles\""));
    }

    #[test]
    fn write_generated_bindings_creates_file() {
        let parsed = parse_prisma_schema(SAMPLE).unwrap();
        let code = generate_rust_bindings(&parsed);
        let mut p = std::env::temp_dir();
        p.push(format!(
            "nestrs_prisma_schema_bridge_{}.rs",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_generated_bindings(p.to_str().unwrap(), &code).unwrap();
        let saved = std::fs::read_to_string(&p).unwrap();
        assert!(saved.contains("prisma_model!"));
        let _ = std::fs::remove_file(&p);
    }
}
