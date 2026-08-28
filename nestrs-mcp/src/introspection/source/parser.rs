//! `syn`-level attribute parsers used by `source.rs`.
//!
//! Each helper is responsible for one specific `nestrs-macros` shape. They
//! are deliberately **lenient**: unrecognized args or variants are recorded
//! in `warnings` rather than failing the parse. This way, when
//! `nestrs-macros` gains a new attribute, the parser doesn't break
//! pre-existing project introspects.

use syn::{
    meta::ParseNestedMeta, parse::Parse, Attribute, Fields, GenericArgument, ItemFn, ItemImpl,
    ItemStruct, LitStr, Meta, PathArguments, Type, TypePath,
};

use super::{ControllerSummary, DtoSummary, ModuleSummary, ParserWarning, RouteSummary};
use crate::introspection::metadata::{DtoField, Validator};

const NESTRS_ATTR_MODULE: &str = "module";
const NESTRS_ATTR_CONTROLLER: &str = "controller";
const NESTRS_ATTR_ROUTES: &str = "routes";
const NESTRS_ATTR_DTO: &str = "dto";
const NESTRS_ATTR_INJECTABLE: &str = "injectable";
const NESTRS_ATTR_GET: &str = "get";
const NESTRS_ATTR_POST: &str = "post";
const NESTRS_ATTR_PUT: &str = "put";
const NESTRS_ATTR_PATCH: &str = "patch";
const NESTRS_ATTR_DELETE: &str = "delete";
const NESTRS_ATTR_OPTIONS: &str = "options";
const NESTRS_ATTR_HEAD: &str = "head";
const NESTRS_ATTR_ALL: &str = "all";
const NESTRS_ATTR_VER: &str = "ver";
const NESTRS_ATTR_USE_GUARDS: &str = "use_guards";
const NESTRS_ATTR_USE_INTERCEPTORS: &str = "use_interceptors";
const NESTRS_ATTR_USE_PIPES: &str = "use_pipes";
const NESTRS_ATTR_USE_FILTERS: &str = "use_filters";
const NESTRS_ATTR_SET_METADATA: &str = "set_metadata";
const NESTRS_ATTR_ROLES: &str = "roles";
const NESTRS_ATTR_OPENAPI: &str = "openapi";
const NESTRS_ATTR_WS_GATEWAY: &str = "ws_gateway";
const NESTRS_ATTR_WS_ROUTES: &str = "ws_routes";
const NESTRS_ATTR_MICRO_ROUTES: &str = "micro_routes";
const NESTRS_ATTR_EVENT_ROUTES: &str = "event_routes";
const NESTRS_ATTR_SCHEDULE_ROUTES: &str = "schedule_routes";

/// All attribute names we treat as "schedules" (function-level markers).
const SCHEDULE_ATTRS: &[&str] = &["interval", "cron"];
/// All attribute names we treat as "event handlers" (function-level markers).
#[allow(dead_code)] // wired up in v2 when the parser visits fn attrs on `impl` blocks
const EVENT_ATTRS: &[&str] = &["on_event"];
/// All attribute names we treat as "queue processors" (function-level markers).
#[allow(dead_code)] // wired up in v2 when the parser visits fn attrs on `impl` blocks
const QUEUE_ATTRS: &[&str] = &["process"];

pub(super) fn struct_name(item: &ItemStruct) -> String {
    item.ident.to_string()
}

/// Parse `#[module(...)] pub struct X;` — attribute is on the struct
/// (the source form users actually write). Equivalent to `parse_module_impl`
/// but takes an `ItemStruct` instead of an `ItemImpl`. Returns `None` if
/// the struct doesn't carry `#[module]`.
pub(super) fn parse_module_struct(
    item_struct: &ItemStruct,
    file: &str,
    warnings: &mut Vec<ParserWarning>,
) -> Option<ModuleSummary> {
    if !has_bare_attr(&item_struct.attrs, NESTRS_ATTR_MODULE) {
        return None;
    }
    let attr = find_attr(&item_struct.attrs, NESTRS_ATTR_MODULE)?;
    let name = item_struct.ident.to_string();
    let mut imports = Vec::new();
    let mut controllers = Vec::new();
    let mut providers = Vec::new();
    let mut microservices = Vec::new();
    let mut exports = Vec::new();
    let mut re_exports = Vec::new();

    let _ = attr.parse_nested_meta(|meta| {
        let list = read_idents_in_paren(&meta).unwrap_or_default();
        match meta
            .path
            .get_ident()
            .map(|i| i.to_string())
            .as_deref()
            .unwrap_or("")
        {
            "imports" => imports = list,
            "controllers" => controllers = list,
            "providers" => providers = list,
            "microservices" => microservices = list,
            "exports" => exports = list,
            "re_exports" => re_exports = list,
            other => warnings.push(ParserWarning {
                file: file.into(),
                line: 0,
                kind: "module".into(),
                message: format!("unrecognized module arg `{other}`"),
            }),
        }
        Ok(())
    });

    Some(ModuleSummary {
        name,
        file: file.into(),
        imports,
        controllers,
        providers,
        microservices,
        exports,
        re_exports,
    })
}

/// Parse `#[controller("/path")] pub struct X;` — the source form users
/// actually write. Returns `None` if the struct doesn't carry
/// `#[controller]`. Routes are populated separately when a
/// `#[routes(X)] impl X { ... }` block is encountered.
pub(super) fn parse_controller_struct(
    item_struct: &ItemStruct,
    file: &str,
    warnings: &mut Vec<ParserWarning>,
) -> Option<ControllerSummary> {
    if !has_bare_attr(&item_struct.attrs, NESTRS_ATTR_CONTROLLER) {
        return None;
    }
    let name = item_struct.ident.to_string();
    let mut prefix = None;
    let mut version = None;
    let mut host = None;

    for attr in &item_struct.attrs {
        if !attr.path().is_ident(NESTRS_ATTR_CONTROLLER) {
            continue;
        }
        if let Meta::List(list) = &attr.meta {
            if let Ok(ts) = list.parse_args::<proc_macro2::TokenStream>() {
                if let Ok(lit) = syn::parse2::<LitStr>(ts) {
                    if prefix.is_none() {
                        prefix = Some(lit.value());
                    }
                    continue;
                }
            }
        }
        let _ = attr.parse_nested_meta(|meta| {
            let key = meta
                .path
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_default();
            let val = read_meta_litstr(&meta).unwrap_or_default();
            match key.as_str() {
                "prefix" => prefix = Some(val),
                "version" => version = Some(val),
                "host" => host = Some(val),
                other => warnings.push(ParserWarning {
                    file: file.into(),
                    line: 0,
                    kind: "controller".into(),
                    message: format!("unrecognized controller arg `{other}`"),
                }),
            }
            Ok(())
        });
    }

    let module_path = infer_module_path(file);

    Some(ControllerSummary {
        name,
        module_path,
        file: file.into(),
        prefix,
        version,
        host,
        routes: Vec::new(),
        controller_guards: Vec::new(),
        state: None,
    })
}

/// Returns true if the attribute is `name`, `name(...)`, or `name = ...`
/// (any of the three meta shapes). Used as a "does this struct carry the
/// attribute at all?" predicate.
fn has_bare_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|a| a.path().is_ident(name))
}

/// Read `state = T` and `controller_guards = (...)` from a `#[routes(...)]`
/// attr list. Returns `(state, controller_guards)`; either may be absent
/// on the `#[routes]` attr, in which case the corresponding value is
/// `None` / `Vec::new()`. Used to back-fill the struct-form controller
/// (which has no `#[routes]` attrs of its own) with the values declared
/// on the matching `#[routes(X)] impl X { ... }`.
pub(super) fn parse_routes_args(item_impl: &ItemImpl) -> (Option<String>, Vec<String>) {
    let attr = match find_attr(&item_impl.attrs, NESTRS_ATTR_ROUTES) {
        Some(a) => a,
        None => return (None, Vec::new()),
    };
    let list = match &attr.meta {
        Meta::List(l) => l,
        _ => return (None, Vec::new()),
    };
    let parsed = syn::parse2::<RoutesArgsParser>(list.tokens.clone()).unwrap_or_default();
    (parsed.state, parsed.controller_guards)
}

#[derive(Default)]
struct RoutesArgsParser {
    state: Option<String>,
    controller_guards: Vec<String>,
    first_ident: Option<syn::Ident>,
}

impl syn::parse::Parse for RoutesArgsParser {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut me = Self::default();
        let mut first = true;
        while !input.is_empty() {
            let key: syn::Ident = input.parse()?;
            let key_str = key.to_string();
            if first {
                me.first_ident = Some(key.clone());
                first = false;
            }
            if input.peek(syn::Token![=]) {
                let _eq: syn::Token![=] = input.parse()?;
                if key_str == "state" {
                    // `state = T` where T is a type ident (e.g. `AppState`).
                    let ty: syn::Type = input.parse()?;
                    me.state = last_path_segment_from_type(&ty);
                } else if key_str == "controller_guards" {
                    let content;
                    syn::parenthesized!(content in input);
                    let list: syn::punctuated::Punctuated<syn::Path, syn::Token![,]> =
                        content.parse_terminated(syn::Path::parse, syn::Token![,])?;
                    me.controller_guards = list
                        .iter()
                        .filter_map(|p| p.segments.last().map(|s| s.ident.to_string()))
                        .collect();
                } else {
                    // Unknown kv pair — skip its value.
                    let _ = input.parse::<proc_macro2::TokenStream>();
                }
            } else {
                // Bare ident (the target type) — ignore.
            }
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(me)
    }
}


/// If this `impl` is `#[routes(SomeName)] impl SomeName { ... }`,
/// return the target controller name so callers can attach the routes
/// to the right ControllerSummary. Returns `None` for other impls.
pub(super) fn parse_routes_target(item_impl: &ItemImpl) -> Option<String> {
    let attr = find_attr(&item_impl.attrs, NESTRS_ATTR_ROUTES)?;
    let list = match &attr.meta {
        Meta::List(l) => l,
        _ => return None,
    };
    // Confirm the first token parses as an ident (the target type). We
    // don't validate it matches `self_name` — the orchestrator does.
    let _first_ident: Option<syn::Ident> = syn::parse2(list.tokens.clone())
        .ok()
        .and_then(|p: RoutesArgsParser| p.first_ident);
    let self_name = match &*item_impl.self_ty {
        Type::Path(TypePath { path, .. }) => last_path_segment(path)?,
        _ => return None,
    };
    Some(self_name)
}

pub(super) fn fn_name(item: &ItemFn) -> String {
    item.sig.ident.to_string()
}

pub(super) fn is_injectable_struct(item: &ItemStruct) -> bool {
    has_attr(&item.attrs, NESTRS_ATTR_INJECTABLE)
}

// ---------- module ----------

pub(super) fn parse_module_impl(
    item_impl: &ItemImpl,
    file: &str,
    warnings: &mut Vec<ParserWarning>,
) -> Option<ModuleSummary> {
    let attr = find_attr(&item_impl.attrs, NESTRS_ATTR_MODULE)?;
    let name = match &*item_impl.self_ty {
        Type::Path(TypePath { path, .. }) => last_path_segment(path).unwrap_or_else(|| "<anon>".into()),
        _ => "<anon>".into(),
    };
    let mut imports = Vec::new();
    let mut controllers = Vec::new();
    let mut providers = Vec::new();
    let mut microservices = Vec::new();
    let mut exports = Vec::new();
    let mut re_exports = Vec::new();

    let _ = attr.parse_nested_meta(|meta| {
        let list = read_idents_in_paren(&meta).unwrap_or_default();
        match meta
            .path
            .get_ident()
            .map(|i| i.to_string())
            .as_deref()
            .unwrap_or("")
        {
            "imports" => imports = list,
            "controllers" => controllers = list,
            "providers" => providers = list,
            "microservices" => microservices = list,
            "exports" => exports = list,
            "re_exports" => re_exports = list,
            other => warnings.push(ParserWarning {
                file: file.into(),
                line: 0,
                kind: "module".into(),
                message: format!("unrecognized module arg `{other}`"),
            }),
        }
        Ok(())
    });

    Some(ModuleSummary {
        name,
        file: file.into(),
        imports,
        controllers,
        providers,
        microservices,
        exports,
        re_exports,
    })
}

// ---------- controller + routes ----------

pub(super) fn parse_controller_impl(
    item_impl: &ItemImpl,
    file: &str,
    warnings: &mut Vec<ParserWarning>,
) -> Option<ControllerSummary> {
    let self_name = match &*item_impl.self_ty {
        Type::Path(TypePath { path, .. }) => last_path_segment(path)?,
        _ => return None,
    };

    let mut prefix = None;
    let mut version = None;
    let mut host = None;
    let mut routes = Vec::new();
    let mut controller_guards = Vec::new();
    let mut state = None;

    for attr in &item_impl.attrs {
        if attr.path().is_ident(NESTRS_ATTR_CONTROLLER) {
            // `#[controller("/users")]` is a bare path literal;
            // `#[controller(prefix = "/users", version = "v2", host = "...")]` is kv.
            if let Meta::List(list) = &attr.meta {
                let parsed = list
                    .parse_args::<proc_macro2::TokenStream>()
                    .ok();
                if let Some(ts) = parsed {
                    // Try to parse as a single string literal first.
                    if let Ok(lit) = syn::parse2::<LitStr>(ts.clone()) {
                        if prefix.is_none() {
                            prefix = Some(lit.value());
                        }
                        continue;
                    }
                }
            }
            let _ = attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                let val = read_meta_litstr(&meta).unwrap_or_default();
                match key.as_str() {
                    "prefix" => prefix = Some(val),
                    "version" => version = Some(val),
                    "host" => host = Some(val),
                    other => warnings.push(ParserWarning {
                        file: file.into(),
                        line: 0,
                        kind: "controller".into(),
                        message: format!("unrecognized controller arg `{other}`"),
                    }),
                }
                Ok(())
            });
        }

        if attr.path().is_ident(NESTRS_ATTR_ROUTES) {
            // Use the same manual walker as `parse_routes_args` — the
            // `#[routes(X, ...)]` form starts with a bare ident which
            // `parse_nested_meta` chokes on. Populate locals only if the
            // walk yielded values; we don't surface unrecognized keys
            // here (the orchestrator already parses them elsewhere).
            if let Meta::List(list) = &attr.meta {
                if let Ok(p) = syn::parse2::<RoutesArgsParser>(list.tokens.clone()) {
                    if state.is_none() {
                        state = p.state;
                    }
                    if controller_guards.is_empty() {
                        controller_guards = p.controller_guards;
                    }
                }
            }
        }
    }

    for item in &item_impl.items {
        if let syn::ImplItem::Fn(method) = item {
            if let Some(route) = parse_route_method(method, warnings) {
                routes.push(route);
            }
        }
    }

    if prefix.is_none() && routes.is_empty() {
        return None;
    }

    let module_path = infer_module_path(file);

    Some(ControllerSummary {
        name: self_name,
        module_path,
        file: file.into(),
        prefix,
        version,
        host,
        routes,
        controller_guards,
        state,
    })
}

pub(super) fn parse_route_method(
    method: &syn::ImplItemFn,
    warnings: &mut Vec<ParserWarning>,
) -> Option<RouteSummary> {
    let name = method.sig.ident.to_string();
    let mut found: Option<(&'static str, String)> = None;
    let mut version = None;
    let mut guards = Vec::new();
    let mut interceptors = Vec::new();
    let mut pipes = Vec::new();
    let mut filters = Vec::new();
    let mut metadata_tree = std::collections::BTreeMap::new();
    let mut body_type = None;
    let mut response_type = None;

    for attr in &method.attrs {
        let ident = match attr.path().get_ident() {
            Some(i) => i.to_string(),
            None => continue,
        };
        match ident.as_str() {
            NESTRS_ATTR_GET
            | NESTRS_ATTR_POST
            | NESTRS_ATTR_PUT
            | NESTRS_ATTR_PATCH
            | NESTRS_ATTR_DELETE
            | NESTRS_ATTR_OPTIONS
            | NESTRS_ATTR_HEAD
            | NESTRS_ATTR_ALL => {
                let path = attr.parse_args::<syn::LitStr>().ok()?.value();
                found = Some((method_name_to_http(ident.as_str()), path));
            }
            NESTRS_ATTR_VER => {
                if let Ok(lit) = attr.parse_args::<syn::LitStr>() {
                    version = Some(lit.value());
                }
            }
            NESTRS_ATTR_USE_GUARDS => {
                guards.extend(read_idents_in_attr_args(attr));
            }
            NESTRS_ATTR_USE_INTERCEPTORS => {
                interceptors.extend(read_idents_in_attr_args(attr));
            }
            NESTRS_ATTR_USE_PIPES => {
                pipes.extend(read_idents_in_attr_args(attr));
            }
            NESTRS_ATTR_USE_FILTERS => {
                filters.extend(read_idents_in_attr_args(attr));
            }
            NESTRS_ATTR_SET_METADATA => {
                if let Some((k, v)) = parse_kv_attr_args(attr) {
                    metadata_tree.insert(k, v);
                }
            }
            NESTRS_ATTR_ROLES => {
                // `#[roles("admin", "ops", ...)]` — multiple positional
                // LitStrs. Walk them manually so we don't trip over
                // `parse_nested_meta`'s kv-only iteration.
                if let Meta::List(list) = &attr.meta {
                    let parsed: Result<RolesList, _> = syn::parse2(list.tokens.clone());
                    if let Ok(p) = parsed {
                        if !p.roles.is_empty() {
                            metadata_tree.insert("roles".into(), p.roles.join(","));
                        }
                    }
                }
            }
            NESTRS_ATTR_OPENAPI => {
                // `#[openapi(summary = "...", operation_id = "...")]` may
                // have multiple kv pairs — collect all of them.
                for (k, v) in parse_kv_attr_args_many(attr) {
                    metadata_tree.insert(format!("openapi.{k}"), v);
                }
            }
            _ => {}
        }
    }

    if let Some((m, p)) = found {
        // Skip `&self` (Receiver) and look at the first real typed arg.
        for arg in &method.sig.inputs {
            if let syn::FnArg::Typed(ty) = arg {
                let type_str = type_to_string(&ty.ty);
                if type_str.contains("ValidatedBody") || type_str.contains("Json<") {
                    body_type = Some(type_str);
                    break;
                }
            }
        }
        if let syn::ReturnType::Type(_, ty) = &method.sig.output {
            response_type = Some(type_to_string(ty));
        }
        return Some(RouteSummary {
            method: m.to_string(),
            path: p,
            handler: name,
            version,
            guards,
            interceptors,
            pipes,
            filters,
            metadata: metadata_tree,
            body_type,
            response_type,
        });
    }

    if !metadata_tree.is_empty() || !guards.is_empty() {
        warnings.push(ParserWarning {
            file: method.sig.ident.to_string(),
            line: 0,
            kind: "routes".into(),
            message: format!(
                "method `{name}` has routing-related attrs but no #[get/post/...] — skipped"
            ),
        });
    }
    None
}

fn method_name_to_http(s: &str) -> &'static str {
    match s {
        "get" => "GET",
        "post" => "POST",
        "put" => "PUT",
        "patch" => "PATCH",
        "delete" => "DELETE",
        "options" => "OPTIONS",
        "head" => "HEAD",
        "all" => "ALL",
        _ => "GET",
    }
}

// ---------- dto ----------

pub(super) fn parse_dto_struct(
    item_struct: &ItemStruct,
    file: &str,
    warnings: &mut Vec<ParserWarning>,
) -> Option<DtoSummary> {
    let attr = find_attr(&item_struct.attrs, NESTRS_ATTR_DTO)?;
    let mut allow_unknown_fields = false;
    let mut expose_only = false;

    // Bare `#[dto]` ⇒ matches `Meta::Path` (no parens).
    if matches!(attr.meta, Meta::Path(_)) {
        // defaults
    } else {
        let _ = attr.parse_nested_meta(|meta| {
            if let Some(id) = meta.path.get_ident() {
                match id.to_string().as_str() {
                    "expose_only" => expose_only = true,
                    "allow_unknown_fields" => allow_unknown_fields = true,
                    other => warnings.push(ParserWarning {
                        file: file.into(),
                        line: 0,
                        kind: "dto".into(),
                        message: format!("unrecognized dto arg `{other}`"),
                    }),
                }
            }
            Ok(())
        });
    }

    let name = struct_name(item_struct);
    let module_path = infer_module_path(file);
    let fields = collect_dto_fields(&item_struct.fields, warnings);
    Some(DtoSummary {
        name,
        module_path,
        file: file.into(),
        field_count: fields.len(),
        allow_unknown_fields,
        expose_only,
    })
}

pub(super) fn collect_dto_fields(
    fields: &Fields,
    _warnings: &mut Vec<ParserWarning>,
) -> Vec<DtoField> {
    let named = match fields {
        Fields::Named(named) => named,
        _ => return Vec::new(),
    };
    named
        .named
        .iter()
        .map(|f| {
            let name = f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default();
            let ty = type_to_string(&f.ty);
            let optional = ty.starts_with("Option <") || ty.starts_with("Option<");
            let validators = collect_validators(&f.attrs);
            DtoField {
                name,
                ty,
                optional,
                validators,
            }
        })
        .collect()
}

fn collect_validators(attrs: &[Attribute]) -> Vec<Validator> {
    let mut out = Vec::new();
    for attr in attrs {
        let Some(ident) = attr.path().get_ident() else {
            continue;
        };
        let name = ident.to_string();
        let v = match name.as_str() {
            "IsString" => Validator::IsString,
            "IsEmail" => Validator::IsEmail,
            "IsNotEmpty" => Validator::IsNotEmpty,
            "IsUUID" | "IsUuid" => Validator::IsUuid,
            "IsPositive" => Validator::IsPositive,
            "IsNegative" => Validator::IsNegative,
            "IsInt" => Validator::IsInt,
            "IsNumber" => Validator::IsNumber,
            "IsBoolean" => Validator::IsBoolean,
            "IsUrl" => Validator::IsUrl,
            "IsOptional" => Validator::IsOptional,
            "MinLength" => Validator::MinLength {
                value: read_u64_arg(attr).unwrap_or(0),
            },
            "MaxLength" => Validator::MaxLength {
                value: read_u64_arg(attr).unwrap_or(0),
            },
            "Length" => match read_min_max(attr) {
                Some((min, max)) => Validator::Length { min, max },
                None => Validator::Unknown {
                    name: name.clone(),
                    args: format!("{attr:?}"),
                },
            },
            "Min" => Validator::Min {
                value: read_string_arg(attr).unwrap_or_default(),
            },
            "Max" => Validator::Max {
                value: read_string_arg(attr).unwrap_or_default(),
            },
            "Matches" => Validator::Matches {
                pattern: read_string_arg(attr).unwrap_or_default(),
            },
            "Contains" => Validator::Contains {
                substring: read_string_arg(attr).unwrap_or_default(),
            },
            "ValidateNested" => Validator::ValidateNested,
            "Expose" => Validator::Expose,
            "Exclude" => Validator::Exclude,
            other => Validator::Unknown {
                name: other.into(),
                args: format!("{attr:?}"),
            },
        };
        out.push(v);
    }
    out
}

// ---------- injectable ----------

pub(super) fn parse_injectable_scope(item: &ItemStruct) -> Option<String> {
    let attr = find_attr(&item.attrs, NESTRS_ATTR_INJECTABLE)?;
    if matches!(attr.meta, Meta::Path(_)) {
        return Some("singleton".into());
    }
    let mut scope = "singleton".to_string();
    let _ = attr.parse_nested_meta(|meta| {
        if let Some(id) = meta.path.get_ident() {
            if id == "scope" {
                if let Some(lit) = read_meta_litstr(&meta) {
                    scope = lit;
                }
            }
        }
        Ok(())
    });
    Some(scope)
}

// ---------- function-level markers ----------

pub(super) fn parse_schedule_attr(attrs: &[Attribute]) -> Option<&'static str> {
    for a in attrs {
        if let Some(id) = a.path().get_ident() {
            let n = id.to_string();
            if SCHEDULE_ATTRS.contains(&n.as_str()) {
                return Some(match n.as_str() {
                    "interval" => "interval",
                    "cron" => "cron",
                    _ => "schedule",
                });
            }
        }
    }
    if attrs.iter().any(|a| a.path().is_ident(NESTRS_ATTR_SCHEDULE_ROUTES)) {
        return Some("schedule_routes");
    }
    None
}

pub(super) fn parse_event_attr(attrs: &[Attribute]) -> Option<&'static str> {
    for a in attrs {
        if let Some(id) = a.path().get_ident() {
            if id == "on_event" {
                return Some("on_event");
            }
        }
    }
    if attrs.iter().any(|a| a.path().is_ident(NESTRS_ATTR_EVENT_ROUTES)) {
        return Some("event_routes");
    }
    None
}

pub(super) fn parse_queue_attr(attrs: &[Attribute]) -> Option<&'static str> {
    for a in attrs {
        if let Some(id) = a.path().get_ident() {
            if id == "process" {
                return Some("process");
            }
        }
    }
    if attrs.iter().any(|a| a.path().is_ident("queue")) {
        return Some("queue_routes");
    }
    None
}

// ---------- small attribute helpers ----------

fn has_attr(attrs: &[Attribute], name: &str) -> bool {
    attrs.iter().any(|a| a.path().is_ident(name))
}

fn find_attr<'a>(attrs: &'a [Attribute], name: &str) -> Option<&'a Attribute> {
    attrs.iter().find(|a| a.path().is_ident(name))
}

/// Read `meta.value()?` and try to extract a `LitStr` from it.
fn read_meta_litstr(meta: &ParseNestedMeta<'_>) -> Option<String> {
    let value = meta.value().ok()?;
    value.parse::<LitStr>().ok().map(|l| l.value())
}

/// Read `#[name(key = "value")]` → `("key", "value")`. Returns `None` if
/// the shape doesn't match.
fn parse_kv_attr_args(attr: &Attribute) -> Option<(String, String)> {
    // First try the positional form: `#[name("key", "value")]` — used by
    // `set_metadata` and `openapi(...)` style attrs that take two string
    // literals. Fall back to the kv form `#[name(key = "...", value = "...")]`
    // for forward-compat / decorator-style attrs.
    if let Meta::List(list) = &attr.meta {
        // Use syn's parse2 over the whole list — it'll handle the
        // leading two LitStr tokens and any trailing comma.
        if let Ok(parsed) = syn::parse2::<PositionalKV>(list.tokens.clone()) {
            if let (Some(k), Some(v)) = (parsed.key, parsed.value) {
                return Some((k, v));
            }
        }
    }
    // Fall back to kv form. `parse_nested_meta` only invokes the closure
    // when a `key = value` pair is present, so this is safe to call on
    // an attribute that uses the positional form too.
    let mut out = None;
    let _ = attr.parse_nested_meta(|meta| {
        let key = meta
            .path
            .get_ident()
            .map(|i| i.to_string())
            .ok_or_else(|| meta.error("expected ident"))?;
        let value = read_meta_litstr(&meta).unwrap_or_default();
        out = Some((key, value));
        Ok(())
    });
    out
}

#[derive(Default)]
struct PositionalKV {
    key: Option<String>,
    value: Option<String>,
}

impl syn::parse::Parse for PositionalKV {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut me = Self::default();
        // Try the first positional LitStr.
        if let Ok(lit) = input.parse::<LitStr>() {
            me.key = Some(lit.value());
            // Skip optional comma.
            let _ = input.parse::<syn::Token![,]>();
            // Try the second positional LitStr.
            if let Ok(lit) = input.parse::<LitStr>() {
                me.value = Some(lit.value());
            }
        }
        Ok(me)
    }
}

#[derive(Default)]
struct RolesList {
    roles: Vec<String>,
}

impl syn::parse::Parse for RolesList {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut me = Self::default();
        while !input.is_empty() {
            let lit: LitStr = input.parse()?;
            me.roles.push(lit.value());
            if input.peek(syn::Token![,]) {
                let _: syn::Token![,] = input.parse()?;
            }
        }
        Ok(me)
    }
}

/// Multi-pair variant of `parse_kv_attr_args`. Collects every `key = value`
/// pair in an attribute's args. Used by `#[openapi(...)]` which carries
/// several (e.g. `summary = "..."`, `operation_id = "..."`).
fn parse_kv_attr_args_many(attr: &Attribute) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let _ = attr.parse_nested_meta(|meta| {
        if let Some(key) = meta.path.get_ident().map(|i| i.to_string()) {
            let value = read_meta_litstr(&meta).unwrap_or_default();
            out.push((key, value));
        }
        Ok(())
    });
    out
}

fn read_u64_arg(attr: &Attribute) -> Option<u64> {
    let lit: syn::LitInt = attr.parse_args().ok()?;
    lit.base10_parse().ok()
}

fn read_string_arg(attr: &Attribute) -> Option<String> {
    let lit: syn::LitStr = attr.parse_args().ok()?;
    Some(lit.value())
}

fn read_min_max(attr: &Attribute) -> Option<(u64, u64)> {
    let inner: syn::ExprTuple = attr.parse_args().ok()?;
    let mut iter = inner.elems.iter();
    let min = match iter.next()? {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => i.base10_parse().ok()?,
        _ => return None,
    };
    let _comma: Option<syn::Token![,]> = iter.next().map(|_| Default::default());
    let max = match iter.next()? {
        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(i), .. }) => i.base10_parse().ok()?,
        _ => return None,
    };
    Some((min, max))
}

/// Read the identifiers inside `( ... )` of a `#[attr(...)]`.
/// Returns them as strings (last path segment only, for readability).
fn read_idents_in_attr_args(attr: &Attribute) -> Vec<String> {
    let mut out = Vec::new();
    let _ = attr.parse_nested_meta(|meta| {
        if let Some(p) = meta.path.get_ident() {
            out.push(p.to_string());
        } else if let Some(s) = meta.path.segments.last() {
            out.push(s.ident.to_string());
        }
        Ok(())
    });
    out
}

/// Read `key = ( A, B, C )` — a tuple of idents under a key — into Vec<String>.
fn read_idents_in_paren(meta: &ParseNestedMeta<'_>) -> Result<Vec<String>, syn::Error> {
    let content;
    let inner = meta.value()?;
    syn::parenthesized!(content in inner);
    let list: syn::punctuated::Punctuated<syn::Path, syn::Token![,]> =
        content.parse_terminated(syn::Path::parse, syn::Token![,])?;
    Ok(list
        .iter()
        .map(|p| {
            p.segments
                .last()
                .map(|s| s.ident.to_string())
                .unwrap_or_else(|| "<anon>".into())
        })
        .collect())
}

fn last_path_segment(path: &syn::Path) -> Option<String> {
    path.segments.last().map(|s| s.ident.to_string())
}

fn last_path_segment_from_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(TypePath { path, .. }) => last_path_segment(path),
        _ => None,
    }
}

fn type_to_string(ty: &Type) -> String {
    match ty {
        Type::Path(TypePath { path, .. }) => {
            let mut out = String::new();
            for (i, seg) in path.segments.iter().enumerate() {
                if i > 0 {
                    out.push_str("::");
                }
                out.push_str(&seg.ident.to_string());
                match &seg.arguments {
                    PathArguments::None => {}
                    PathArguments::AngleBracketed(args) => {
                        out.push('<');
                        for (j, ga) in args.args.iter().enumerate() {
                            if j > 0 {
                                out.push_str(", ");
                            }
                            match ga {
                                GenericArgument::Type(t) => out.push_str(&type_to_string(t)),
                                GenericArgument::Lifetime(l) => out.push_str(&l.to_string()),
                                _ => out.push('…'),
                            }
                        }
                        out.push('>');
                    }
                    PathArguments::Parenthesized(_) => {
                        out.push('(');
                        out.push(')');
                    }
                }
            }
            out
        }
        Type::Tuple(t) => {
            if t.elems.is_empty() {
                "()".into()
            } else {
                let inner: Vec<String> = t.elems.iter().map(type_to_string).collect();
                format!("({})", inner.join(", "))
            }
        }
        _ => "<unknown>".into(),
    }
}

fn infer_module_path(file: &str) -> String {
    let trimmed = file.trim_start_matches("src/").trim_end_matches(".rs");
    if trimmed == "main" || trimmed == "lib" {
        return "crate".into();
    }
    trimmed.replace('/', "::")
}

// Public exports the ws_gateway/parser hints look for in v2.
#[allow(dead_code)]
pub(super) const WS_GATEWAY_ATTR: &str = NESTRS_ATTR_WS_GATEWAY;
#[allow(dead_code)]
pub(super) const WS_ROUTES_ATTR: &str = NESTRS_ATTR_WS_ROUTES;
#[allow(dead_code)]
pub(super) const MICRO_ROUTES_ATTR: &str = NESTRS_ATTR_MICRO_ROUTES;
