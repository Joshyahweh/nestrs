use proc_macro::TokenStream;
use quote::{format_ident, quote};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, DeriveInput, Expr, Field, Fields, Ident, ImplItemFn, Item, ItemFn,
    ItemStruct, LitInt, LitStr, Meta, Result, Token, Type,
};

struct ModuleArgs {
    imports: Vec<Expr>,
    controllers: Vec<Type>,
    providers: Vec<Type>,
    microservices: Option<Vec<Type>>,
    exports: Vec<Type>,
    re_exports: Vec<Type>,
}

impl Parse for ModuleArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut imports = Vec::new();
        let mut controllers = Vec::new();
        let mut providers = Vec::new();
        let mut microservices = None::<Vec<Type>>;
        let mut exports = Vec::new();
        let mut re_exports = Vec::new();

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            let content;
            syn::bracketed!(content in input);

            match key.to_string().as_str() {
                "imports" => {
                    let values: Punctuated<Expr, Token![,]> =
                        content.parse_terminated(Expr::parse, Token![,])?;
                    imports = values.into_iter().collect::<Vec<_>>();
                }
                "controllers" => {
                    let values: Punctuated<Type, Token![,]> =
                        content.parse_terminated(Type::parse, Token![,])?;
                    controllers = values.into_iter().collect::<Vec<_>>();
                }
                "providers" => {
                    let values: Punctuated<Type, Token![,]> =
                        content.parse_terminated(Type::parse, Token![,])?;
                    providers = values.into_iter().collect::<Vec<_>>();
                }
                "microservices" => {
                    let values: Punctuated<Type, Token![,]> =
                        content.parse_terminated(Type::parse, Token![,])?;
                    microservices = Some(values.into_iter().collect::<Vec<_>>());
                }
                "exports" => {
                    let values: Punctuated<Type, Token![,]> =
                        content.parse_terminated(Type::parse, Token![,])?;
                    exports = values.into_iter().collect::<Vec<_>>();
                }
                "re_exports" => {
                    let values: Punctuated<Type, Token![,]> =
                        content.parse_terminated(Type::parse, Token![,])?;
                    re_exports = values.into_iter().collect::<Vec<_>>();
                }
                _ => return Err(syn::Error::new(key.span(), "unknown module key")),
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(Self {
            imports,
            controllers,
            providers,
            microservices,
            exports,
            re_exports,
        })
    }
}

#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ModuleArgs);
    let module_struct = parse_macro_input!(item as ItemStruct);
    let name = &module_struct.ident;

    enum ImportKind {
        Normal,
        ForwardRef,
        Lazy,
    }

    struct ImportItem {
        ty: Type,
        kind: ImportKind,
    }

    fn lazy_static_ident(ty: &Type) -> Ident {
        let mut hasher = DefaultHasher::new();
        quote!(#ty).to_string().hash(&mut hasher);
        let h = hasher.finish();
        format_ident!("__NESTRS_LAZY_{:016x}", h)
    }

    fn forward_ref_expr_to_type(expr: &Expr) -> Option<Type> {
        fn is_forward_ref_ident(ident: &Ident) -> bool {
            ident == "forward_ref" || ident == "forwardRef"
        }

        let (path, args) = match expr {
            Expr::Call(call) => {
                let Expr::Path(p) = call.func.as_ref() else {
                    return None;
                };
                if !call.args.is_empty() {
                    return None;
                }
                let seg = p.path.segments.last()?;
                if !is_forward_ref_ident(&seg.ident) {
                    return None;
                }
                (p.path.clone(), &seg.arguments)
            }
            Expr::Path(p) => {
                let seg = p.path.segments.last()?;
                if !is_forward_ref_ident(&seg.ident) {
                    return None;
                }
                (p.path.clone(), &seg.arguments)
            }
            _ => return None,
        };

        let syn::PathArguments::AngleBracketed(ab) = args else {
            return None;
        };

        let ty = ab.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(t) => Some(t.clone()),
            _ => None,
        })?;

        // Preserve the qualified path for nicer error messages; only the type matters.
        let _ = path;
        Some(ty)
    }

    fn lazy_module_expr_to_type(expr: &Expr) -> Option<Type> {
        fn is_lazy_ident(ident: &Ident) -> bool {
            ident == "lazy_module" || ident == "lazy"
        }

        let args = match expr {
            Expr::Call(call) => {
                let Expr::Path(p) = call.func.as_ref() else {
                    return None;
                };
                if !call.args.is_empty() {
                    return None;
                }
                let seg = p.path.segments.last()?;
                if !is_lazy_ident(&seg.ident) {
                    return None;
                }
                &seg.arguments
            }
            Expr::Path(p) => {
                let seg = p.path.segments.last()?;
                if !is_lazy_ident(&seg.ident) {
                    return None;
                }
                &seg.arguments
            }
            _ => return None,
        };

        let syn::PathArguments::AngleBracketed(ab) = args else {
            return None;
        };

        ab.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(t) => Some(t.clone()),
            _ => None,
        })
    }

    let imports_exprs = args.imports;
    let mut imports_static = Vec::<ImportItem>::new();
    let mut imports_dynamic = Vec::<Expr>::new();
    for expr in imports_exprs {
        if let Some(ty) = forward_ref_expr_to_type(&expr) {
            imports_static.push(ImportItem {
                ty,
                kind: ImportKind::ForwardRef,
            });
            continue;
        }
        if let Some(ty) = lazy_module_expr_to_type(&expr) {
            imports_static.push(ImportItem {
                ty,
                kind: ImportKind::Lazy,
            });
            continue;
        }

        match expr {
            Expr::Path(p) => imports_static.push(ImportItem {
                ty: Type::Path(syn::TypePath {
                    qself: None,
                    path: p.path,
                }),
                kind: ImportKind::Normal,
            }),
            other => imports_dynamic.push(other),
        };
    }

    let mut lazy_seen: HashSet<String> = HashSet::new();
    let mut lazy_unique: Vec<Type> = Vec::new();
    for imp in &imports_static {
        if matches!(imp.kind, ImportKind::Lazy) {
            let ty = &imp.ty;
            let key = quote!(#ty).to_string();
            if lazy_seen.insert(key) {
                lazy_unique.push(imp.ty.clone());
            }
        }
    }
    let lazy_static_items = lazy_unique.iter().map(|ty| {
        let id = lazy_static_ident(ty);
        quote! {
            static #id: std::sync::OnceLock<nestrs::core::DynamicModule> =
                std::sync::OnceLock::new();
        }
    });

    let import_builds = imports_static
        .iter()
        .map(|imp| {
            let ty = &imp.ty;
            match imp.kind {
                ImportKind::ForwardRef => quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            // `forward_ref` back-edge: referenced module is already being built.
                            // Skip to avoid infinite recursion.
                        } else {
                            let (child_registry, child_router) = <#ty as nestrs::core::Module>::build();
                            let child_exports = <#ty as nestrs::core::Module>::exports();
                            registry.absorb_exported(child_registry, &child_exports);
                            router = router.merge(child_router);
                        }
                    }
                },
                ImportKind::Lazy => {
                    let lazy_ident = lazy_static_ident(ty);
                    quote! {
                        {
                            let __dm_lazy = #lazy_ident
                                .get_or_init(|| nestrs::core::DynamicModule::from_module::<#ty>());
                            registry.absorb_exported(__dm_lazy.registry.clone(), &__dm_lazy.exports);
                            router = router.merge(__dm_lazy.router.clone());
                        }
                    }
                }
                ImportKind::Normal => quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            nestrs::core::__nestrs_panic_circular_module_dependency(std::any::type_name::<#ty>());
                        }
                        let (child_registry, child_router) = <#ty as nestrs::core::Module>::build();
                        let child_exports = <#ty as nestrs::core::Module>::exports();
                        registry.absorb_exported(child_registry, &child_exports);
                        router = router.merge(child_router);
                    }
                },
            }
        })
        .collect::<Vec<_>>();

    let import_graph_providers = imports_static
        .iter()
        .map(|imp| {
            let ty = &imp.ty;
            match imp.kind {
                ImportKind::ForwardRef => quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            // `forward_ref` back-edge: referenced module is already being traversed.
                            // Skip to avoid infinite recursion.
                        } else {
                            <#ty as nestrs::core::ModuleGraph>::register_providers(registry);
                        }
                    }
                },
                ImportKind::Lazy => {
                    let lazy_ident = lazy_static_ident(ty);
                    quote! {
                        {
                            let __dm_lazy = #lazy_ident
                                .get_or_init(|| nestrs::core::DynamicModule::from_module::<#ty>());
                            registry.absorb_exported(__dm_lazy.registry.clone(), &__dm_lazy.exports);
                        }
                    }
                }
                ImportKind::Normal => quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            nestrs::core::__nestrs_panic_circular_module_dependency(std::any::type_name::<#ty>());
                        }
                        <#ty as nestrs::core::ModuleGraph>::register_providers(registry);
                    }
                },
            }
        })
        .collect::<Vec<_>>();

    let import_graph_controllers = imports_static
        .iter()
        .map(|imp| {
            let ty = &imp.ty;
            match imp.kind {
                ImportKind::ForwardRef => quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            // `forward_ref` back-edge: referenced module is already being traversed.
                            // Skip to avoid infinite recursion.
                        } else {
                            router = <#ty as nestrs::core::ModuleGraph>::register_controllers(router, registry);
                        }
                    }
                },
                ImportKind::Lazy => {
                    let lazy_ident = lazy_static_ident(ty);
                    quote! {
                        {
                            let __dm_lazy = #lazy_ident
                                .get_or_init(|| nestrs::core::DynamicModule::from_module::<#ty>());
                            router = router.merge(__dm_lazy.router.clone());
                        }
                    }
                }
                ImportKind::Normal => quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            nestrs::core::__nestrs_panic_circular_module_dependency(std::any::type_name::<#ty>());
                        }
                        router = <#ty as nestrs::core::ModuleGraph>::register_controllers(router, registry);
                    }
                },
            }
        })
        .collect::<Vec<_>>();

    let controllers = args.controllers;
    let providers = args.providers;
    let microservices = args.microservices.unwrap_or_default();
    let microservices_ref = &microservices;
    let exports = args.exports;
    let re_exports = args.re_exports;

    let microservices_impl = if microservices.is_empty() {
        quote! {}
    } else {
        quote! {
            impl nestrs::microservices::MicroserviceModule for #name {
                fn microservice_handlers() -> Vec<nestrs::microservices::MicroserviceHandlerFactory> {
                    vec![
                        #(
                            nestrs::microservices::handler_factory::<#microservices_ref>
                                as nestrs::microservices::MicroserviceHandlerFactory
                        ),*
                    ]
                }
            }
        }
    };

    let expanded = quote! {
        #module_struct

        #(#lazy_static_items)*

        impl nestrs::core::Module for #name {
            fn build() -> (nestrs::core::ProviderRegistry, axum::Router) {
                // Memoized process-wide: modules imported by multiple importers share one built
                // instance (NestJS module-instance caching), avoiding duplicate singletons and
                // duplicate route registration.
                nestrs::core::__nestrs_memoize_module_build::<#name>(Box::new(|| {
                    let _module_guard = nestrs::core::__NestrsModuleBuildGuard::push(
                        std::any::TypeId::of::<#name>(),
                        std::any::type_name::<#name>(),
                    );
                    let mut registry = nestrs::core::ProviderRegistry::new();
                    let mut router = axum::Router::new();

                    #(#import_builds)*

                    #(
                        {
                            let __dm: nestrs::core::DynamicModule = (#imports_dynamic);
                            registry.absorb_exported(__dm.registry, &__dm.exports);
                            router = router.merge(__dm.router);
                        }
                    )*

                    #(
                        registry.register::<#providers>();
                    )*

                    #(
                        registry.register::<#microservices_ref>();
                    )*

                    #(
                        router = <#controllers as nestrs::core::Controller>::register(router, &registry);
                    )*

                    (registry, router)
                }))
            }

            fn exports() -> Vec<std::any::TypeId> {
                let mut out = vec![
                    #(std::any::TypeId::of::<#exports>()),*
                ];
                #(
                    out.extend(<#re_exports as nestrs::core::Module>::exports());
                )*
                out
            }
        }

        impl nestrs::core::ModuleGraph for #name {
            fn register_providers(registry: &mut nestrs::core::ProviderRegistry) {
                let _module_guard = nestrs::core::__NestrsModuleBuildGuard::push(
                    std::any::TypeId::of::<#name>(),
                    std::any::type_name::<#name>(),
                );
                #(#import_graph_providers)*
                #(
                    {
                        let __dm: nestrs::core::DynamicModule = (#imports_dynamic);
                        registry.absorb_exported(__dm.registry, &__dm.exports);
                    }
                )*
                #(
                    registry.register::<#providers>();
                )*
                #(
                    registry.register::<#microservices_ref>();
                )*
            }

            fn register_controllers(
                mut router: axum::Router,
                registry: &nestrs::core::ProviderRegistry,
            ) -> axum::Router {
                let _module_guard = nestrs::core::__NestrsModuleBuildGuard::push(
                    std::any::TypeId::of::<#name>(),
                    std::any::type_name::<#name>(),
                );
                #(#import_graph_controllers)*
                #(
                    router = <#controllers as nestrs::core::Controller>::register(router, registry);
                )*
                router
            }
        }

        #microservices_impl
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn injectable(attr: TokenStream, item: TokenStream) -> TokenStream {
    #[derive(Default)]
    struct InjectableArgs {
        scope: Option<String>,
    }

    impl Parse for InjectableArgs {
        fn parse(input: ParseStream<'_>) -> Result<Self> {
            let mut args = InjectableArgs::default();

            while !input.is_empty() {
                let key: Ident = input.parse()?;
                input.parse::<Token![=]>()?;
                match key.to_string().as_str() {
                    "scope" => {
                        let v: LitStr = input.parse()?;
                        args.scope = Some(v.value());
                    }
                    _ => return Err(syn::Error::new(key.span(), "unknown injectable key")),
                }

                if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                }
            }

            Ok(args)
        }
    }

    let args = parse_macro_input!(attr as InjectableArgs);
    let item_struct = parse_macro_input!(item as ItemStruct);
    let name = &item_struct.ident;
    let scope_impl = match args.scope.as_deref() {
        None | Some("singleton") | Some("default") => quote! {},
        Some("transient") => quote! {
            fn scope() -> nestrs::core::ProviderScope {
                nestrs::core::ProviderScope::Transient
            }
        },
        Some("request") => quote! {
            fn scope() -> nestrs::core::ProviderScope {
                nestrs::core::ProviderScope::Request
            }
        },
        Some(other) => {
            return syn::Error::new_spanned(
                item_struct,
                format!(
                    "unsupported injectable scope `{other}` (expected singleton|transient|request)"
                ),
            )
            .to_compile_error()
            .into();
        }
    };

    let construct_body = match &item_struct.fields {
        Fields::Unit => {
            quote! {
                std::sync::Arc::new(#name)
            }
        }
        Fields::Named(named) => {
            let assignments = named.named.iter().map(|field| {
                let field_ident = field.ident.as_ref().expect("named field should have ident");
                let ty = &field.ty;

                // Inject `Arc<T>` fields from the registry.
                let Type::Path(tp) = ty else {
                    return syn::Error::new_spanned(
                        ty,
                        "injectable currently supports fields typed `Arc<T>` only",
                    )
                    .to_compile_error();
                };
                let seg = tp
                    .path
                    .segments
                    .last()
                    .cloned()
                    .expect("path has at least one segment");
                if seg.ident != "Arc" {
                    return syn::Error::new_spanned(
                        ty,
                        "injectable currently supports fields typed `Arc<T>` only",
                    )
                    .to_compile_error();
                }
                let syn::PathArguments::AngleBracketed(args) = seg.arguments else {
                    return syn::Error::new_spanned(ty, "Arc field must be `Arc<T>`")
                        .to_compile_error();
                };
                let inner = args
                    .args
                    .iter()
                    .filter_map(|a| match a {
                        syn::GenericArgument::Type(t) => Some(t),
                        _ => None,
                    })
                    .next();
                let Some(inner) = inner else {
                    return syn::Error::new_spanned(ty, "Arc field must be `Arc<T>`")
                        .to_compile_error();
                };

                // Trait-object bindings (`arc_dyn: Arc<dyn MyTrait>`) resolve the registered
                // `Arc<dyn MyTrait>` value (register it via `ProviderRegistry::register_use_factory`
                // with `T = Arc<dyn MyTrait>`).
                if matches!(inner, Type::TraitObject(_)) {
                    quote! {
                        #field_ident: (*registry.get::<#inner>()).clone()
                    }
                } else {
                    quote! {
                        #field_ident: registry.get::<#inner>()
                    }
                }
            });

            quote! {
                std::sync::Arc::new(Self {
                    #(#assignments,)*
                })
            }
        }
        Fields::Unnamed(_) => {
            return syn::Error::new_spanned(
                item_struct,
                "injectable currently supports unit structs and named-field structs only",
            )
            .to_compile_error()
            .into();
        }
    };
    let expanded = quote! {
        #item_struct

        impl nestrs::core::Injectable for #name {
            fn construct(_registry: &nestrs::core::ProviderRegistry) -> std::sync::Arc<Self> {
                let registry = _registry;
                #construct_body
            }

            #scope_impl
        }
    };
    expanded.into()
}

#[proc_macro_attribute]
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut item_struct = parse_macro_input!(item as ItemStruct);
    let attr_tokens = proc_macro2::TokenStream::from(attr);
    let mut version_from_attr = None::<String>;
    item_struct.attrs.retain(|a| {
        if a.path().is_ident("version") || a.path().is_ident("__nestrs_version_marker") {
            if let Meta::List(list) = &a.meta {
                if let Ok(v) = syn::parse2::<LitStr>(list.tokens.clone()) {
                    version_from_attr = Some(v.value());
                }
            }
            false
        } else {
            true
        }
    });

    let (prefix, mut version, host) = if attr_tokens.is_empty() {
        ("/".to_string(), "".to_string(), None::<String>)
    } else if let Ok(v) = syn::parse2::<LitStr>(attr_tokens.clone()) {
        (v.value(), "".to_string(), None::<String>)
    } else {
        let mut prefix = "/".to_string();
        let mut version = "".to_string();
        let mut host: Option<String> = None;
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("prefix") {
                let value: LitStr = meta.value()?.parse()?;
                prefix = value.value();
                Ok(())
            } else if meta.path.is_ident("version") {
                let value: LitStr = meta.value()?.parse()?;
                version = value.value();
                Ok(())
            } else if meta.path.is_ident("host") {
                let value: LitStr = meta.value()?.parse()?;
                host = Some(value.value());
                Ok(())
            } else {
                Err(meta.error("unknown controller key; expected `prefix`, `version`, or `host`"))
            }
        });

        if parser.parse2(attr_tokens.clone()).is_err() {
            return syn::Error::new_spanned(
                item_struct,
                "controller expects `#[controller]`, `#[controller(\"/x\")]`, or `#[controller(prefix = \"/x\", version = \"v1\")]`",
            )
            .to_compile_error()
            .into();
        }
        (prefix, version, host)
    };
    if version.is_empty() {
        if let Some(v) = version_from_attr {
            version = v;
        }
    }

    let name = &item_struct.ident;
    let host_fn = match &host {
        Some(h) => quote! {
            pub fn __nestrs_host() -> Option<&'static str> {
                Some(#h)
            }
        },
        None => quote! {
            pub fn __nestrs_host() -> Option<&'static str> {
                None
            }
        },
    };
    let expanded = quote! {
        #item_struct

        impl #name {
            pub fn __nestrs_prefix() -> &'static str {
                #prefix
            }

            pub fn __nestrs_version() -> &'static str {
                #version
            }

            #host_fn
        }
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn version(attr: TokenStream, item: TokenStream) -> TokenStream {
    let version = parse_macro_input!(attr as LitStr);
    let mut parsed_item = parse_macro_input!(item as Item);
    let marker: syn::Attribute = syn::parse_quote!(#[__nestrs_version_marker(#version)]);

    match &mut parsed_item {
        Item::Struct(item_struct) => {
            item_struct.attrs.push(marker);
            quote!(#item_struct).into()
        }
        _ => syn::Error::new_spanned(
            parsed_item,
            "version currently supports structs only (for use with #[controller])",
        )
        .to_compile_error()
        .into(),
    }
}

fn passthrough(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
#[proc_macro_attribute]
pub fn patch(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
#[proc_macro_attribute]
pub fn options(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
#[proc_macro_attribute]
pub fn head(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}
#[proc_macro_attribute]
pub fn all(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn openapi(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn sse(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn raw_body(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

struct RoutesArgs {
    state: Type,
    controller_guards: Option<Type>,
}

impl Parse for RoutesArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut state = None::<Type>;
        let mut controller_guards = None::<Type>;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "state" => state = Some(input.parse()?),
                "controller_guards" => controller_guards = Some(input.parse()?),
                _ => return Err(syn::Error::new(key.span(), "unknown routes key")),
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        let Some(state) = state else {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "routes requires `state = SomeProviderType`",
            ));
        };

        Ok(Self {
            state,
            controller_guards,
        })
    }
}

struct WsGatewayArgs {
    path: LitStr,
}

impl Parse for WsGatewayArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        if input.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "ws_gateway requires `path = \"/ws\"`",
            ));
        }

        // Allow `#[ws_gateway(\"/ws\")]` or `#[ws_gateway(path = \"/ws\")]`.
        if input.peek(LitStr) {
            let path: LitStr = input.parse()?;
            return Ok(Self { path });
        }

        let key: Ident = input.parse()?;
        input.parse::<Token![=]>()?;
        if key != "path" {
            return Err(syn::Error::new(
                key.span(),
                "ws_gateway expects `path = \"/ws\"`",
            ));
        }
        let path: LitStr = input.parse()?;
        Ok(Self { path })
    }
}

#[derive(Clone, Copy)]
enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
    All,
}

impl HttpMethod {
    fn to_ident(self) -> Ident {
        Ident::new(
            match self {
                HttpMethod::Get => "GET",
                HttpMethod::Post => "POST",
                HttpMethod::Put => "PUT",
                HttpMethod::Patch => "PATCH",
                HttpMethod::Delete => "DELETE",
                HttpMethod::Options => "OPTIONS",
                HttpMethod::Head => "HEAD",
                HttpMethod::All => "ALL",
            },
            proc_macro2::Span::call_site(),
        )
    }
}

struct RouteDef {
    method: HttpMethod,
    path: LitStr,
    handler: Ident,
    version: Option<LitStr>,
    guards: Vec<Type>,
    #[allow(dead_code)]
    pipes: Vec<Type>,
    interceptors: Vec<Type>,
    filters: Vec<Type>,
    metadata: Vec<(LitStr, LitStr)>,
    /// Empty, or `openapi <expr>` for `impl_routes!`.
    openapi_line: proc_macro2::TokenStream,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParamDecorator {
    Body,
    Query,
    Param,
    Req,
    Headers,
    Ip,
}

fn path_ends_with_2(path: &syn::Path, a: &str, b: &str) -> bool {
    let segs = &path.segments;
    if segs.len() < 2 {
        return false;
    }
    let last = segs.last().unwrap().ident.to_string();
    let prev = segs.iter().nth(segs.len() - 2).unwrap().ident.to_string();
    prev == a && last == b
}

fn param_decorator_from_attr(attr: &syn::Attribute) -> Option<ParamDecorator> {
    let p = attr.path();
    if path_ends_with_2(p, "param", "body") {
        Some(ParamDecorator::Body)
    } else if path_ends_with_2(p, "param", "query") {
        Some(ParamDecorator::Query)
    } else if path_ends_with_2(p, "param", "param") {
        Some(ParamDecorator::Param)
    } else if path_ends_with_2(p, "param", "req") {
        Some(ParamDecorator::Req)
    } else if path_ends_with_2(p, "param", "headers") {
        Some(ParamDecorator::Headers)
    } else if path_ends_with_2(p, "param", "ip") {
        Some(ParamDecorator::Ip)
    } else {
        None
    }
}

fn is_validation_pipe(ty: &Type) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    let Some(seg) = tp.path.segments.last() else {
        return false;
    };
    seg.ident == "ValidationPipe"
}

fn parse_subscribe_message(attrs: &[syn::Attribute]) -> Result<Option<LitStr>> {
    let mut out = None::<LitStr>;
    for attr in attrs {
        if !attr.path().is_ident("subscribe_message") {
            continue;
        }
        if out.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "subscribe_message can only be specified once per handler",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "subscribe_message expects a string literal, e.g. #[subscribe_message(\"ping\")]",
            ));
        };
        if list.tokens.is_empty() {
            return Err(syn::Error::new_spanned(
                list,
                "subscribe_message expects a string literal, e.g. #[subscribe_message(\"ping\")]",
            ));
        }
        let lit = syn::parse2::<LitStr>(list.tokens.clone()).map_err(|_| {
            syn::Error::new_spanned(
                list,
                "subscribe_message expects a string literal, e.g. #[subscribe_message(\"ping\")]",
            )
        })?;
        out = Some(lit);
    }
    Ok(out)
}

fn parse_message_pattern(attrs: &[syn::Attribute]) -> Result<Option<LitStr>> {
    let mut out = None::<LitStr>;
    for attr in attrs {
        if !attr.path().is_ident("message_pattern") {
            continue;
        }
        if out.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "message_pattern can only be specified once per handler",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "message_pattern expects a string literal, e.g. #[message_pattern(\"user.get\")]",
            ));
        };
        if list.tokens.is_empty() {
            return Err(syn::Error::new_spanned(
                list,
                "message_pattern expects a string literal, e.g. #[message_pattern(\"user.get\")]",
            ));
        }
        let lit = syn::parse2::<LitStr>(list.tokens.clone()).map_err(|_| {
            syn::Error::new_spanned(
                list,
                "message_pattern expects a string literal, e.g. #[message_pattern(\"user.get\")]",
            )
        })?;
        out = Some(lit);
    }
    Ok(out)
}

fn parse_event_pattern(attrs: &[syn::Attribute]) -> Result<Option<LitStr>> {
    let mut out = None::<LitStr>;
    for attr in attrs {
        if !attr.path().is_ident("event_pattern") {
            continue;
        }
        if out.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "event_pattern can only be specified once per handler",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "event_pattern expects a string literal, e.g. #[event_pattern(\"user.created\")]",
            ));
        };
        if list.tokens.is_empty() {
            return Err(syn::Error::new_spanned(
                list,
                "event_pattern expects a string literal, e.g. #[event_pattern(\"user.created\")]",
            ));
        }
        let lit = syn::parse2::<LitStr>(list.tokens.clone()).map_err(|_| {
            syn::Error::new_spanned(
                list,
                "event_pattern expects a string literal, e.g. #[event_pattern(\"user.created\")]",
            )
        })?;
        out = Some(lit);
    }
    Ok(out)
}

fn parse_on_event(attrs: &[syn::Attribute]) -> Result<Option<LitStr>> {
    let mut out = None::<LitStr>;
    for attr in attrs {
        if !attr.path().is_ident("on_event") {
            continue;
        }
        if out.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "on_event can only be specified once per handler",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "on_event expects a string literal, e.g. #[on_event(\"order.created\")]",
            ));
        };
        if list.tokens.is_empty() {
            return Err(syn::Error::new_spanned(
                list,
                "on_event expects a string literal, e.g. #[on_event(\"order.created\")]",
            ));
        }
        let lit = syn::parse2::<LitStr>(list.tokens.clone()).map_err(|_| {
            syn::Error::new_spanned(
                list,
                "on_event expects a string literal, e.g. #[on_event(\"order.created\")]",
            )
        })?;
        out = Some(lit);
    }
    Ok(out)
}

fn parse_cron(attrs: &[syn::Attribute]) -> Result<Option<LitStr>> {
    let mut out = None::<LitStr>;
    for attr in attrs {
        if !attr.path().is_ident("cron") {
            continue;
        }
        if out.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "cron can only be specified once per handler",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "cron expects a string literal, e.g. #[cron(\"0 * * * * *\")]",
            ));
        };
        if list.tokens.is_empty() {
            return Err(syn::Error::new_spanned(
                list,
                "cron expects a string literal, e.g. #[cron(\"0 * * * * *\")]",
            ));
        }
        let lit = syn::parse2::<LitStr>(list.tokens.clone()).map_err(|_| {
            syn::Error::new_spanned(
                list,
                "cron expects a string literal, e.g. #[cron(\"0 * * * * *\")]",
            )
        })?;
        out = Some(lit);
    }
    Ok(out)
}

fn parse_interval(attrs: &[syn::Attribute]) -> Result<Option<LitInt>> {
    let mut out = None::<LitInt>;
    for attr in attrs {
        if !attr.path().is_ident("interval") {
            continue;
        }
        if out.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "interval can only be specified once per handler",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "interval expects an integer literal in milliseconds, e.g. #[interval(30_000)]",
            ));
        };
        if list.tokens.is_empty() {
            return Err(syn::Error::new_spanned(
                list,
                "interval expects an integer literal in milliseconds, e.g. #[interval(30_000)]",
            ));
        }
        let lit = syn::parse2::<LitInt>(list.tokens.clone()).map_err(|_| {
            syn::Error::new_spanned(
                list,
                "interval expects an integer literal in milliseconds, e.g. #[interval(30_000)]",
            )
        })?;
        out = Some(lit);
    }
    Ok(out)
}

fn is_ws_client_type(ty: &Type) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    let Some(seg) = tp.path.segments.last() else {
        return false;
    };
    seg.ident == "WsClient"
}

fn is_serde_json_value_type(ty: &Type) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    let segs = &tp.path.segments;
    let Some(last) = segs.last() else {
        return false;
    };
    if last.ident != "Value" {
        return false;
    }
    if segs.len() >= 2 {
        let prev = segs.iter().nth(segs.len() - 2).unwrap();
        return prev.ident == "serde_json";
    }
    false
}

fn is_transport_error_type(ty: &Type) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    let Some(seg) = tp.path.segments.last() else {
        return false;
    };
    seg.ident == "TransportError"
}

fn is_http_exception_type(ty: &Type) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    let Some(seg) = tp.path.segments.last() else {
        return false;
    };
    seg.ident == "HttpException"
}

fn split_result_type(ty: &Type) -> Option<(Type, Type)> {
    let Type::Path(tp) = ty else {
        return None;
    };
    let seg = tp.path.segments.last()?;
    if seg.ident != "Result" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return None;
    };
    let mut it = args.args.iter().filter_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    });
    let ok = it.next()?;
    let err = it.next()?;
    Some((ok, err))
}

fn parse_route_method(attrs: &[syn::Attribute]) -> Result<Option<(HttpMethod, LitStr)>> {
    for attr in attrs {
        let Some(ident) = attr.path().get_ident().cloned() else {
            continue;
        };
        let method = match ident.to_string().as_str() {
            "get" => HttpMethod::Get,
            "post" => HttpMethod::Post,
            "put" => HttpMethod::Put,
            "patch" => HttpMethod::Patch,
            "delete" => HttpMethod::Delete,
            "options" => HttpMethod::Options,
            "head" => HttpMethod::Head,
            "all" => HttpMethod::All,
            _ => continue,
        };

        let path = match &attr.meta {
            Meta::Path(_) => LitStr::new("/", attr.span()),
            Meta::List(list) => {
                if list.tokens.is_empty() {
                    LitStr::new("/", attr.span())
                } else {
                    syn::parse2::<LitStr>(list.tokens.clone()).map_err(|_| {
                        syn::Error::new_spanned(
                            list,
                            "route attribute expects a string literal path, e.g. #[get(\"/\")]",
                        )
                    })?
                }
            }
            Meta::NameValue(_) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "route attribute expects #[get(\"/\")] syntax",
                ));
            }
        };
        return Ok(Some((method, path)));
    }
    Ok(None)
}

fn parse_route_version(attrs: &[syn::Attribute]) -> Result<Option<LitStr>> {
    for attr in attrs {
        if !attr.path().is_ident("ver") {
            continue;
        }
        match &attr.meta {
            Meta::Path(_) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "ver expects a version string, e.g. #[ver(\"v2\")]",
                ));
            }
            Meta::List(list) => {
                if list.tokens.is_empty() {
                    return Err(syn::Error::new_spanned(
                        list,
                        "ver expects a version string, e.g. #[ver(\"v2\")]",
                    ));
                }
                let lit = syn::parse2::<LitStr>(list.tokens.clone()).map_err(|_| {
                    syn::Error::new_spanned(
                        list,
                        "ver expects a version string, e.g. #[ver(\"v2\")]",
                    )
                })?;
                return Ok(Some(lit));
            }
            Meta::NameValue(_) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "ver expects #[ver(\"v2\")] syntax",
                ));
            }
        }
    }
    Ok(None)
}

fn parse_use_guards(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_guards") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_guards expects types, e.g. #[use_guards(AuthGuard, RolesGuard)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let guards: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_guards expects types, e.g. #[use_guards(AuthGuard, RolesGuard)]",
                )
            })?;
        return Ok(guards.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_pipes(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_pipes") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_pipes expects types, e.g. #[use_pipes(ValidationPipe)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let values: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_pipes expects types, e.g. #[use_pipes(ValidationPipe)]",
                )
            })?;
        return Ok(values.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_ws_guards(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_ws_guards") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_ws_guards expects types, e.g. #[use_ws_guards(MyWsGuard)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let guards: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_ws_guards expects types, e.g. #[use_ws_guards(MyWsGuard)]",
                )
            })?;
        return Ok(guards.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_ws_pipes(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_ws_pipes") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_ws_pipes expects types, e.g. #[use_ws_pipes(MyWsPipe)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let values: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_ws_pipes expects types, e.g. #[use_ws_pipes(MyWsPipe)]",
                )
            })?;
        return Ok(values.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_ws_interceptors(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_ws_interceptors") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_ws_interceptors expects types, e.g. #[use_ws_interceptors(LogWs)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let values: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_ws_interceptors expects types, e.g. #[use_ws_interceptors(LogWs)]",
                )
            })?;
        return Ok(values.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_micro_guards(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_micro_guards") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_micro_guards expects types, e.g. #[use_micro_guards(MyMicroGuard)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let guards: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_micro_guards expects types, e.g. #[use_micro_guards(MyMicroGuard)]",
                )
            })?;
        return Ok(guards.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_micro_pipes(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_micro_pipes") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_micro_pipes expects types, e.g. #[use_micro_pipes(MyMicroPipe)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let values: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_micro_pipes expects types, e.g. #[use_micro_pipes(MyMicroPipe)]",
                )
            })?;
        return Ok(values.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_micro_interceptors(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_micro_interceptors") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_micro_interceptors expects types, e.g. #[use_micro_interceptors(LogMicro)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let values: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_micro_interceptors expects types, e.g. #[use_micro_interceptors(LogMicro)]",
                )
            })?;
        return Ok(values.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_interceptors(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_interceptors") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_interceptors expects types, e.g. #[use_interceptors(LoggingInterceptor)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let values: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_interceptors expects types, e.g. #[use_interceptors(LoggingInterceptor)]",
                )
            })?;
        return Ok(values.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_use_filters(attrs: &[syn::Attribute]) -> Result<Vec<Type>> {
    for attr in attrs {
        if !attr.path().is_ident("use_filters") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "use_filters expects types, e.g. #[use_filters(HttpExceptionFilter)]",
            ));
        };
        if list.tokens.is_empty() {
            return Ok(Vec::new());
        }
        let values: Punctuated<Type, Token![,]> = Punctuated::<Type, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "use_filters expects types, e.g. #[use_filters(HttpExceptionFilter)]",
                )
            })?;
        return Ok(values.into_iter().collect());
    }
    Ok(Vec::new())
}

fn parse_set_metadata(attrs: &[syn::Attribute]) -> Result<Vec<(LitStr, LitStr)>> {
    let mut out = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("set_metadata") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "set_metadata expects two string literals, e.g. #[set_metadata(\"roles\", \"admin\")]",
            ));
        };
        let args: Punctuated<LitStr, Token![,]> =
            Punctuated::<LitStr, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .map_err(|_| {
                    syn::Error::new_spanned(
                        list,
                        "set_metadata expects two string literals, e.g. #[set_metadata(\"roles\", \"admin\")]",
                    )
                })?;
        let mut it = args.into_iter();
        let key = it.next().ok_or_else(|| {
            syn::Error::new_spanned(
                list,
                "set_metadata expects two string literals, e.g. #[set_metadata(\"roles\", \"admin\")]",
            )
        })?;
        let value = it.next().ok_or_else(|| {
            syn::Error::new_spanned(
                list,
                "set_metadata expects two string literals, e.g. #[set_metadata(\"roles\", \"admin\")]",
            )
        })?;
        out.push((key, value));
    }
    Ok(out)
}

fn parse_roles(attrs: &[syn::Attribute]) -> Result<Vec<(LitStr, LitStr)>> {
    let mut out = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("roles") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "roles expects one or more string literals, e.g. #[roles(\"admin\")]",
            ));
        };
        let args: Punctuated<LitStr, Token![,]> = Punctuated::<LitStr, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "roles expects one or more string literals, e.g. #[roles(\"admin\")]",
                )
            })?;
        let joined = args.iter().map(|s| s.value()).collect::<Vec<_>>().join(",");
        out.push((
            LitStr::new("roles", attr.span()),
            LitStr::new(&joined, attr.span()),
        ));
    }
    Ok(out)
}

/// Parse `#[check_policies("read:Post", "update:User")]` and emit a single
/// `("check_policies", "read:Post,update:User")` metadata entry. Same shape as
/// `parse_roles` (CSV in the value field) so it slots into the existing
/// metadata pipeline without a new registry API.
fn parse_check_policies(attrs: &[syn::Attribute]) -> Result<Vec<(LitStr, LitStr)>> {
    let mut out = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("check_policies") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "check_policies expects one or more string literals, e.g. #[check_policies(\"read:Post\")]",
            ));
        };
        let args: Punctuated<LitStr, Token![,]> = Punctuated::<LitStr, Token![,]>::parse_terminated
            .parse2(list.tokens.clone())
            .map_err(|_| {
                syn::Error::new_spanned(
                    list,
                    "check_policies expects action:Subject tokens, e.g. #[check_policies(\"read:Post\", \"update:User\")]",
                )
            })?;
        let joined = args.iter().map(|s| s.value()).collect::<Vec<_>>().join(",");
        out.push((
            LitStr::new("check_policies", attr.span()),
            LitStr::new(&joined, attr.span()),
        ));
    }
    Ok(out)
}

/// Parse `#[throttle(5, "minute")]` → `("throttle", "5/minute")` and the bare
/// `#[skip_throttle]` → `("skip_throttle", "true")`. Same CSV-style metadata
/// pipeline as `parse_check_policies`; the runtime side (`nestrs::throttler`)
/// parses the value back into a `ThrottleSpec`.
fn parse_throttle(attrs: &[syn::Attribute]) -> Result<Vec<(LitStr, LitStr)>> {
    struct ThrottleArgs {
        limit: LitInt,
        per: LitStr,
    }
    impl Parse for ThrottleArgs {
        fn parse(input: ParseStream<'_>) -> Result<Self> {
            let limit: LitInt = input.parse()?;
            input.parse::<Token![,]>()?;
            let per: LitStr = input.parse()?;
            Ok(ThrottleArgs { limit, per })
        }
    }

    let mut out = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("skip_throttle") {
            out.push((
                LitStr::new("skip_throttle", attr.span()),
                LitStr::new("true", attr.span()),
            ));
            continue;
        }
        if !attr.path().is_ident("throttle") {
            continue;
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "throttle expects #[throttle(n, \"second\"|\"minute\"|\"hour\")]",
            ));
        };
        let args: ThrottleArgs = syn::parse2(list.tokens.clone()).map_err(|_| {
            syn::Error::new_spanned(
                list,
                "throttle expects #[throttle(n, \"second\"|\"minute\"|\"hour\")]",
            )
        })?;
        let per = args.per.value();
        if !matches!(per.as_str(), "second" | "minute" | "hour") {
            return Err(syn::Error::new_spanned(
                &args.per,
                "throttle window must be \"second\", \"minute\" or \"hour\"",
            ));
        }
        let limit: u64 = args.limit.base10_parse().map_err(|_| {
            syn::Error::new_spanned(&args.limit, "throttle limit must be a positive integer")
        })?;
        if limit == 0 {
            return Err(syn::Error::new_spanned(
                &args.limit,
                "throttle limit must be at least 1",
            ));
        }
        out.push((
            LitStr::new("throttle", attr.span()),
            LitStr::new(&format!("{limit}/{per}"), attr.span()),
        ));
    }
    Ok(out)
}

/// Parse the health probe decorators — bare `#[liveness]`, `#[readiness]`,
/// `#[startup]` on a route handler → `("probe", "<kind>")`. Same CSV-style
/// metadata pipeline as `parse_throttle`; the runtime side
/// (`nestrs::health_probes`) resolves the stamped handlers at router-build
/// time and mounts the `/__nestrs/health/{live,ready,startup}` endpoints.
/// More than one probe decorator on the same handler is a compile error.
fn parse_probe(attrs: &[syn::Attribute]) -> Result<Vec<(LitStr, LitStr)>> {
    let mut out = Vec::new();
    for attr in attrs {
        let kind = if attr.path().is_ident("liveness") {
            "liveness"
        } else if attr.path().is_ident("readiness") {
            "readiness"
        } else if attr.path().is_ident("startup") {
            "startup"
        } else {
            continue;
        };
        if let Meta::List(_) | Meta::NameValue(_) = &attr.meta {
            return Err(syn::Error::new_spanned(
                attr,
                format!("#[{kind}] takes no arguments"),
            ));
        }
        if out
            .iter()
            .any(|(k, _): &(LitStr, LitStr)| k.value() == "probe")
        {
            return Err(syn::Error::new_spanned(
                attr,
                "only one probe decorator (`#[liveness]` / `#[readiness]` / `#[startup]`) per route handler",
            ));
        }
        out.push((
            LitStr::new("probe", attr.span()),
            LitStr::new(kind, attr.span()),
        ));
    }
    Ok(out)
}

struct OpenApiResponsePair(LitInt, LitStr);

impl Parse for OpenApiResponsePair {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let status: LitInt = content.parse()?;
        content.parse::<Token![,]>()?;
        let desc: LitStr = content.parse()?;
        Ok(Self(status, desc))
    }
}

struct OpenApiAttrBody {
    summary: Option<LitStr>,
    tag: Option<LitStr>,
    responses: Vec<(LitInt, LitStr)>,
}

impl Parse for OpenApiAttrBody {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut summary = None::<LitStr>;
        let mut tag = None::<LitStr>;
        let mut responses = Vec::<(LitInt, LitStr)>::new();

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match key.to_string().as_str() {
                "summary" => {
                    if summary.is_some() {
                        return Err(syn::Error::new_spanned(
                            key,
                            "duplicate `summary` in #[openapi(...)]",
                        ));
                    }
                    summary = Some(input.parse::<LitStr>()?);
                }
                "tag" => {
                    if tag.is_some() {
                        return Err(syn::Error::new_spanned(
                            key,
                            "duplicate `tag` in #[openapi(...)]",
                        ));
                    }
                    tag = Some(input.parse::<LitStr>()?);
                }
                "responses" => {
                    if !responses.is_empty() {
                        return Err(syn::Error::new_spanned(
                            key,
                            "duplicate `responses` in #[openapi(...)]",
                        ));
                    }
                    let content;
                    syn::parenthesized!(content in input);
                    let pairs: Punctuated<OpenApiResponsePair, Token![,]> =
                        content.parse_terminated(OpenApiResponsePair::parse, Token![,])?;
                    for pair in pairs {
                        let OpenApiResponsePair(st, ds) = pair;
                        let v: u128 = st.base10_parse().map_err(|_| {
                            syn::Error::new_spanned(&st, "response status must be a valid integer")
                        })?;
                        if v > u16::MAX as u128 {
                            return Err(syn::Error::new_spanned(
                                st,
                                "HTTP status code must fit in u16",
                            ));
                        }
                        responses.push((st, ds));
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        key,
                        "unknown #[openapi(...)] field (expected summary, tag, responses)",
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(OpenApiAttrBody {
            summary,
            tag,
            responses,
        })
    }
}

fn parse_openapi(attrs: &[syn::Attribute]) -> Result<Option<OpenApiAttrBody>> {
    let mut found = None::<OpenApiAttrBody>;
    for attr in attrs {
        if !attr.path().is_ident("openapi") {
            continue;
        }
        if found.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "#[openapi(...)] can only appear once per handler",
            ));
        }
        let Meta::List(list) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "#[openapi] expects parentheses, e.g. #[openapi(summary = \"...\")]",
            ));
        };
        let parsed: OpenApiAttrBody = syn::parse2(list.tokens.clone())?;
        found = Some(parsed);
    }
    Ok(found)
}

fn openapi_impl_line(body: &OpenApiAttrBody) -> proc_macro2::TokenStream {
    if body.summary.is_none() && body.tag.is_none() && body.responses.is_empty() {
        return quote! {};
    }
    let summary = match &body.summary {
        Some(s) => quote! { ::core::option::Option::Some(#s) },
        None => quote! { ::core::option::Option::None },
    };
    let tag = match &body.tag {
        Some(s) => quote! { ::core::option::Option::Some(#s) },
        None => quote! { ::core::option::Option::None },
    };
    let pairs = body
        .responses
        .iter()
        .map(|(st, ds)| quote! { (#st as u16, #ds) });
    quote! {
        openapi ( nestrs::__nestrs_openapi_spec_leaked(#summary, #tag, &[ #(#pairs),* ]) )
    }
}

#[proc_macro_attribute]
pub fn routes(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as RoutesArgs);
    let mut item_impl = parse_macro_input!(item as syn::ItemImpl);

    if item_impl.trait_.is_some() {
        return syn::Error::new_spanned(item_impl, "routes supports inherent impl blocks only")
            .to_compile_error()
            .into();
    }

    let self_ty = item_impl.self_ty.clone();

    let controller_ident = match &*self_ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map(|s| s.ident.clone())
            .unwrap_or_else(|| Ident::new("Controller", proc_macro2::Span::call_site())),
        _ => Ident::new("Controller", proc_macro2::Span::call_site()),
    };

    let mut routes = Vec::<RouteDef>::new();
    for it in &mut item_impl.items {
        let syn::ImplItem::Fn(func) = it else {
            continue;
        };

        let (method, path) = match parse_route_method(&func.attrs) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => return e.to_compile_error().into(),
        };

        let version = match parse_route_version(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        let guards = match parse_use_guards(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        let pipes = match parse_use_pipes(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        // Two cases for parameter binding today:
        //   1. No pipes — use the raw axum extractor.
        //   2. Any pipes that include ValidationPipe (last segment ident match)
        //      — use the legacy Validated* extractor that hard-codes
        //      `value.validate()` and returns UnprocessableEntityException.
        //      Preserved bit-for-bit so existing `#[use_pipes(ValidationPipe)]`
        //      users don't observe any behavioral change.
        //
        // NOTE: chains containing non-ValidationPipe pipes (e.g.
        // `#[use_pipes(ParseIntPipe)]`) are intentionally NOT auto-rewritten
        // here yet — the per-arity Piped* extractors exist and work but the
        // macro plumbing to feed them needs an output-type flow that proc
        // macros can't introspect today without specialization. Users who
        // want a runtime pipe chain can write the extractor explicitly:
        //   #[use_pipes(ParseIntPipe)]
        //   #[get("/int")] async fn f(
        //       #[param::query] raw: nestrs::PipedQuery1<String, ParseIntPipe>,
        //   ) -> ... { raw.0 }
        // See `nestrs/tests/pipes_chain.rs`.
        let has_validation = pipes.iter().any(is_validation_pipe);

        // Expand Nest-like parameter decorators into Axum extractors.
        for input in func.sig.inputs.iter_mut() {
            let syn::FnArg::Typed(pat_ty) = input else {
                continue;
            };

            let decorators = pat_ty
                .attrs
                .iter()
                .filter_map(param_decorator_from_attr)
                .collect::<Vec<_>>();
            if decorators.len() > 1 {
                return syn::Error::new_spanned(
                    &pat_ty.attrs[0],
                    "only one #[param::...] decorator is allowed per parameter",
                )
                .to_compile_error()
                .into();
            }
            let decorator = decorators.first().copied();
            pat_ty
                .attrs
                .retain(|a| param_decorator_from_attr(a).is_none());

            let Some(decorator) = decorator else {
                continue;
            };

            let inner_pat = (*pat_ty.pat).clone();
            let inner_ty = (*pat_ty.ty).clone();

            match decorator {
                ParamDecorator::Body => {
                    if has_validation {
                        pat_ty.pat = syn::parse_quote!(nestrs::ValidatedBody(#inner_pat));
                        pat_ty.ty = syn::parse_quote!(nestrs::ValidatedBody<#inner_ty>);
                    } else {
                        pat_ty.pat = syn::parse_quote!(nestrs::axum::Json(#inner_pat));
                        pat_ty.ty = syn::parse_quote!(nestrs::axum::Json<#inner_ty>);
                    }
                }
                ParamDecorator::Query => {
                    if has_validation {
                        pat_ty.pat = syn::parse_quote!(nestrs::ValidatedQuery(#inner_pat));
                        pat_ty.ty = syn::parse_quote!(nestrs::ValidatedQuery<#inner_ty>);
                    } else {
                        pat_ty.pat = syn::parse_quote!(nestrs::axum::extract::Query(#inner_pat));
                        pat_ty.ty = syn::parse_quote!(nestrs::axum::extract::Query<#inner_ty>);
                    }
                }
                ParamDecorator::Param => {
                    if has_validation {
                        pat_ty.pat = syn::parse_quote!(nestrs::ValidatedPath(#inner_pat));
                        pat_ty.ty = syn::parse_quote!(nestrs::ValidatedPath<#inner_ty>);
                    } else {
                        pat_ty.pat = syn::parse_quote!(nestrs::axum::extract::Path(#inner_pat));
                        pat_ty.ty = syn::parse_quote!(nestrs::axum::extract::Path<#inner_ty>);
                    }
                }
                ParamDecorator::Req => {
                    pat_ty.ty = syn::parse_quote!(nestrs::axum::extract::Request);
                }
                ParamDecorator::Headers => {
                    pat_ty.ty = syn::parse_quote!(nestrs::axum::http::HeaderMap);
                }
                ParamDecorator::Ip => {
                    pat_ty.pat = syn::parse_quote!(nestrs::ClientIp(#inner_pat));
                    pat_ty.ty = syn::parse_quote!(nestrs::ClientIp);
                }
            }
        }

        let interceptors = match parse_use_interceptors(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        let filters = match parse_use_filters(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        let mut metadata = match parse_set_metadata(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        match parse_roles(&func.attrs) {
            Ok(v) => metadata.extend(v),
            Err(e) => return e.to_compile_error().into(),
        }
        match parse_check_policies(&func.attrs) {
            Ok(v) => metadata.extend(v),
            Err(e) => return e.to_compile_error().into(),
        }
        match parse_throttle(&func.attrs) {
            Ok(v) => metadata.extend(v),
            Err(e) => return e.to_compile_error().into(),
        }
        match parse_probe(&func.attrs) {
            Ok(v) => metadata.extend(v),
            Err(e) => return e.to_compile_error().into(),
        }

        let openapi_line = match parse_openapi(&func.attrs) {
            Ok(Some(body)) => openapi_impl_line(&body),
            Ok(None) => quote! {},
            Err(e) => return e.to_compile_error().into(),
        };
        func.attrs.retain(|a| !a.path().is_ident("openapi"));

        routes.push(RouteDef {
            method,
            path,
            handler: func.sig.ident.clone(),
            version,
            guards,
            pipes,
            interceptors,
            filters,
            metadata,
            openapi_line,
        });
    }

    if routes.is_empty() {
        let msg = format!(
            "routes found no #[get]/#[post]/... handlers in impl {controller_ident} {{ ... }}",
        );
        return syn::Error::new_spanned(item_impl, msg)
            .to_compile_error()
            .into();
    }

    let state_ty = args.state;
    let controller_guards = args.controller_guards;

    let route_entries = routes
        .into_iter()
        .map(|r| {
            let method = r.method.to_ident();
            let path = r.path;
            let handler_name = r.handler;
            let handler = quote!(#self_ty::#handler_name);
            let guards = r.guards;
            let interceptors = r.interceptors;
            let filters = r.filters;
            let metadata = r.metadata;
            let openapi_line = r.openapi_line;
            let maybe_ver = r.version.map(|v| quote!(@ver(#v)));
            let interceptors_tokens = if interceptors.is_empty() {
                quote! {}
            } else {
                quote! { interceptors ( #(#interceptors),* ) }
            };
            let filters_tokens = if filters.is_empty() {
                quote! {}
            } else {
                quote! { filters ( #(#filters),* ) }
            };
            let metadata_tokens = if metadata.is_empty() {
                quote! {}
            } else {
                let keys = metadata.iter().map(|(k, _)| k);
                let values = metadata.iter().map(|(_, v)| v);
                quote! { metadata ( #( ( #keys, #values ) ),* ) }
            };

            quote! {
                #maybe_ver
                    #method #path
                    #openapi_line
                    with ( #(#guards),* )
                    #interceptors_tokens
                    #filters_tokens
                    #metadata_tokens
                    => #handler,
            }
        })
        .collect::<Vec<_>>();

    let register = if let Some(ctrl_guard) = controller_guards {
        quote! {
            nestrs::impl_routes!(#self_ty, state #state_ty, controller_guards ( #ctrl_guard ) => [
                #(#route_entries)*
            ]);
        }
    } else {
        quote! {
            nestrs::impl_routes!(#self_ty, state #state_ty => [
                #(#route_entries)*
            ]);
        }
    };

    let expanded = quote! {
        #item_impl
        #register
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn subscribe_message(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn ws_routes(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_impl = parse_macro_input!(item as syn::ItemImpl);

    if item_impl.trait_.is_some() {
        return syn::Error::new_spanned(item_impl, "ws_routes supports inherent impl blocks only")
            .to_compile_error()
            .into();
    }

    let self_ty = item_impl.self_ty.clone();

    struct WsHandlerDef {
        event: LitStr,
        name: Ident,
        expects_client: bool,
        payload_ty: Option<Type>,
        ws_interceptors: Vec<Type>,
        ws_guards: Vec<Type>,
        ws_pipes: Vec<Type>,
    }

    let mut handlers = Vec::<WsHandlerDef>::new();
    for it in &item_impl.items {
        let syn::ImplItem::Fn(func) = it else {
            continue;
        };
        let event = match parse_subscribe_message(&func.attrs) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => return e.to_compile_error().into(),
        };

        // Require `&self` receiver so we can call methods from `WsGateway::on_message(&self, ...)`.
        let mut inputs = func.sig.inputs.iter();
        let Some(first) = inputs.next() else {
            return syn::Error::new_spanned(
                func,
                "subscribe_message handlers must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        let syn::FnArg::Receiver(recv) = first else {
            return syn::Error::new_spanned(
                first,
                "subscribe_message handlers must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        if recv.reference.is_none() {
            return syn::Error::new_spanned(
                recv,
                "subscribe_message handlers must use `&self` receiver",
            )
            .to_compile_error()
            .into();
        }
        if recv.mutability.is_some() {
            return syn::Error::new_spanned(
                recv,
                "subscribe_message handlers must use `&self` receiver (not `&mut self`)",
            )
            .to_compile_error()
            .into();
        }

        let typed_args = inputs
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(pat_ty) => Some((*pat_ty.ty).clone()),
                syn::FnArg::Receiver(_) => None,
            })
            .collect::<Vec<_>>();

        if typed_args.len() > 2 {
            return syn::Error::new_spanned(
                func,
                "subscribe_message handlers support at most: (&self, WsClient?, Payload?)",
            )
            .to_compile_error()
            .into();
        }

        let (expects_client, payload_ty) = match typed_args.as_slice() {
            [] => (false, None),
            [a] if is_ws_client_type(a) => (true, None),
            [payload_ty] => (false, Some(payload_ty.clone())),
            [a, payload_ty] if is_ws_client_type(a) => (true, Some(payload_ty.clone())),
            _ => {
                return syn::Error::new_spanned(
                    func.sig.clone(),
                    "subscribe_message handlers must be one of: (&self), (&self, WsClient), (&self, Payload), (&self, WsClient, Payload)",
                )
                .to_compile_error()
                .into();
            }
        };

        let ws_interceptors = match parse_use_ws_interceptors(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        let ws_guards = match parse_use_ws_guards(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        let ws_pipes = match parse_use_ws_pipes(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        handlers.push(WsHandlerDef {
            event,
            name: func.sig.ident.clone(),
            expects_client,
            payload_ty,
            ws_interceptors,
            ws_guards,
            ws_pipes,
        });
    }

    if handlers.is_empty() {
        return syn::Error::new_spanned(
            item_impl,
            "ws_routes found no #[subscribe_message(\"...\")] handlers in this impl block",
        )
        .to_compile_error()
        .into();
    }

    let mut arms = Vec::new();
    for h in handlers {
        let event = h.event;
        let name = h.name;
        let expects_client = h.expects_client;
        let payload_ty = h.payload_ty;
        let ws_interceptors = h.ws_interceptors;
        let ws_guards = h.ws_guards;
        let ws_pipes = h.ws_pipes;

        let call = match (expects_client, payload_ty) {
            (false, None) => quote! {
                let _ = self.#name().await;
            },
            (true, None) => quote! {
                let _ = self.#name(client.clone()).await;
            },
            (false, Some(payload_ty)) => {
                if is_serde_json_value_type(&payload_ty) {
                    quote! {
                        let __pl = __ws_payload.clone();
                        let _ = self.#name(__pl).await;
                    }
                } else {
                    quote! {
                        let __pl = __ws_payload.clone();
                        let __value: #payload_ty = match nestrs::serde_json::from_value(__pl) {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = client.emit(
                                    nestrs::ws::WS_ERROR_EVENT,
                                    nestrs::serde_json::json!({
                                        "event": #event,
                                        "message": "invalid payload",
                                        "details": e.to_string()
                                    }),
                                );
                                return;
                            }
                        };
                        let _ = self.#name(__value).await;
                    }
                }
            }
            (true, Some(payload_ty)) => {
                if is_serde_json_value_type(&payload_ty) {
                    quote! {
                        let __pl = __ws_payload.clone();
                        let _ = self.#name(client.clone(), __pl).await;
                    }
                } else {
                    quote! {
                        let __pl = __ws_payload.clone();
                        let __value: #payload_ty = match nestrs::serde_json::from_value(__pl) {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = client.emit(
                                    nestrs::ws::WS_ERROR_EVENT,
                                    nestrs::serde_json::json!({
                                        "event": #event,
                                        "message": "invalid payload",
                                        "details": e.to_string()
                                    }),
                                );
                                return;
                            }
                        };
                        let _ = self.#name(client.clone(), __value).await;
                    }
                }
            }
        };

        let inter_ts: Vec<_> = ws_interceptors
            .iter()
            .map(|t| {
                quote! {
                    <#t as ::core::default::Default>::default()
                        .before_handle(client.handshake(), #event, &__ws_payload)
                        .await;
                }
            })
            .collect();

        let guard_ts: Vec<_> = ws_guards
            .iter()
            .map(|g| {
                quote! {
                    if let Err(__e) = <#g as ::core::default::Default>::default()
                        .can_activate_ws(client.handshake(), #event, &__ws_payload)
                        .await
                    {
                        let _ = client.emit(nestrs::ws::WS_ERROR_EVENT, __e.to_json());
                        return;
                    }
                }
            })
            .collect();

        let pipe_ts: Vec<_> = ws_pipes
            .iter()
            .map(|p| {
                quote! {
                    __ws_payload = match <#p as ::core::default::Default>::default()
                        .transform(#event, __ws_payload)
                        .await
                    {
                        Ok(__v) => __v,
                        Err(__e) => {
                            let _ = client.emit(nestrs::ws::WS_ERROR_EVENT, __e.to_json());
                            return;
                        }
                    };
                }
            })
            .collect();

        arms.push(quote! {
            #event => {
                let mut __ws_payload = payload.clone();
                #(#inter_ts)*
                #(#guard_ts)*
                #(#pipe_ts)*
                #call
            }
        });
    }

    let expanded = quote! {
        #item_impl

        #[nestrs::async_trait]
        impl nestrs::ws::WsGateway for #self_ty {
            async fn on_message(
                &self,
                client: nestrs::ws::WsClient,
                event: &str,
                payload: nestrs::serde_json::Value,
            ) {
                match event {
                    #(#arms,)*
                    _ => {
                        let _ = client.emit(
                            nestrs::ws::WS_ERROR_EVENT,
                            nestrs::serde_json::json!({
                                "event": event,
                                "message": "unknown event"
                            }),
                        );
                    }
                }
            }
        }
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn ws_gateway(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as WsGatewayArgs);
    let item_struct = parse_macro_input!(item as ItemStruct);
    let name = &item_struct.ident;
    let path = args.path;

    let expanded = quote! {
        #item_struct

        impl nestrs::core::Controller for #name {
            fn register(
                router: nestrs::axum::Router,
                registry: &nestrs::core::ProviderRegistry
            ) -> nestrs::axum::Router {
                let gateway = registry.get::<#name>();
                router.route(#path, nestrs::ws::ws_route(gateway))
            }
        }
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn event_routes(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_impl = parse_macro_input!(item as syn::ItemImpl);

    if item_impl.trait_.is_some() {
        return syn::Error::new_spanned(
            item_impl,
            "event_routes supports inherent impl blocks only",
        )
        .to_compile_error()
        .into();
    }

    let self_ty = item_impl.self_ty.clone();

    struct EventHandlerDef {
        pattern: LitStr,
        name: Ident,
        payload_ty: Option<Type>,
    }

    let mut handlers = Vec::<EventHandlerDef>::new();

    for it in &item_impl.items {
        let syn::ImplItem::Fn(func) = it else {
            continue;
        };

        let pattern = match parse_on_event(&func.attrs) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(e) => return e.to_compile_error().into(),
        };

        if func.sig.asyncness.is_none() {
            return syn::Error::new_spanned(func.sig.clone(), "on_event handlers must be async")
                .to_compile_error()
                .into();
        }

        // Require `&self` receiver so we can call methods from the subscription closure.
        let mut inputs = func.sig.inputs.iter();
        let Some(first) = inputs.next() else {
            return syn::Error::new_spanned(
                func,
                "on_event handlers must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        let syn::FnArg::Receiver(recv) = first else {
            return syn::Error::new_spanned(
                first,
                "on_event handlers must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        if recv.reference.is_none() {
            return syn::Error::new_spanned(recv, "on_event handlers must use `&self` receiver")
                .to_compile_error()
                .into();
        }
        if recv.mutability.is_some() {
            return syn::Error::new_spanned(
                recv,
                "on_event handlers must use `&self` receiver (not `&mut self`)",
            )
            .to_compile_error()
            .into();
        }

        let typed_args = inputs
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(pat_ty) => Some((*pat_ty.ty).clone()),
                syn::FnArg::Receiver(_) => None,
            })
            .collect::<Vec<_>>();

        if typed_args.len() > 1 {
            return syn::Error::new_spanned(
                func,
                "on_event handlers support at most: (&self, Payload?)",
            )
            .to_compile_error()
            .into();
        }

        let payload_ty = typed_args.into_iter().next();

        handlers.push(EventHandlerDef {
            pattern,
            name: func.sig.ident.clone(),
            payload_ty,
        });
    }

    if handlers.is_empty() {
        return syn::Error::new_spanned(
            item_impl,
            "event_routes found no #[on_event(\"...\")] handlers in this impl block",
        )
        .to_compile_error()
        .into();
    }

    let subscribe_stmts = handlers
        .into_iter()
        .map(|h| {
            let pattern = h.pattern;
            let name = h.name;
            let payload_ty = h.payload_ty;

            let call = match payload_ty {
                None => quote! {
                    let _ = service.#name().await;
                },
                Some(payload_ty) if is_serde_json_value_type(&payload_ty) => quote! {
                    let payload = payload.clone();
                    let _ = service.#name(payload).await;
                },
                Some(payload_ty) => quote! {
                    let decoded: #payload_ty = match nestrs::serde_json::from_value(payload.clone()) {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                    let _ = service.#name(decoded).await;
                },
            };

            quote! {
                bus.subscribe(#pattern, {
                    let service = service.clone();
                    move |payload: nestrs::serde_json::Value| {
                        let service = service.clone();
                        async move {
                            #call
                        }
                    }
                });
            }
        })
        .collect::<Vec<_>>();

    let expanded = quote! {
        #item_impl

        const _: () = {
            fn __nestrs_register(registry: &nestrs::core::ProviderRegistry) {
                let bus = registry.get::<nestrs::EventBus>();
                let service = registry.get::<#self_ty>();
                #(#subscribe_stmts)*
            }

            #[nestrs::microservices::linkme::distributed_slice(nestrs::microservices::ON_EVENT_REGISTRATIONS)]
            static __NES_ON_EVENT: nestrs::microservices::OnEventRegistration =
                nestrs::microservices::OnEventRegistration { register: __nestrs_register };
        };
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn schedule_routes(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_impl = parse_macro_input!(item as syn::ItemImpl);

    if item_impl.trait_.is_some() {
        return syn::Error::new_spanned(
            item_impl,
            "schedule_routes supports inherent impl blocks only",
        )
        .to_compile_error()
        .into();
    }

    let self_ty = item_impl.self_ty.clone();

    enum TaskKind {
        Cron(LitStr),
        Interval(LitInt),
    }

    struct TaskDef {
        kind: TaskKind,
        name: Ident,
    }

    let mut tasks = Vec::<TaskDef>::new();

    for it in &item_impl.items {
        let syn::ImplItem::Fn(func) = it else {
            continue;
        };

        let cron = match parse_cron(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        let interval = match parse_interval(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        let kind = match (cron, interval) {
            (None, None) => continue,
            (Some(c), None) => TaskKind::Cron(c),
            (None, Some(i)) => TaskKind::Interval(i),
            (Some(_), Some(_)) => {
                return syn::Error::new_spanned(
                    func,
                    "scheduled task cannot have both #[cron] and #[interval]",
                )
                .to_compile_error()
                .into();
            }
        };

        if func.sig.asyncness.is_none() {
            return syn::Error::new_spanned(func.sig.clone(), "scheduled tasks must be async")
                .to_compile_error()
                .into();
        }

        let mut inputs = func.sig.inputs.iter();
        let Some(first) = inputs.next() else {
            return syn::Error::new_spanned(
                func,
                "scheduled tasks must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        let syn::FnArg::Receiver(recv) = first else {
            return syn::Error::new_spanned(
                first,
                "scheduled tasks must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        if recv.reference.is_none() {
            return syn::Error::new_spanned(recv, "scheduled tasks must use `&self` receiver")
                .to_compile_error()
                .into();
        }
        if recv.mutability.is_some() {
            return syn::Error::new_spanned(
                recv,
                "scheduled tasks must use `&self` receiver (not `&mut self`)",
            )
            .to_compile_error()
            .into();
        }

        let typed_args = inputs
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(pat_ty) => Some((*pat_ty.ty).clone()),
                syn::FnArg::Receiver(_) => None,
            })
            .collect::<Vec<_>>();

        if !typed_args.is_empty() {
            return syn::Error::new_spanned(func, "scheduled tasks must be one of: (&self)")
                .to_compile_error()
                .into();
        }

        tasks.push(TaskDef {
            kind,
            name: func.sig.ident.clone(),
        });
    }

    if tasks.is_empty() {
        return syn::Error::new_spanned(
            item_impl,
            "schedule_routes found no #[cron(\"...\")] or #[interval(...)] tasks in this impl block",
        )
        .to_compile_error()
        .into();
    }

    let mut cron_stmts = Vec::new();
    let mut interval_stmts = Vec::new();
    for t in tasks {
        let name = t.name;
        match t.kind {
            TaskKind::Cron(expr) => {
                cron_stmts.push(quote! {
                    let job = nestrs::schedule::Job::new_async(#expr, {
                        let service = service.clone();
                        move |_uuid, _lock| {
                            let service = service.clone();
                            ::std::boxed::Box::pin(async move {
                                let _ = service.#name().await;
                            })
                        }
                    })
                    .unwrap_or_else(|e| panic!("failed to register cron job: {e:?}"));
                    jobs.push(job);
                });
            }
            TaskKind::Interval(ms) => {
                // Native tokio timer loop — tokio-cron-scheduler truncates repeat
                // periods to whole seconds, which breaks sub-second intervals.
                interval_stmts.push(quote! {
                    futures.push(::std::boxed::Box::pin(async move {
                        let mut timer = nestrs::schedule::tokio::time::interval(
                            ::std::time::Duration::from_millis(#ms as u64),
                        );
                        timer.set_missed_tick_behavior(
                            nestrs::schedule::tokio::time::MissedTickBehavior::Skip,
                        );
                        loop {
                            timer.tick().await;
                            let _ = service.#name().await;
                        }
                    }));
                });
            }
        }
    }

    let expanded = quote! {
        #item_impl

        const _: () = {
            fn __nestrs_build(registry: &nestrs::core::ProviderRegistry) -> ::std::vec::Vec<nestrs::schedule::Job> {
                let service = registry.get::<#self_ty>();
                let mut jobs = ::std::vec::Vec::<nestrs::schedule::Job>::new();
                #(#cron_stmts)*
                jobs
            }

            fn __nestrs_build_intervals(
                registry: &nestrs::core::ProviderRegistry,
            ) -> ::std::vec::Vec<nestrs::schedule::IntervalFuture> {
                let service = registry.get::<#self_ty>();
                let mut futures = ::std::vec::Vec::<nestrs::schedule::IntervalFuture>::new();
                #(#interval_stmts)*
                futures
            }

            #[nestrs::schedule::linkme::distributed_slice(nestrs::schedule::SCHEDULE_REGISTRATIONS)]
            static __NES_SCHEDULE: nestrs::schedule::ScheduleRegistration =
                nestrs::schedule::ScheduleRegistration {
                    build: __nestrs_build,
                    build_intervals: __nestrs_build_intervals,
                };
        };
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn micro_routes(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_impl = parse_macro_input!(item as syn::ItemImpl);

    if item_impl.trait_.is_some() {
        return syn::Error::new_spanned(
            item_impl,
            "micro_routes supports inherent impl blocks only",
        )
        .to_compile_error()
        .into();
    }

    let self_ty = item_impl.self_ty.clone();

    struct MsHandlerDef {
        pattern: LitStr,
        name: Ident,
        is_message: bool,
        payload_ty: Option<Type>,
        ok_ty: Option<Type>,
        err_ty: Option<Type>,
        micro_interceptors: Vec<Type>,
        micro_guards: Vec<Type>,
        micro_pipes: Vec<Type>,
    }

    let mut handlers = Vec::<MsHandlerDef>::new();

    for it in &item_impl.items {
        let syn::ImplItem::Fn(func) = it else {
            continue;
        };

        let msg = match parse_message_pattern(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        let evt = match parse_event_pattern(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        let (pattern, is_message) = match (msg, evt) {
            (Some(m), None) => (m, true),
            (None, Some(e)) => (e, false),
            (None, None) => continue,
            (Some(_), Some(_)) => {
                return syn::Error::new_spanned(
                    func,
                    "handler cannot have both #[message_pattern] and #[event_pattern]",
                )
                .to_compile_error()
                .into();
            }
        };

        if func.sig.asyncness.is_none() {
            return syn::Error::new_spanned(
                func.sig.clone(),
                "microservice handlers must be async",
            )
            .to_compile_error()
            .into();
        }

        // Require `&self`.
        let mut inputs = func.sig.inputs.iter();
        let Some(first) = inputs.next() else {
            return syn::Error::new_spanned(
                func,
                "microservice handlers must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        let syn::FnArg::Receiver(recv) = first else {
            return syn::Error::new_spanned(
                first,
                "microservice handlers must be methods with `&self` receiver",
            )
            .to_compile_error()
            .into();
        };
        if recv.reference.is_none() || recv.mutability.is_some() {
            return syn::Error::new_spanned(
                recv,
                "microservice handlers must use `&self` receiver",
            )
            .to_compile_error()
            .into();
        }

        let typed_args = inputs
            .filter_map(|arg| match arg {
                syn::FnArg::Typed(pat_ty) => Some((*pat_ty.ty).clone()),
                syn::FnArg::Receiver(_) => None,
            })
            .collect::<Vec<_>>();

        if typed_args.len() > 1 {
            return syn::Error::new_spanned(
                func.sig.clone(),
                "microservice handlers support at most one payload parameter",
            )
            .to_compile_error()
            .into();
        }

        let payload_ty = typed_args.first().cloned();

        let (ok_ty, err_ty) = if is_message {
            match &func.sig.output {
                syn::ReturnType::Default => (Some(syn::parse_quote!(())), None),
                syn::ReturnType::Type(_, ty) => {
                    if let Some((ok, err)) = split_result_type(ty) {
                        (Some(ok), Some(err))
                    } else {
                        (Some((**ty).clone()), None)
                    }
                }
            }
        } else {
            // Event handlers don't produce responses.
            (None, None)
        };

        if let Some(ref err) = err_ty {
            if !(is_transport_error_type(err) || is_http_exception_type(err)) {
                return syn::Error::new_spanned(
                    err,
                    "message_pattern handlers returning Result must use HttpException or TransportError as the error type",
                )
                .to_compile_error()
                .into();
            }
        }

        let micro_interceptors = match parse_use_micro_interceptors(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        let micro_guards = match parse_use_micro_guards(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };
        let micro_pipes = match parse_use_micro_pipes(&func.attrs) {
            Ok(v) => v,
            Err(e) => return e.to_compile_error().into(),
        };

        handlers.push(MsHandlerDef {
            pattern,
            name: func.sig.ident.clone(),
            is_message,
            payload_ty,
            ok_ty,
            err_ty,
            micro_interceptors,
            micro_guards,
            micro_pipes,
        });
    }

    let mut message_arms = Vec::new();
    let mut event_arms = Vec::new();

    for h in handlers {
        if h.is_message {
            let pattern = h.pattern;
            let name = h.name;
            let payload_ty = h.payload_ty.clone();
            let ok_ty = h.ok_ty.clone();
            let err_ty = h.err_ty.clone();
            let micro_interceptors = h.micro_interceptors.clone();
            let micro_guards = h.micro_guards.clone();
            let micro_pipes = h.micro_pipes.clone();

            let micro_inter_ts: Vec<_> = micro_interceptors
                .iter()
                .map(|t| {
                    quote! {
                        <#t as ::core::default::Default>::default()
                            .before_handle_micro(#pattern, &__ms_payload)
                            .await;
                    }
                })
                .collect();

            let micro_guard_ts: Vec<_> = micro_guards
                .iter()
                .map(|g| {
                    quote! {
                        if let Err(__e) = <#g as ::core::default::Default>::default()
                            .can_activate_micro(#pattern, &__ms_payload)
                            .await
                        {
                            return Some(Err(__e));
                        }
                    }
                })
                .collect();

            let micro_pipe_ts: Vec<_> = micro_pipes
                .iter()
                .map(|p| {
                    quote! {
                        __ms_payload = match <#p as ::core::default::Default>::default()
                            .transform_micro(#pattern, __ms_payload)
                            .await
                        {
                            Ok(__v) => __v,
                            Err(__e) => return Some(Err(__e)),
                        };
                    }
                })
                .collect();

            let decode = if let Some(payload_ty) = payload_ty.clone() {
                if is_serde_json_value_type(&payload_ty) {
                    quote! { let __payload = __ms_payload.clone(); }
                } else {
                    quote! {
                        let __payload: #payload_ty = match nestrs::serde_json::from_value(__ms_payload.clone()) {
                            Ok(v) => v,
                            Err(e) => {
                                return Some(Err(nestrs::microservices::TransportError::new(format!(
                                    "invalid payload for `{}`: {}",
                                    #pattern,
                                    e
                                ))));
                            }
                        };
                    }
                }
            } else {
                quote! {}
            };

            let call = match (payload_ty.as_ref(), ok_ty.as_ref(), err_ty.as_ref()) {
                (None, Some(ok_ty), None) => quote! {
                    let __out: #ok_ty = self.#name().await;
                    nestrs::serde_json::to_value(__out)
                        .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}")))
                },
                (Some(_payload_ty), Some(ok_ty), None) => {
                    quote! {
                        let __out: #ok_ty = self.#name(__payload).await;
                        nestrs::serde_json::to_value(__out)
                            .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}")))
                    }
                }
                (None, Some(_ok_ty), Some(err_ty)) => {
                    if is_transport_error_type(err_ty) {
                        quote! {
                            match self.#name().await {
                                Ok(v) => nestrs::serde_json::to_value(v)
                                    .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}"))),
                                Err(e) => Err(e),
                            }
                        }
                    } else {
                        // HttpException
                        quote! {
                            match self.#name().await {
                                Ok(v) => nestrs::serde_json::to_value(v)
                                    .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}"))),
                                Err(ex) => {
                                    let details = nestrs::serde_json::json!({
                                        "type": "HttpException",
                                        "statusCode": ex.status.as_u16(),
                                        "message": ex.message,
                                        "error": ex.error,
                                        "errors": ex.details,
                                    });
                                    Err(nestrs::microservices::TransportError::new("microservice handler threw HttpException").with_details(details))
                                }
                            }
                        }
                    }
                }
                (Some(_payload_ty), Some(_ok_ty), Some(err_ty)) => {
                    let pass_payload = quote! { __payload };

                    if is_transport_error_type(err_ty) {
                        quote! {
                            match self.#name(#pass_payload).await {
                                Ok(v) => nestrs::serde_json::to_value(v)
                                    .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}"))),
                                Err(e) => Err(e),
                            }
                        }
                    } else {
                        quote! {
                            match self.#name(#pass_payload).await {
                                Ok(v) => nestrs::serde_json::to_value(v)
                                    .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}"))),
                                Err(ex) => {
                                    let details = nestrs::serde_json::json!({
                                        "type": "HttpException",
                                        "statusCode": ex.status.as_u16(),
                                        "message": ex.message,
                                        "error": ex.error,
                                        "errors": ex.details,
                                    });
                                    Err(nestrs::microservices::TransportError::new("microservice handler threw HttpException").with_details(details))
                                }
                            }
                        }
                    }
                }
                _ => {
                    return syn::Error::new_spanned(
                        &name,
                        "unsupported message_pattern handler signature",
                    )
                    .to_compile_error()
                    .into();
                }
            };

            message_arms.push(quote! {
                #pattern => {
                    let mut __ms_payload = payload.clone();
                    #(#micro_inter_ts)*
                    #(#micro_guard_ts)*
                    #(#micro_pipe_ts)*
                    #decode
                    Some({
                        #call
                    })
                }
            });
        } else {
            let pattern = h.pattern;
            let name = h.name;
            let payload_ty = h.payload_ty.clone();
            let micro_interceptors = h.micro_interceptors.clone();
            let micro_guards = h.micro_guards.clone();
            let micro_pipes = h.micro_pipes.clone();

            let micro_inter_ts: Vec<_> = micro_interceptors
                .iter()
                .map(|t| {
                    quote! {
                        <#t as ::core::default::Default>::default()
                            .before_handle_micro(#pattern, &__ms_payload)
                            .await;
                    }
                })
                .collect();

            let micro_guard_ts: Vec<_> = micro_guards
                .iter()
                .map(|g| {
                    quote! {
                        if <#g as ::core::default::Default>::default()
                            .can_activate_micro(#pattern, &__ms_payload)
                            .await
                            .is_err()
                        {
                            return true;
                        }
                    }
                })
                .collect();

            let micro_pipe_ts: Vec<_> = micro_pipes
                .iter()
                .map(|p| {
                    quote! {
                        __ms_payload = match <#p as ::core::default::Default>::default()
                            .transform_micro(#pattern, __ms_payload)
                            .await
                        {
                            Ok(__v) => __v,
                            Err(_) => return true,
                        };
                    }
                })
                .collect();

            let decode = if let Some(payload_ty) = payload_ty.clone() {
                if is_serde_json_value_type(&payload_ty) {
                    quote! { let __payload = __ms_payload.clone(); }
                } else {
                    quote! {
                        let __payload: #payload_ty = match nestrs::serde_json::from_value(__ms_payload.clone()) {
                            Ok(v) => v,
                            Err(_) => {
                                return true;
                            }
                        };
                    }
                }
            } else {
                quote! {}
            };

            let call = match payload_ty.as_ref() {
                None => quote! { let _ = self.#name().await; },
                Some(_payload_ty) => quote! { let _ = self.#name(__payload).await; },
            };

            event_arms.push(quote! {
                #pattern => {
                    let mut __ms_payload = payload.clone();
                    #(#micro_inter_ts)*
                    #(#micro_guard_ts)*
                    #(#micro_pipe_ts)*
                    #decode
                    #call
                    true
                }
            });
        }
    }

    if message_arms.is_empty() && event_arms.is_empty() {
        return syn::Error::new_spanned(
            item_impl,
            "micro_routes found no #[message_pattern] or #[event_pattern] handlers in this impl block",
        )
        .to_compile_error()
        .into();
    }

    let expanded = quote! {
        #item_impl

        #[nestrs::async_trait]
        impl nestrs::microservices::MicroserviceHandler for #self_ty {
            async fn handle_message(
                &self,
                pattern: &str,
                payload: nestrs::serde_json::Value,
            ) -> Option<Result<nestrs::serde_json::Value, nestrs::microservices::TransportError>> {
                match pattern {
                    #(#message_arms,)*
                    _ => None,
                }
            }

            async fn handle_event(&self, pattern: &str, payload: nestrs::serde_json::Value) -> bool {
                match pattern {
                    #(#event_arms,)*
                    _ => false,
                }
            }
        }
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn ver(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_guards(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_pipes(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_interceptors(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_ws_guards(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_ws_pipes(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_ws_interceptors(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_micro_guards(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_micro_pipes(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_micro_interceptors(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn use_filters(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn set_metadata(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn roles(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn check_policies(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn throttle(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn skip_throttle(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

/// `#[liveness]` — marks the decorated route handler as the process
/// liveness probe. `GET /__nestrs/health/live` mirrors the handler's
/// status (2xx ⇒ up, otherwise down). Consumed by `#[routes]` as route
/// metadata (`probe => liveness`); the macro itself is a passthrough.
#[proc_macro_attribute]
pub fn liveness(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

/// `#[readiness]` — marks the decorated route handler as the readiness
/// probe, mirrored at `GET /__nestrs/health/ready`.
#[proc_macro_attribute]
pub fn readiness(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

/// `#[startup]` — marks the decorated route handler as the startup probe,
/// mirrored at `GET /__nestrs/health/startup`. The outcome is evaluated
/// once per process and cached; subsequent probes return the first result.
#[proc_macro_attribute]
pub fn startup(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn message_pattern(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn event_pattern(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn on_event(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn cron(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn interval(attr: TokenStream, item: TokenStream) -> TokenStream {
    passthrough(attr, item)
}

#[proc_macro_attribute]
pub fn queue_processor(attr: TokenStream, item: TokenStream) -> TokenStream {
    let queue = parse_macro_input!(attr as LitStr);
    let item_struct = parse_macro_input!(item as ItemStruct);
    let name = &item_struct.ident;

    let expanded = quote! {
        #item_struct

        const _: () = {
            #[nestrs::queues::linkme::distributed_slice(nestrs::queues::QUEUE_PROCESSORS)]
            static __NES_QUEUE_PROCESSOR: nestrs::queues::QueueProcessorRegistration =
                nestrs::queues::QueueProcessorRegistration {
                    queue: #queue,
                    create: nestrs::queues::handler_factory::<#name>,
                };
        };
    };

    expanded.into()
}

/// Wraps handler return values in `axum::Json(...)` (NestJS "return object => JSON" ergonomics).
///
/// Supports:
/// - `async fn handler() -> T` where `T: serde::Serialize`
/// - `async fn handler() -> Result<T, E>` where `T: serde::Serialize` and `E: IntoResponse`
/// - `#[serialize(strip_null)]` — drop JSON `null` values after serialization (Nest `@Exclude` / transformer-style cleanup).
#[proc_macro_attribute]
pub fn serialize(attr: TokenStream, item: TokenStream) -> TokenStream {
    let strip_null = if attr.is_empty() {
        false
    } else {
        let id = parse_macro_input!(attr as Ident);
        if id != "strip_null" {
            return syn::Error::new_spanned(
                id,
                "unknown serialize option (expected `strip_null` or empty)",
            )
            .to_compile_error()
            .into();
        }
        true
    };

    let mut method = parse_macro_input!(item as ImplItemFn);
    let output = method.sig.output.clone();
    let block = method.block;

    method.sig.output = syn::parse_quote!(-> axum::response::Response);

    let ok_json = if strip_null {
        quote! {
            match serde_json::to_value(&__ok) {
                Ok(__v) => {
                    let __v = nestrs::strip_null_json_value(__v);
                    axum::response::IntoResponse::into_response(axum::Json(__v))
                }
                Err(__e) => axum::response::IntoResponse::into_response(
                    nestrs::InternalServerErrorException::new(__e.to_string()),
                ),
            }
        }
    } else {
        quote! {
            axum::response::IntoResponse::into_response(axum::Json(__ok))
        }
    };

    let body = match output {
        syn::ReturnType::Default => {
            quote! {
                let __value: () = (async move #block).await;
                axum::response::IntoResponse::into_response(axum::Json(__value))
            }
        }
        syn::ReturnType::Type(_, ty) => {
            if let Some((_ok, _err)) = split_result_type(&ty) {
                quote! {
                    let __value: #ty = (async move #block).await;
                    match __value {
                        Ok(__ok) => { #ok_json }
                        Err(__err) => axum::response::IntoResponse::into_response(__err),
                    }
                }
            } else if strip_null {
                quote! {
                    let __value: #ty = (async move #block).await;
                    match serde_json::to_value(&__value) {
                        Ok(__v) => {
                            let __v = nestrs::strip_null_json_value(__v);
                            axum::response::IntoResponse::into_response(axum::Json(__v))
                        }
                        Err(__e) => axum::response::IntoResponse::into_response(
                            nestrs::InternalServerErrorException::new(__e.to_string()),
                        ),
                    }
                }
            } else {
                quote! {
                    let __value: #ty = (async move #block).await;
                    axum::response::IntoResponse::into_response(axum::Json(__value))
                }
            }
        }
    };

    method.block = syn::parse_quote!({ #body });

    quote!(#method).into()
}

#[proc_macro_attribute]
pub fn http_code(attr: TokenStream, item: TokenStream) -> TokenStream {
    let code = parse_macro_input!(attr as LitInt);
    let mut method = parse_macro_input!(item as ImplItemFn);
    let block = method.block;

    method.sig.output = syn::parse_quote!(-> axum::response::Response);
    method.block = syn::parse_quote!({
        let __value = (async move #block).await;
        let mut __response = axum::response::IntoResponse::into_response(__value);
        if let Ok(__status) = axum::http::StatusCode::from_u16(#code) {
            __response.status_mut().clone_from(&__status);
        }
        __response
    });

    quote!(#method).into()
}

struct HeaderArgs {
    name: LitStr,
    value: LitStr,
}

impl Parse for HeaderArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let name: LitStr = input.parse()?;
        input.parse::<Token![,]>()?;
        let value: LitStr = input.parse()?;
        Ok(Self { name, value })
    }
}

#[proc_macro_attribute]
pub fn response_header(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as HeaderArgs);
    let mut method = parse_macro_input!(item as ImplItemFn);
    let block = method.block;
    let name = args.name;
    let value = args.value;

    method.sig.output = syn::parse_quote!(-> axum::response::Response);
    method.block = syn::parse_quote!({
        let __value = (async move #block).await;
        let mut __response = axum::response::IntoResponse::into_response(__value);
        if let (Ok(__name), Ok(__value)) = (
            axum::http::header::HeaderName::from_bytes(#name.as_bytes()),
            axum::http::HeaderValue::from_str(#value),
        ) {
            __response.headers_mut().insert(__name, __value);
        }
        __response
    });

    quote!(#method).into()
}

struct RedirectArgs {
    url: LitStr,
    code: Option<LitInt>,
}

impl Parse for RedirectArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let url: LitStr = input.parse()?;
        let mut code = None;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            code = Some(input.parse()?);
        }
        Ok(Self { url, code })
    }
}

#[proc_macro_attribute]
pub fn redirect(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as RedirectArgs);
    let mut method = parse_macro_input!(item as ImplItemFn);
    let url = args.url;
    let maybe_code = args.code;

    method.sig.output = syn::parse_quote!(-> axum::response::Response);
    method.block = if let Some(code) = maybe_code {
        syn::parse_quote!({
            let mut __response = axum::response::Redirect::to(#url).into_response();
            if let Ok(__status) = axum::http::StatusCode::from_u16(#code) {
                __response.status_mut().clone_from(&__status);
            }
            __response
        })
    } else {
        syn::parse_quote!({
            axum::response::Redirect::to(#url).into_response()
        })
    };

    quote!(#method).into()
}

fn field_has_marker(field: &Field, marker: &str) -> bool {
    field.attrs.iter().any(|a| a.path().is_ident(marker))
}

fn convert_dto_field_attrs(field: &Field, expose_only: bool) -> Vec<syn::Attribute> {
    let mut out = Vec::new();

    for attr in &field.attrs {
        let Some(name) = attr.path().get_ident().map(|v| v.to_string()) else {
            out.push(attr.clone());
            continue;
        };

        match name.as_str() {
            "Exclude" => {
                out.push(syn::parse_quote!(#[serde(skip_serializing)]));
            }
            "Expose" => {}
            "IsEmail" => out.push(syn::parse_quote!(#[validate(email)])),
            "IsNotEmpty" => out.push(syn::parse_quote!(#[validate(length(min = 1))])),
            "IsString" => {
                // Type-level no-op in Rust, retained for Nest-like readability.
            }
            "IsUUID" => {
                // validator 0.21 dropped the `uuid` built-in; UUID-ness is a
                // type-level concern (use `uuid::Uuid`), so this is a
                // type-level no-op like `IsString`.
            }
            "IsBoolean" => {}
            "IsPositive" => out.push(syn::parse_quote!(#[validate(range(min = 1))])),
            "IsNegative" => out.push(syn::parse_quote!(#[validate(range(max = -1))])),
            "MinLength" => {
                if let Meta::List(list) = &attr.meta {
                    let tokens = list.tokens.clone();
                    out.push(syn::parse_quote!(#[validate(length(min = #tokens))]));
                }
            }
            "MaxLength" => {
                if let Meta::List(list) = &attr.meta {
                    let tokens = list.tokens.clone();
                    out.push(syn::parse_quote!(#[validate(length(max = #tokens))]));
                }
            }
            "Length" => {
                if let Meta::List(list) = &attr.meta {
                    let tokens = list.tokens.clone();
                    out.push(syn::parse_quote!(#[validate(length(#tokens))]));
                }
            }
            "Min" => {
                if let Meta::List(list) = &attr.meta {
                    let tokens = list.tokens.clone();
                    out.push(syn::parse_quote!(#[validate(range(min = #tokens))]));
                }
            }
            "Max" => {
                if let Meta::List(list) = &attr.meta {
                    let tokens = list.tokens.clone();
                    out.push(syn::parse_quote!(#[validate(range(max = #tokens))]));
                }
            }
            // Integer / number checks are expressed by Rust types (`i32`, `f64`, …) and `range` where needed.
            "IsInt" | "IsNumber" => {}
            "IsUrl" => out.push(syn::parse_quote!(#[validate(url)])),
            "Matches" => {
                if let Meta::List(list) = &attr.meta {
                    let tokens = list.tokens.clone();
                    out.push(syn::parse_quote!(#[validate(regex = #tokens)]));
                }
            }
            "Contains" => {
                if let Meta::List(list) = &attr.meta {
                    let tokens = list.tokens.clone();
                    out.push(syn::parse_quote!(#[validate(contains(#tokens))]));
                }
            }
            // Nest `@IsOptional()` maps to `Option<T>` in Rust; strip the marker attribute.
            "IsOptional" => {}
            "ValidateNested" => out.push(syn::parse_quote!(#[validate(nested)])),
            _ => out.push(attr.clone()),
        }
    }

    if expose_only && !field_has_marker(field, "Expose") {
        out.push(syn::parse_quote!(#[serde(skip_serializing)]));
    }

    out
}

#[derive(Default)]
struct DtoAttr {
    expose_only: bool,
    allow_unknown_fields: bool,
}

impl Parse for DtoAttr {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut out = DtoAttr::default();
        if input.is_empty() {
            return Ok(out);
        }

        let vars = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
        for id in vars {
            if id == "expose_only" {
                out.expose_only = true;
            } else if id == "allow_unknown_fields" {
                out.allow_unknown_fields = true;
            } else {
                return Err(syn::Error::new_spanned(
                    id,
                    "unknown dto option (expected `expose_only` or `allow_unknown_fields`)",
                ));
            }
        }
        Ok(out)
    }
}

/// `#[dataloader]` options: `key = <Type>` (required), `value = <Type>`
/// (required), `error = <Type>` (optional — defaults to
/// `nestrs::graphql::Error`).
struct DataLoaderAttr {
    key: syn::Type,
    value: syn::Type,
    error: Option<syn::Type>,
}

impl Parse for DataLoaderAttr {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut key = None;
        let mut value = None;
        let mut error = None;

        while !input.is_empty() {
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let ty: syn::Type = input.parse()?;
            match name.to_string().as_str() {
                "key" => key = Some(ty),
                "value" => value = Some(ty),
                "error" => error = Some(ty),
                _ => {
                    return Err(syn::Error::new_spanned(
                        name,
                        "unknown dataloader option (expected `key`, `value`, or `error`)",
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        let key = key.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "`#[dataloader]` requires `key = <Type>`",
            )
        })?;
        let value = value.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "`#[dataloader]` requires `value = <Type>`",
            )
        })?;
        Ok(DataLoaderAttr { key, value, error })
    }
}

/// Generate a `Loader` impl (plus a `into_data_loader` constructor) for
/// a batching-loader struct:
///
/// ```ignore
/// #[dataloader(key = i64, value = User, error = MyError)]
/// struct UserLoader { pool: Arc<AnyPool> }
///
/// impl UserLoader {
///     // exactly one SQL round-trip for every load_one/load_many in the
///     // request; the generated trait impl delegates here.
///     async fn batch_load(&self, keys: &[i64]) -> Result<HashMap<i64, User>, MyError> {
///         /* SELECT ... WHERE id IN (...) */
///     }
/// }
/// ```
///
/// The struct keeps its own definition; the macro appends the
/// `nestrs::graphql::Loader<Key>` impl and a `into_data_loader()`
/// constructor that wraps it in a fresh `DataLoader` (1 ms batch window,
/// batch tasks spawned on the current tokio runtime, no internal cache —
/// per-request instances make memoization redundant). Requires the
/// `nestrs` `graphql-dataloader` feature and the loader's inherent
/// `async fn batch_load(&self, keys: &[Key]) -> Result<HashMap<Key, Value>, Error>`.
#[proc_macro_attribute]
pub fn dataloader(attr: TokenStream, item: TokenStream) -> TokenStream {
    let opts = parse_macro_input!(attr as DataLoaderAttr);
    let item_struct = parse_macro_input!(item as ItemStruct);

    let vis = &item_struct.vis;
    let ident = &item_struct.ident;
    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();

    let key_ty = opts.key;
    let value_ty = opts.value;
    let error_ty = opts
        .error
        .unwrap_or_else(|| syn::parse_quote!(nestrs::graphql::Error));

    quote! {
        #item_struct

        impl #impl_generics nestrs::graphql::Loader<#key_ty> for #ident #ty_generics #where_clause {
            type Value = #value_ty;
            type Error = #error_ty;

            async fn load(
                &self,
                keys: &[#key_ty],
            ) -> Result<std::collections::HashMap<#key_ty, Self::Value>, Self::Error> {
                <Self>::batch_load(self, keys).await
            }
        }

        impl #impl_generics #ident #ty_generics #where_clause {
            /// Wrap this loader in a fresh per-request `DataLoader`.
            /// Call once per request (the `DataLoaderRegistry` factory
            /// closures generated into apps do exactly that): a
            /// `DataLoader` memoizes for its whole lifetime, so sharing
            /// one instance across requests would leak results.
            #vis fn into_data_loader(self) -> nestrs::graphql::DataLoader<Self> {
                nestrs::graphql::data_loader::data_loader(self)
            }
        }
    }
    .into()
}

// ---------------------------------------------------------------------------
// `#[nestrs::crud(entity = ..., output = ..., create = ..., update = ...,
//   [transport = "http"|"graphql"])]`
//
// Generates a 5-verb REST (or GraphQL) controller + service + hidden state
// provider from one attribute on a struct that holds an
// `Arc<sqlx::AnyPool>`. The five routes/operations are:
//   GET  /            list     (with page/sort/filter via `serde_qs`)
//   GET  /:id         read one
//   POST /            create
//   PATCH /:id        update
//   DELETE /:id       delete
//
// Each handler is wired through the existing `nestrs::impl_routes!` so
// guards / interceptors / filters / OpenAPI metadata all work unchanged.
// Row-level authz is enforced by the existing `CrudService<T>` (deny-closed
// when `authz-row-level` is on) — the macro adds zero new authz surface.
// ---------------------------------------------------------------------------

struct CrudArgs {
    entity: syn::Type,
    output: syn::Type,
    create: syn::Type,
    update: syn::Type,
    transport: Option<syn::LitStr>,
}

impl Parse for CrudArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut entity: Option<syn::Type> = None;
        let mut output: Option<syn::Type> = None;
        let mut create: Option<syn::Type> = None;
        let mut update: Option<syn::Type> = None;
        let mut transport: Option<syn::LitStr> = None;

        while !input.is_empty() {
            let name: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            match name.to_string().as_str() {
                "entity" => {
                    entity = Some(input.parse::<syn::Type>()?);
                }
                "output" => {
                    output = Some(input.parse::<syn::Type>()?);
                }
                "create" => {
                    create = Some(input.parse::<syn::Type>()?);
                }
                "update" => {
                    update = Some(input.parse::<syn::Type>()?);
                }
                "transport" => {
                    transport = Some(input.parse::<syn::LitStr>()?);
                }
                other => {
                    return Err(syn::Error::new_spanned(
                        &name,
                        format!(
                            "unknown #[crud] option `{other}`; expected `entity`, `output`, \
                             `create`, `update`, or `transport`"
                        ),
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }

        let entity = entity.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "`#[crud]` requires `entity = <Type>`",
            )
        })?;
        let output = output.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "`#[crud]` requires `output = <Type>`",
            )
        })?;
        let create = create.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "`#[crud]` requires `create = <Type>`",
            )
        })?;
        let update = update.ok_or_else(|| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                "`#[crud]` requires `update = <Type>`",
            )
        })?;
        if let Some(t) = &transport {
            let v = t.value();
            if v != "http" && v != "graphql" {
                return Err(syn::Error::new_spanned(
                    t,
                    "#[crud] `transport` must be `\"http\"` or `\"graphql\"`",
                ));
            }
        }

        Ok(CrudArgs {
            entity,
            output,
            create,
            update,
            transport,
        })
    }
}

#[proc_macro_attribute]
pub fn crud(attr: TokenStream, item: TokenStream) -> TokenStream {
    let opts = parse_macro_input!(attr as CrudArgs);
    let mut item_struct = parse_macro_input!(item as ItemStruct);

    // Reject any user-supplied `#[routes(...)]` — the macro generates the
    // routes itself, and a double registration would clash.
    let mut had_user_routes = false;
    item_struct.attrs.retain(|a| {
        if a.path().is_ident("routes") {
            had_user_routes = true;
            false
        } else {
            true
        }
    });
    if had_user_routes {
        return syn::Error::new_spanned(
            &item_struct,
            "#[crud] is incompatible with `#[routes(...)]`; the macro generates the routes itself",
        )
        .to_compile_error()
        .into();
    }

    // The user's struct must have exactly one `Arc<sqlx::AnyPool>` field —
    // Validate the user supplied a named `pool: Arc<sqlx::AnyPool>` field —
    // the macro plumbs it into a hidden `#[injectable]` state struct.
    if let Err(e) = find_pool_field(&item_struct) {
        return e.to_compile_error().into();
    }

    let controller_ident = &item_struct.ident;
    let entity = &opts.entity;
    let output = &opts.output;
    let create = &opts.create;
    let update = &opts.update;
    let transport = opts
        .transport
        .as_ref()
        .map(|l| l.value())
        .unwrap_or_else(|| "http".to_string());

    // `{Pascal}Service`, `{Pascal}ListQuery` — derived from the controller's
    // ident. `{Pascal}` for `PostController` is `Post`.
    let pascal = pascal_from_controller(&item_struct.ident.to_string());
    let service_ident = format_ident!("{}Service", pascal);
    let list_query_ident = format_ident!("{}ListQuery", pascal);
    let state_ident = format_ident!("__{}CrudState", pascal);
    let filter_ident = format_ident!("{}Filter", pascal);
    let check_policies_subject = pascal.clone();
    let sdl_const_ident = format_ident!("{}_CONTROLLER_SDL", pascal.to_uppercase());

    // Field declaration for the `{Pascal}ListQuery` DTO the macro generates
    // alongside the user-supplied DTOs. Mirrors `@nestjsx/crud`'s
    // contract: `?page`, `?per_page`, `?sort=field:DIR`, `?filter[field]=x`,
    // `?search=...`. Defaults: page=1, per_page=20, no sort, no filter.
    //
    // The `ListQuery` struct is hand-shaped rather than built from
    // `convert_dto_field_attrs` so the macro doesn't accidentally try to
    // apply NestJS-style field decorators (`#[IsEmail]`, etc.) at this
    // layer; this DTO is internal to the framework.
    let list_query_def = quote! {
        #[derive(::std::fmt::Debug, ::std::clone::Clone,
                 ::serde::Serialize, ::serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        #[allow(non_camel_case_types)]
        pub struct #list_query_ident {
            #[serde(default = "nestrs::crud_macro::default_page")]
            pub page: u32,
            #[serde(default = "nestrs::crud_macro::default_per_page")]
            pub per_page: u32,
            #[serde(default)]
            pub sort: ::std::option::Option<::std::string::String>,
            #[serde(default)]
            pub filter: ::std::option::Option<#filter_ident>,
            #[serde(default)]
            pub search: ::std::option::Option<::std::string::String>,
        }

        impl ::nestrs::NestDto for #list_query_ident {}

        impl ::validator::Validate for #list_query_ident {
            fn validate(&self) -> ::std::result::Result<(), ::validator::ValidationErrors> {
                let mut errs = ::validator::ValidationErrors::new();
                if self.page < 1 {
                    errs.add("page", ::validator::ValidationError::new("range_min"));
                }
                if self.per_page < 1 || self.per_page > 1000 {
                    errs.add("per_page", ::validator::ValidationError::new("range"));
                }
                if let Some(sort) = &self.sort {
                    if let Err(e) = ::nestrs::crud_macro::validate_sort_string(sort) {
                        errs.add("sort", e);
                    }
                }
                if errs.is_empty() {
                    ::std::result::Result::Ok(())
                } else {
                    ::std::result::Result::Err(errs)
                }
            }
        }

        // Filter struct: holds bracketed filter params as string->string.
        // We store as strings and parse to JSON values at filter-application time.
        // This avoids `validator` trying to recurse into `serde_json::Value`.
        #[derive(::std::fmt::Debug, ::std::clone::Clone,
                 ::serde::Serialize, ::serde::Deserialize,
                 ::std::default::Default)]
        #[allow(non_camel_case_types)]
        pub struct #filter_ident {
            #[serde(default, flatten)]
            pub __rest: ::std::collections::BTreeMap<
                ::std::string::String,
                ::std::string::String,
            >,
        }

        impl ::validator::Validate for #filter_ident {
            fn validate(&self) -> ::std::result::Result<(), ::validator::ValidationErrors> {
                ::std::result::Result::Ok(())
            }
        }
    };

    // Helper functions for the list path: parse `sort=field:DIR` strings
    // into (field, dir) pairs, apply sort + page slice to an in-memory
    // vector, and (when authz is on) honour the authz filter that's
    // already pushed by `CrudService::list`.
    //
    // Lives on the `{Service}` so it's easy to unit-test from integration
    // code without going through Axum.
    let service_def = quote! {
        pub struct #service_ident {
            crud: ::nestrs::CrudService<#entity>,
            _out: ::std::marker::PhantomData<#output>,
        }

        impl #service_ident {
            /// Build a service from the shared SQL pool. Called once per
            /// request from inside the generated handler (the service is
            /// stateless — `CrudService` just wraps the pool — so a
            /// per-request build is essentially free).
            pub fn new(pool: ::std::sync::Arc<::sqlx::AnyPool>) -> Self {
                Self {
                    crud: ::nestrs::CrudService::new(pool),
                    _out: ::std::marker::PhantomData,
                }
            }

            /// The list endpoint. Reads the authz-filtered set (which
            /// already excludes rows the request-scoped `Ability` denies),
            /// then applies in-memory `sort` + `page` + `per_page` +
            /// `filter` + `search`. SQL pushdown + authz in one query is
            /// a v2 problem — the contract here matches
            /// `@nestjsx/crud` exactly.
            pub async fn list_query(
                &self,
                q: &#list_query_ident,
            ) -> ::std::result::Result<
                ::std::vec::Vec<#output>,
                ::nestrs::crud_macro::CrudError,
            > {
                let mut rows = self
                    .crud
                    .list()
                    .await
                    .map_err(::nestrs::crud_macro::CrudError::from)?;

                // `sort`: comma-separated `field:DIR` pairs. The first
                // recognised field wins (we sort stably per key, so
                // multiple keys would just need a stable-sort extension).
                if let Some(sort) = &q.sort {
                    for token in sort.split(',') {
                        let mut parts = token.split(':');
                        let key = parts.next().unwrap_or("").trim();
                        let dir = parts.next().unwrap_or("ASC").trim();
                        rows.sort_by(|a, b| {
                            let av = sort_value(a, key);
                            let bv = sort_value(b, key);
                            let ord = ::nestrs::crud_macro::sort_value_cmp(&av, &bv);
                            if dir.eq_ignore_ascii_case("DESC") {
                                ord.reverse()
                            } else {
                                ord
                            }
                        });
                    }
                }

                // `filter` + `search`: case-insensitive substring match
                // against any string field on the row's JSON
                // representation. Best-effort; the typed DTO shape is
                // outside the macro's reach (it'd require parsing the
                // user's `Create` DTO and matching fields by name).
                //
                // The filter values are stored as plain strings
                // (see the `__rest` field declaration above) — at match
                // time we coerce each filter value to `serde_json::Value`
                // via `serde_json::from_str` so `?filter[count]=1` and
                // `?filter[done]=true` still hit typed JSON values when
                // the user supplies parseable input.
                if let Some(filter) = &q.filter {
                    let needle: ::std::vec::Vec<(::std::string::String, ::serde_json::Value)> =
                        filter
                            .__rest
                            .iter()
                            .filter_map(|(k, v)| {
                                let parsed = ::serde_json::from_str::<::serde_json::Value>(v)
                                    .unwrap_or_else(|_| ::serde_json::Value::String(v.clone()));
                                Some((k.to_lowercase(), parsed))
                            })
                            .collect();
                    rows.retain(|row| {
                        let v = ::serde_json::to_value(row).unwrap_or(::serde_json::Value::Null);
                        needle.iter().all(|(k, want)| {
                            v.get(k)
                                .map(|got| ::nestrs::crud_macro::json_matches(got, want))
                                .unwrap_or(false)
                        })
                    });
                }
                if let Some(needle) = &q.search.as_ref().map(|s| s.to_lowercase()) {
                    rows.retain(|row| {
                        let v = ::serde_json::to_value(row).unwrap_or(::serde_json::Value::Null);
                        ::nestrs::crud_macro::json_contains_string(&v, needle)
                    });
                }

                // `page`/`per_page` slice. `per_page` is `u32` (validated
                // >= 1, <= 1000 above), so the cast is safe.
                let per_page = q.per_page as usize;
                let page = q.page.saturating_sub(1) as usize;
                let start = page.saturating_mul(per_page);
                let end = (start + per_page).min(rows.len());
                let slice: &[#entity] = if start >= rows.len() { &rows[0..0] } else { &rows[start..end] };

                let mut out: ::std::vec::Vec<#output> = ::std::vec::Vec::with_capacity(slice.len());
                for row in slice {
                    let v: ::serde_json::Value =
                        ::serde_json::to_value(row).unwrap_or(::serde_json::Value::Null);
                    let dto: #output = ::serde_json::from_value(v)
                        .map_err(::nestrs::crud_macro::CrudError::from)?;
                    out.push(dto);
                }
                ::std::result::Result::Ok(out)
            }

            /// Read one row by id, returning the output DTO. `None` if
            /// the row is missing **or** the request-scoped `Ability`
            /// denies the read.
            pub async fn get_one(
                &self,
                id: i64,
            ) -> ::std::result::Result<
                ::std::option::Option<#output>,
                ::nestrs::crud_macro::CrudError,
            > {
                let row = self
                    .crud
                    .read(id)
                    .await
                    .map_err(::nestrs::crud_macro::CrudError::from)?;
                match row {
                    Some(t) => ::serde_json::from_value(
                        ::serde_json::to_value(&t).unwrap_or(::serde_json::Value::Null),
                    )
                    .map(Some)
                    .map_err(::nestrs::crud_macro::CrudError::from),
                    None => ::std::result::Result::Ok(::std::option::Option::None),
                }
            }

            /// Insert a new row from the `Create` DTO. The DTO is
            /// round-tripped through `serde_json` to flatten the typed
            /// payload into the JSON-blob shape `CrudService::create`
            /// expects.
            pub async fn create_one(
                &self,
                body: #create,
            ) -> ::std::result::Result<#output, ::nestrs::crud_macro::CrudError> {
                let value = ::serde_json::to_value(&body)
                    .map_err(::nestrs::crud_macro::CrudError::from)?;
                let row = self
                    .crud
                    .create(value)
                    .await
                    .map_err(::nestrs::crud_macro::CrudError::from)?;
                ::serde_json::from_value(::serde_json::to_value(&row).unwrap_or(::serde_json::Value::Null))
                    .map_err(::nestrs::crud_macro::CrudError::from)
            }

            /// Patch a row by id from the `Update` DTO. Returns `None`
            /// if the id is missing or authz denies the write.
            /// Merges the partial update DTO with the existing entity
            /// so only the provided fields are changed.
            pub async fn update_one(
                &self,
                id: i64,
                body: #update,
            ) -> ::std::result::Result<
                ::std::option::Option<#output>,
                ::nestrs::crud_macro::CrudError,
            > {
                // Fetch the existing entity first
                let current = self
                    .crud
                    .read(id)
                    .await
                    .map_err(::nestrs::crud_macro::CrudError::from)?;
                let Some(current) = current else {
                    return ::std::result::Result::Ok(::std::option::Option::None);
                };
                // Merge: convert current to JSON, then apply the update DTO fields
                let mut current_json = ::serde_json::to_value(&current)
                    .map_err(::nestrs::crud_macro::CrudError::from)?;
                let update_json = ::serde_json::to_value(&body)
                    .map_err(::nestrs::crud_macro::CrudError::from)?;
                if let (serde_json::Value::Object(cur), serde_json::Value::Object(upd)) = (&mut current_json, update_json) {
                    for (k, v) in upd {
                        cur.insert(k, v);
                    }
                }
                // Now pass the merged JSON to CrudService::update
                let row = self
                    .crud
                    .update(id, current_json)
                    .await
                    .map_err(::nestrs::crud_macro::CrudError::from)?;
                match row {
                    Some(t) => ::serde_json::from_value(
                        ::serde_json::to_value(&t).unwrap_or(::serde_json::Value::Null),
                    )
                    .map(Some)
                    .map_err(::nestrs::crud_macro::CrudError::from),
                    None => ::std::result::Result::Ok(::std::option::Option::None),
                }
            }

            /// Delete a row by id. Returns `true` if a row was
            /// removed, `false` if the id was missing or authz denied
            /// the delete. Deny-closed: under `authz-row-level`, the
            /// ability must explicitly allow `Delete` on the subject.
            pub async fn delete_one(
                &self,
                id: i64,
            ) -> ::std::result::Result<bool, ::nestrs::crud_macro::CrudError> {
                self.crud
                    .delete(id)
                    .await
                    .map_err(::nestrs::crud_macro::CrudError::from)
            }
        }
    };

    // The hidden state struct that the user's `pool` field plumbs into.
    // It exists so the generated `impl_routes!` call has a concrete
    // `state = ...` type to thread through every request, exactly like a
    // hand-written controller's `AppState`. The state carries the
    // `Arc<AnyPool>` directly — handlers construct a `{Service}` from it
    // on every request (cheap; the service is just a thin wrapper).
    //
    // The pool is wrapped in `Option` so the struct can be `Default`-built
    // when the user doesn't provide a real one — handlers then return
    // a clear 500 if the default state is hit (pool is `None`).
    let state_def = quote! {
        #[derive(::std::default::Default, ::std::clone::Clone)]
        pub struct #state_ident {
            pub pool: ::std::option::Option<::std::sync::Arc<::sqlx::AnyPool>>,
        }

        impl ::nestrs::core::Injectable for #state_ident {
            fn construct(_registry: &::nestrs::core::ProviderRegistry) -> ::std::sync::Arc<Self> {
                // The macro's caller is expected to register
                // `__{Pascal}CrudState` with the actual pool via
                // `registry.override_provider::<__{Pascal}CrudState>(Arc::new(
                //     __{Pascal}CrudState::from_pool(pool)))`
                // — at macro time we don't have that pool, so the
                // default constructor yields an empty state and the
                // module-level wiring replaces it.
                ::std::sync::Arc::new(Self::default())
            }
        }

        impl #state_ident {
            /// Build the state from a pool — the call site the user's
            /// module setup uses when overriding the default.
            pub fn from_pool(pool: ::std::sync::Arc<::sqlx::AnyPool>) -> Self {
                Self { pool: ::std::option::Option::Some(pool) }
            }

            /// Hand the pool back to a handler, or 500 if the state was
            /// built via the default constructor (no pool injected).
            fn pool(&self) -> ::std::result::Result<::std::sync::Arc<::sqlx::AnyPool>, ::nestrs::HttpException> {
                self.pool.clone().ok_or_else(|| {
                    ::nestrs::InternalServerErrorException::new(
                        "CRUD state not initialised — call registry.override_provider::<__{Pascal}CrudState>(...)".to_string(),
                    )
                })
            }
        }
    };

    // The 5 generated handlers. Each is one line of business logic that
    // calls the `{Service}` method and translates its `Result<_, CrudError>`
    // into the right HTTP status:
    //   - `CrudError::NotFound` (from `get_one` / `update_one`) → 404
    //   - `CrudError::Sqlx(sqlx::Error)` → 500 (exception filter)
    //   - `CrudError::Json(_)` (from DTO round-trip) → 500 (exception filter)
    //
    // The `Json(...)` wrap is `nestrs::axum::Json`, which serializes via
    // `serde_json`. `ValidationPipe` is auto-applied on the body / query
    // routes so 422s on invalid input come out of the box.
    //
    // Why `nestrs::axum::...` instead of `axum::...`? The dev-dep tree for
    // `nestrs` (and any downstream test crate that pulls in async-graphql)
    // can resolve two axum versions — the workspace-pinned 0.7 (via the
    // `nestrs` direct dep) and 0.8 (via async-graphql 7). An absolute
    // `::axum::...` path in generated code is then ambiguous between the
    // two and the generated handler fails to satisfy the wrong version's
    // `Handler` trait. Routing through `nestrs::axum` (re-exported at
    // `nestrs/src/lib.rs:5`) anchors resolution to the same axum version
    // `nestrs` itself was compiled against.
    let handler_def = quote! {
        impl #controller_ident {
            // `#[nestrs::axum::debug_handler]` is intentionally omitted.
            // The expanded `__axum_macros_check_*_from_request_check`
            // shim it generates references `::axum::extract::FromRequestParts`
            // by absolute path. In a crate whose dep graph contains two
            // axum versions (e.g. via async-graphql 7 pulling axum 0.8),
            // the shim's path resolves to the wrong version and the
            // `const _: () = { ... }` block that calls it surfaces as
            // "cannot find function `__axum_macros_check_*`". Omitting
            // the attribute trades the better compile-error message for
            // an actual working build. The handlers themselves still
            // type-check correctly because axum's `Handler` trait impls
            // resolve through `nestrs::axum::...`.
            async fn list(
                ::nestrs::axum::extract::State(state): ::nestrs::axum::extract::State<::std::sync::Arc<#state_ident>>,
                q: ::nestrs::crud_macro::__CrudQueryAdapter<#list_query_ident>,
            ) -> ::std::result::Result<
                ::nestrs::axum::Json<::std::vec::Vec<#output>>,
                ::nestrs::HttpException,
            > {
                let pool = state.pool()?;
                let svc = #service_ident::new(pool);
                let rows = svc.list_query(&q.0).await.map_err(
                    ::nestrs::crud_macro::map_crud_error_to_http,
                )?;
                ::std::result::Result::Ok(::nestrs::axum::Json(rows))
            }

            async fn get_one(
                ::nestrs::axum::extract::State(state): ::nestrs::axum::extract::State<::std::sync::Arc<#state_ident>>,
                ::nestrs::axum::extract::Path(id): ::nestrs::axum::extract::Path<i64>,
            ) -> ::std::result::Result<
                ::nestrs::axum::Json<#output>,
                ::nestrs::HttpException,
            > {
                let pool = state.pool()?;
                let svc = #service_ident::new(pool);
                match svc.get_one(id).await.map_err(
                    ::nestrs::crud_macro::map_crud_error_to_http,
                )? {
                    Some(dto) => ::std::result::Result::Ok(::nestrs::axum::Json(dto)),
                    None => ::std::result::Result::Err(
                        ::nestrs::NotFoundException::new("not found")
                    ),
                }
            }

            async fn create_one(
                ::nestrs::axum::extract::State(state): ::nestrs::axum::extract::State<::std::sync::Arc<#state_ident>>,
                ::nestrs::axum::extract::Json(body): ::nestrs::axum::extract::Json<#create>,
            ) -> ::std::result::Result<
                (::nestrs::axum::http::StatusCode, ::nestrs::axum::Json<#output>),
                ::nestrs::HttpException,
            > {
                let pool = state.pool()?;
                let svc = #service_ident::new(pool);
                let dto = svc.create_one(body).await.map_err(
                    ::nestrs::crud_macro::map_crud_error_to_http,
                )?;
                ::std::result::Result::Ok((
                    ::nestrs::axum::http::StatusCode::CREATED,
                    ::nestrs::axum::Json(dto),
                ))
            }

            async fn update_one(
                ::nestrs::axum::extract::State(state): ::nestrs::axum::extract::State<::std::sync::Arc<#state_ident>>,
                ::nestrs::axum::extract::Path(id): ::nestrs::axum::extract::Path<i64>,
                ::nestrs::axum::extract::Json(body): ::nestrs::axum::extract::Json<#update>,
            ) -> ::std::result::Result<
                ::nestrs::axum::Json<#output>,
                ::nestrs::HttpException,
            > {
                let pool = state.pool()?;
                let svc = #service_ident::new(pool);
                match svc.update_one(id, body).await.map_err(
                    ::nestrs::crud_macro::map_crud_error_to_http,
                )? {
                    Some(dto) => ::std::result::Result::Ok(::nestrs::axum::Json(dto)),
                    None => ::std::result::Result::Err(
                        ::nestrs::NotFoundException::new("not found")
                    ),
                }
            }

            async fn delete_one(
                ::nestrs::axum::extract::State(state): ::nestrs::axum::extract::State<::std::sync::Arc<#state_ident>>,
                ::nestrs::axum::extract::Path(id): ::nestrs::axum::extract::Path<i64>,
            ) -> ::std::result::Result<
                ::nestrs::axum::http::StatusCode,
                ::nestrs::HttpException,
            > {
                let pool = state.pool()?;
                let svc = #service_ident::new(pool);
                let removed = svc.delete_one(id).await.map_err(
                    ::nestrs::crud_macro::map_crud_error_to_http,
                )?;
                if removed {
                    ::std::result::Result::Ok(::nestrs::axum::http::StatusCode::NO_CONTENT)
                } else {
                    ::std::result::Result::Err(
                        ::nestrs::NotFoundException::new("not found")
                    )
                }
            }
        }
    };

    // The `impl_routes!` invocation. Five entries, one per HTTP verb.
    // `metadata (("check_policies", "read:Post"))` etc. wire the right
    // action:subject tokens so a `PoliciesGuard` on top sees them.
    // The metadata values must be `literal`s (the `impl_routes!`
    // macro arm requires `$meta_value:literal`), so we precompute the
    // action:subject strings here and pass them as plain string
    // literals. `openapi`, `interceptors`, `filters` are all optional
    // in `impl_routes!` — we omit them.
    let read_token = format!("read:{check_policies_subject}");
    let create_token = format!("create:{check_policies_subject}");
    let update_token = format!("update:{check_policies_subject}");
    let delete_token = format!("delete:{check_policies_subject}");
    let route_entries = quote! {
        GET    "/"      with ()
            metadata (("check_policies", #read_token))
            => #controller_ident::list,
        GET    "/:id"   with ()
            metadata (("check_policies", #read_token))
            => #controller_ident::get_one,
        POST   "/"      with ()
            metadata (("check_policies", #create_token))
            => #controller_ident::create_one,
        PATCH  "/:id"   with ()
            metadata (("check_policies", #update_token))
            => #controller_ident::update_one,
        DELETE "/:id"   with ()
            metadata (("check_policies", #delete_token))
            => #controller_ident::delete_one,
    };
    let routes_call = quote! {
        nestrs::impl_routes!(#controller_ident, state #state_ident => [
            #route_entries
        ]);
    };

    // Compile-time assertions. `T: Entity`, `T: Serialize + DeserializeOwned`,
    // and the three DTOs implement `NestDto`. The `Entity`/`Serialize`/
    // `DeserializeOwned` assertions fail with a clear message at
    // expansion time; the `NestDto` ones fail when the user forgot
    // `#[dto]` on their output/create/update DTOs.
    let assertions = quote! {
        const _: () = {
            const fn __nestrs_assert_entity<T: ::nestrs::Entity>() {}
            __nestrs_assert_entity::<#entity>();
            const fn __nestrs_assert_serde<T: ::serde::Serialize + ::serde::de::DeserializeOwned>() {}
            __nestrs_assert_serde::<#entity>();
            __nestrs_assert_serde::<#output>();
            __nestrs_assert_serde::<#create>();
            __nestrs_assert_serde::<#update>();
            const fn __nestrs_assert_dto<T: ::nestrs::NestDto>() {}
            __nestrs_assert_dto::<#output>();
            __nestrs_assert_dto::<#create>();
            __nestrs_assert_dto::<#update>();
        };
    };

    // The `sort_value` helper extracts a JSON field from a row for sorting.
    // Lives at module scope so the closure inside `list_query` can call it.
    let sort_helper = quote! {
        #[doc(hidden)]
        fn sort_value<T: ::serde::Serialize>(row: &T, key: &str) -> ::serde_json::Value {
            let v = ::serde_json::to_value(row).unwrap_or(::serde_json::Value::Null);
            v.get(key).cloned().unwrap_or(::serde_json::Value::Null)
        }
    };

    // For `transport = "graphql"`: emit a `pub const {PASCAL}_CONTROLLER_SDL`
    // string the user can hand-merge into their schema (async-graphql has
    // no first-class runtime SDL merge primitive). The typed
    // `#[async_graphql::Object]` resolvers are out of scope for v1 — see
    // `docs/src/backend-recipes.md` for the hand-rolled pattern that
    // wraps the same `{Service}` and is the recommended migration path.
    let graphql_block = if transport == "graphql" {
        let sdl_const = format!(
            "# Generated by #[nestrs::crud] for {pascal}.\n\
             # Hand-merge this into your `async_graphql::Schema` via\n\
             # `Schema::build(..., {pascal}_CONTROLLER_SDL)`.\n\
             scalar JSON\n\
             type {pascal} {{ id: Int!, data: JSON }}\n\
             input {pascal}FilterInput {{\n  page: Int\n  perPage: Int\n  sort: String\n  search: String\n  filter: JSON\n}}\n\
             input {pascal}CreateInput {{ data: JSON! }}\n\
             input {pascal}UpdateInput {{ data: JSON! }}\n\
             type Query {{\n  {snake}_one(id: Int!): {pascal}\n  {snake}_list(filter: {pascal}FilterInput): [{pascal}!]!\n}}\n\
             type Mutation {{\n  {snake}_create(input: {pascal}CreateInput!): {pascal}!\n  {snake}_update(id: Int!, input: {pascal}UpdateInput!): {pascal}!\n  {snake}_delete(id: Int!): Boolean!\n}}\n",
            pascal = pascal,
            snake = pascal_to_snake(&pascal),
        );
        quote! {
            #[allow(non_upper_case_globals)]
            pub const #sdl_const_ident: &'static str = #sdl_const;
        }
    } else {
        quote! {}
    };

    // Final expansion: re-emit the user's struct unchanged (so the
    // `pool` field stays in scope for any `PostController { pool: ... }`
    // literal in user code), plus the service, the state, the handlers,
    // the `impl_routes!` call, and the compile-time assertions. The
    // `pool` field is *not* stripped — it's the user's field; the
    // macro just doesn't generate any code that uses it.
    let expanded = quote! {
        #item_struct
        #service_def
        #list_query_def
        #state_def
        #handler_def
        #routes_call
        #sort_helper
        #assertions
        #graphql_block
    };

    let s = expanded.to_string();
    eprintln!("=== EXPANDED CRUD ===\n{}\n=== END ===", s);
    expanded.into()
}

/// Inspect the user's struct fields and return the index of the single
/// `Arc<sqlx::AnyPool>` field. Rejects anything else (zero matches, or
/// multiple) with a clear compile error.
fn find_pool_field(item_struct: &ItemStruct) -> std::result::Result<usize, syn::Error> {
    let named = match &item_struct.fields {
        Fields::Named(named) => named,
        _ => {
            return Err(syn::Error::new_spanned(
                &item_struct.ident,
                "#[crud] requires a named-field struct (e.g. `struct FooController { pool: Arc<AnyPool> }`)",
            ));
        }
    };
    let pool_indices: Vec<usize> = named
        .named
        .iter()
        .enumerate()
        .filter(|(_, f)| is_pool_field(f))
        .map(|(i, _)| i)
        .collect();
    match pool_indices.len() {
        1 => Ok(pool_indices[0]),
        0 => Err(syn::Error::new_spanned(
            &item_struct.ident,
            "#[crud] requires the controller struct to have exactly one field of type `Arc<sqlx::AnyPool>` \
             (e.g. `pool: Arc<sqlx::AnyPool>`); declare additional dependencies as separate `#[injectable]` providers",
        )),
        _ => Err(syn::Error::new_spanned(
            &item_struct.ident,
            "#[crud] controllers must have exactly one `Arc<sqlx::AnyPool>` field; \
             declare additional dependencies as separate `#[injectable]` providers",
        )),
    }
}

fn is_pool_field(field: &Field) -> bool {
    let ty = &field.ty;
    // `Arc<sqlx::AnyPool>` desugars to `::std::sync::Arc<sqlx::AnyPool>`;
    // accept either spelling.
    let s = quote!(#ty).to_string();
    s.contains("AnyPool") && (s.contains("Arc") || s.contains("sync::Arc"))
}

fn pascal_from_controller(name: &str) -> String {
    // `PostController` → `Post`. We strip the trailing `Controller` if
    // present, otherwise use the ident as-is.
    if let Some(stripped) = name.strip_suffix("Controller") {
        stripped.to_string()
    } else {
        name.to_string()
    }
}

fn pascal_to_snake(pascal: &str) -> String {
    let mut out = String::new();
    for (i, ch) in pascal.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

#[proc_macro_attribute]
pub fn dto(attr: TokenStream, item: TokenStream) -> TokenStream {
    let opts = parse_macro_input!(attr as DtoAttr);
    let expose_only = opts.expose_only;
    let deny_unknown_fields = !opts.allow_unknown_fields;
    let item_struct = parse_macro_input!(item as ItemStruct);
    let vis = item_struct.vis;
    let ident = item_struct.ident;

    let fields = match item_struct.fields {
        Fields::Named(named) => named,
        _ => {
            return syn::Error::new_spanned(
                ident,
                "dto currently supports named-field structs only",
            )
            .to_compile_error()
            .into()
        }
    };

    let field_defs: Vec<_> = fields
        .named
        .iter()
        .map(|field| {
            let attrs = convert_dto_field_attrs(field, expose_only);
            let field_ident = field.ident.clone();
            let ty = field.ty.clone();
            quote! {
                #(#attrs)*
                pub #field_ident: #ty
            }
        })
        .collect();

    if deny_unknown_fields {
        quote! {
            #[derive(Debug, Clone, serde::Deserialize, serde::Serialize, nestrs::NestDto, nestrs::schemars::JsonSchema, validator::Validate)]
            #[serde(deny_unknown_fields)]
            #vis struct #ident {
                #(#field_defs,)*
            }
        }
        .into()
    } else {
        quote! {
            #[derive(Debug, Clone, serde::Deserialize, serde::Serialize, nestrs::NestDto, nestrs::schemars::JsonSchema, validator::Validate)]
            #vis struct #ident {
                #(#field_defs,)*
            }
        }
        .into()
    }
}

#[proc_macro_derive(
    NestDto,
    attributes(
        IsString,
        IsEmail,
        IsNotEmpty,
        MinLength,
        MaxLength,
        Length,
        Min,
        Max,
        IsInt,
        IsNumber,
        IsUrl,
        IsOptional,
        ValidateNested,
        Exclude,
        Expose,
        IsUUID,
        IsBoolean,
        IsPositive,
        IsNegative,
        Matches,
        Contains
    )
)]
pub fn derive_nest_dto(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let ident = input.ident;

    let expanded = quote! {
        impl nestrs::NestDto for #ident {}
    };

    expanded.into()
}

#[proc_macro_derive(NestConfig)]
pub fn derive_nest_config(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let ident = input.ident;

    let expanded = quote! {
        impl nestrs::NestConfig for #ident {}

        impl nestrs::core::Injectable for #ident {
            fn construct(_registry: &nestrs::core::ProviderRegistry) -> std::sync::Arc<Self> {
                let cfg = nestrs::load_config::<Self>().unwrap_or_else(|e| {
                    panic!("config load failed for `{}`: {e}", std::any::type_name::<Self>())
                });
                std::sync::Arc::new(cfg)
            }
        }
    };

    expanded.into()
}

// ---------------------------------------------------------------------------
// `#[config(namespace = "db")]`
//
// Struct-level attribute decorator for the namespaced config system
// (`NESTRS_<NS>__<KEY>`). Emits the two required derives plus the
// `nestrs::ConfigNamespace` marker impl:
//
//     #[config(namespace = "db")]
//     #[derive(serde::Deserialize, validator::Validate)]   // emitted by the macro
//     struct DatabaseConfig { host: String }
//
// so the struct itself only needs the attribute. The attribute must come
// *before* any other derives the user writes (attribute macros run before
// derives regardless of order, and the emitted derives are additive).
// ---------------------------------------------------------------------------

#[proc_macro_attribute]
pub fn config(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr2 = proc_macro2::TokenStream::from(attr.clone());
    let meta = match syn::parse2::<syn::Meta>(attr2.clone()) {
        Ok(m) => m,
        Err(_) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[config] expects `namespace = \"<name>\"`",
            )
            .to_compile_error()
            .into();
        }
    };

    let namespace = match &meta {
        syn::Meta::NameValue(nv) if nv.path.is_ident("namespace") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => s.value(),
            _ => {
                return syn::Error::new_spanned(&nv.value, "namespace must be a string literal")
                    .to_compile_error()
                    .into()
            }
        },
        _ => {
            return syn::Error::new_spanned(meta, "#[config] expects `namespace = \"<name>\"`")
                .to_compile_error()
                .into()
        }
    };

    let item2: proc_macro2::TokenStream = item.into();
    let ident = match syn::parse2::<syn::DeriveInput>(item2.clone()) {
        Ok(d) => d.ident,
        Err(_) => {
            return syn::Error::new(
                proc_macro2::Span::call_site(),
                "#[config] can only be applied to a struct or enum",
            )
            .to_compile_error()
            .into()
        }
    };

    let expanded = quote! {
        #[derive(::serde::Deserialize, ::validator::Validate)]
        #item2

        impl nestrs::ConfigNamespace for #ident {
            const NAMESPACE: &'static str = #namespace;
        }
    };

    expanded.into()
}

// ---------------------------------------------------------------------------
// `#[upload_to("bucket", "prefix/{user_id}/file-{file_id}")]`
//
// Function-level decorator. Marks a route handler as an upload
// endpoint and stashes the resolved bucket + key template onto
// the function via two `pub fn` accessors that mirror the
// `#[controller]` shape:
//
//     pub fn __upload_to_bucket() -> &'static str { "bucket" }
//     pub fn __upload_to_key_template() -> &'static str { "prefix/{user_id}/..." }
//     pub fn __upload_to_params() -> &'static [&'static str] { &["user_id", "file_id"] }
//
// The runtime helper `nestrs_storage::resolve_upload_key` walks
// the template, substituting the placeholders in order, and the
// upload handler calls `upload_to(storage, &key, body)` once the
// substitution is done. This keeps the proc-macro honest: it only
// stamps metadata; the body of the handler still calls
// `upload_to` explicitly (or a `#[post]` wrapper does, in a
// future wave).
// ---------------------------------------------------------------------------

struct UploadToArgs {
    bucket: LitStr,
    template: LitStr,
}

impl Parse for UploadToArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let bucket: LitStr = input.parse()?;
        let _comma: Token![,] = input.parse()?;
        let template: LitStr = input.parse()?;
        if !input.is_empty() {
            return Err(input.error(
                "#[upload_to(\"bucket\", \"prefix/{id}\")] expects exactly two string literals",
            ));
        }
        Ok(Self { bucket, template })
    }
}

/// Extract `{name}` placeholders from a key template. Names
/// must be valid Rust idents and may not repeat. We return them
/// in declaration order — that's the order the runtime helper
/// will substitute them in.
fn extract_template_params(template: &str) -> syn::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end] != b'}' {
                end += 1;
            }
            if end >= bytes.len() {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!(
                        "upload_to template `{template}` has unterminated `{{` at byte {start}"
                    ),
                ));
            }
            let name = &template[start..end];
            if name.is_empty() {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("upload_to template `{template}` has empty `{{}}` placeholder"),
                ));
            }
            if !seen.insert(name.to_string()) {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("upload_to template `{template}` repeats placeholder `{{{name}}}`"),
                ));
            }
            // Validate the name is a valid ident.
            if syn::parse_str::<Ident>(name).is_err() {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    format!("upload_to template placeholder `{{{name}}}` is not a valid ident"),
                ));
            }
            out.push(name.to_string());
            i = end + 1;
        } else {
            i += 1;
        }
    }
    Ok(out)
}

#[proc_macro_attribute]
pub fn upload_to(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as UploadToArgs);
    let bucket = args.bucket.value();
    let template = args.template.value();
    let template_span = args.template.span();

    let params = match extract_template_params(&template) {
        Ok(p) => p,
        Err(e) => {
            // Re-span the error against the original `LitStr` for better
            // diagnostics, then bail.
            let span_err = syn::Error::new(template_span, e.to_string());
            return span_err.to_compile_error().into();
        }
    };
    let param_idents: Vec<Ident> = params
        .iter()
        .map(|p| Ident::new(p, proc_macro2::Span::call_site()))
        .collect();

    // The decorated item can be either a free function or an
    // inherent impl method. We don't transform its body — we
    // only stamp three companion `pub fn` accessors. For a
    // free function, those become free `pub fn`s with a
    // `__upload_to_<original_fn_name>_*` prefix to avoid name
    // collisions; for an impl method, they're attached to the
    // same `impl` block.
    //
    // Implementation: try `ItemFn` first; if that fails, try
    // `ImplItemFn`; if both fail, error.
    let item_clone = item.clone();
    if let Ok(item_fn) = syn::parse::<ItemFn>(item.clone()) {
        let name = &item_fn.sig.ident;
        let bucket_fn = format_ident!("__upload_to_{}_bucket", name);
        let template_fn = format_ident!("__upload_to_{}_template", name);
        let params_fn = format_ident!("__upload_to_{}_params", name);
        let params_array: Vec<&str> = params.iter().map(String::as_str).collect();
        let expanded = quote! {
            #item_fn

            #[doc(hidden)]
            pub fn #bucket_fn() -> &'static str {
                #bucket
            }

            #[doc(hidden)]
            pub fn #template_fn() -> &'static str {
                #template
            }

            #[doc(hidden)]
            pub fn #params_fn() -> &'static [&'static str] {
                &[ #(#params_array),* ]
            }
        };
        return expanded.into();
    }

    if let Ok(impl_fn) = syn::parse::<ImplItemFn>(item_clone) {
        let name = &impl_fn.sig.ident;
        let bucket_fn = format_ident!("__upload_to_{}_bucket", name);
        let template_fn = format_ident!("__upload_to_{}_template", name);
        let params_fn = format_ident!("__upload_to_{}_params", name);
        let params_array: Vec<&str> = params.iter().map(String::as_str).collect();
        // For `impl Foo { #[upload_to(...)] fn handler(...) }` we
        // emit the method unchanged and attach the three
        // accessors as free `pub fn` siblings in the parent
        // module (with a mangled name). The handler is free to
        // call them — they're not on `Self` because we can't
        // reach the surrounding `impl` block from inside the
        // attribute.
        let expanded = quote! {
            #impl_fn

            #[doc(hidden)]
            pub fn #bucket_fn() -> &'static str {
                #bucket
            }

            #[doc(hidden)]
            pub fn #template_fn() -> &'static str {
                #template
            }

            #[doc(hidden)]
            pub fn #params_fn() -> &'static [&'static str] {
                &[ #(#params_array),* ]
            }
        };
        return expanded.into();
    }

    // Not a function — error.
    let _ = param_idents; // silence unused warning when we return the error
    syn::Error::new(
        proc_macro2::Span::call_site(),
        "#[upload_to] can only decorate a function or method",
    )
    .to_compile_error()
    .into()
}
