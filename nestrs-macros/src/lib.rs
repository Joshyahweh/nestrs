use proc_macro::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, DeriveInput, Expr, Field, Fields, Ident, ImplItemFn, Item, ItemStruct,
    LitInt, LitStr, Meta, Result, Token, Type,
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

    struct ImportItem {
        ty: Type,
        forward_ref: bool,
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

    let imports_exprs = args.imports;
    let mut imports_static = Vec::<ImportItem>::new();
    let mut imports_dynamic = Vec::<Expr>::new();
    for expr in imports_exprs {
        if let Some(ty) = forward_ref_expr_to_type(&expr) {
            imports_static.push(ImportItem {
                ty,
                forward_ref: true,
            });
            continue;
        }

        match expr {
            Expr::Path(p) => imports_static.push(ImportItem {
                ty: Type::Path(syn::TypePath { qself: None, path: p.path }),
                forward_ref: false,
            }),
            other => imports_dynamic.push(other),
        };
    }

    let import_builds = imports_static
        .iter()
        .map(|imp| {
            let ty = &imp.ty;
            let is_forward_ref = imp.forward_ref;
            if is_forward_ref {
                quote! {
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
                }
            } else {
                quote! {
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
                }
            }
        })
        .collect::<Vec<_>>();

    let import_graph_providers = imports_static
        .iter()
        .map(|imp| {
            let ty = &imp.ty;
            let is_forward_ref = imp.forward_ref;
            if is_forward_ref {
                quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            // `forward_ref` back-edge: referenced module is already being traversed.
                            // Skip to avoid infinite recursion.
                        } else {
                            <#ty as nestrs::core::ModuleGraph>::register_providers(registry);
                        }
                    }
                }
            } else {
                quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            nestrs::core::__nestrs_panic_circular_module_dependency(std::any::type_name::<#ty>());
                        }
                        <#ty as nestrs::core::ModuleGraph>::register_providers(registry);
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    let import_graph_controllers = imports_static
        .iter()
        .map(|imp| {
            let ty = &imp.ty;
            let is_forward_ref = imp.forward_ref;
            if is_forward_ref {
                quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            // `forward_ref` back-edge: referenced module is already being traversed.
                            // Skip to avoid infinite recursion.
                        } else {
                            router = <#ty as nestrs::core::ModuleGraph>::register_controllers(router, registry);
                        }
                    }
                }
            } else {
                quote! {
                    {
                        let __type_id = std::any::TypeId::of::<#ty>();
                        if nestrs::core::__nestrs_module_stack_contains(__type_id) {
                            nestrs::core::__nestrs_panic_circular_module_dependency(std::any::type_name::<#ty>());
                        }
                        router = <#ty as nestrs::core::ModuleGraph>::register_controllers(router, registry);
                    }
                }
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

        impl nestrs::core::Module for #name {
            fn build() -> (nestrs::core::ProviderRegistry, axum::Router) {
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
                format!("unsupported injectable scope `{other}` (expected singleton|transient|request)"),
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
                let field_ident = field
                    .ident
                    .as_ref()
                    .expect("named field should have ident");
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
                    return syn::Error::new_spanned(
                        ty,
                        "Arc field must be `Arc<T>`",
                    )
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

                quote! {
                    #field_ident: registry.get::<#inner>()
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
                Err(meta.error(
                    "unknown controller key; expected `prefix`, `version`, or `host`",
                ))
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
    pipes: Vec<Type>,
    interceptors: Vec<Type>,
    filters: Vec<Type>,
    metadata: Vec<(LitStr, LitStr)>,
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
    seg.ident.to_string() == "ValidationPipe"
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
    seg.ident.to_string() == "WsClient"
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
    seg.ident.to_string() == "TransportError"
}

fn is_http_exception_type(ty: &Type) -> bool {
    let Type::Path(tp) = ty else {
        return false;
    };
    let Some(seg) = tp.path.segments.last() else {
        return false;
    };
    seg.ident.to_string() == "HttpException"
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
        let args: Punctuated<LitStr, Token![,]> =
            Punctuated::<LitStr, Token![,]>::parse_terminated
                .parse2(list.tokens.clone())
                .map_err(|_| {
                    syn::Error::new_spanned(
                        list,
                        "roles expects one or more string literals, e.g. #[roles(\"admin\")]",
                    )
                })?;
        let joined = args
            .iter()
            .map(|s| s.value())
            .collect::<Vec<_>>()
            .join(",");
        out.push((
            LitStr::new("roles", attr.span()),
            LitStr::new(&joined, attr.span()),
        ));
    }
    Ok(out)
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
        });
    }

    if routes.is_empty() {
        let msg = format!(
            "routes found no #[get]/#[post]/... handlers in impl {} {{ ... }}",
            controller_ident
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
                #method #path with ( #(#guards),* )
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

        handlers.push(WsHandlerDef {
            event,
            name: func.sig.ident.clone(),
            expects_client,
            payload_ty,
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
                        let payload = payload.clone();
                        let _ = self.#name(payload).await;
                    }
                } else {
                    quote! {
                        let payload = payload.clone();
                        let __value: #payload_ty = match nestrs::serde_json::from_value(payload) {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = client.emit(
                                    "error",
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
                        let payload = payload.clone();
                        let _ = self.#name(client.clone(), payload).await;
                    }
                } else {
                    quote! {
                        let payload = payload.clone();
                        let __value: #payload_ty = match nestrs::serde_json::from_value(payload) {
                            Ok(v) => v,
                            Err(e) => {
                                let _ = client.emit(
                                    "error",
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

        arms.push(quote! {
            #event => { #call }
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
                            "error",
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
        return syn::Error::new_spanned(item_impl, "event_routes supports inherent impl blocks only")
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
            return syn::Error::new_spanned(
                func.sig.clone(),
                "scheduled tasks must be async",
            )
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
            return syn::Error::new_spanned(
                func,
                "scheduled tasks must be one of: (&self)",
            )
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

    let job_stmts = tasks
        .into_iter()
        .map(|t| {
            let name = t.name;
            match t.kind {
                TaskKind::Cron(expr) => {
                    quote! {
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
                    }
                }
                TaskKind::Interval(ms) => {
                    quote! {
                        let job = nestrs::schedule::Job::new_repeated_async(
                            ::std::time::Duration::from_millis(#ms as u64),
                            {
                                let service = service.clone();
                                move |_uuid, _lock| {
                                    let service = service.clone();
                                    ::std::boxed::Box::pin(async move {
                                        let _ = service.#name().await;
                                    })
                                }
                            },
                        )
                        .unwrap_or_else(|e| panic!("failed to register interval job: {e:?}"));
                        jobs.push(job);
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    let expanded = quote! {
        #item_impl

        const _: () = {
            fn __nestrs_build(registry: &nestrs::core::ProviderRegistry) -> ::std::vec::Vec<nestrs::schedule::Job> {
                let service = registry.get::<#self_ty>();
                let mut jobs = ::std::vec::Vec::<nestrs::schedule::Job>::new();
                #(#job_stmts)*
                jobs
            }

            #[nestrs::schedule::linkme::distributed_slice(nestrs::schedule::SCHEDULE_REGISTRATIONS)]
            static __NES_SCHEDULE: nestrs::schedule::ScheduleRegistration =
                nestrs::schedule::ScheduleRegistration { build: __nestrs_build };
        };
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn micro_routes(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_impl = parse_macro_input!(item as syn::ItemImpl);

    if item_impl.trait_.is_some() {
        return syn::Error::new_spanned(item_impl, "micro_routes supports inherent impl blocks only")
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

        handlers.push(MsHandlerDef {
            pattern,
            name: func.sig.ident.clone(),
            is_message,
            payload_ty,
            ok_ty,
            err_ty,
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

            let decode = if let Some(payload_ty) = payload_ty.clone() {
                if is_serde_json_value_type(&payload_ty) {
                    quote! { let __payload = payload.clone(); }
                } else {
                    quote! {
                        let __payload: #payload_ty = match nestrs::serde_json::from_value(payload.clone()) {
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
                (Some(payload_ty), Some(ok_ty), None) => {
                    if is_serde_json_value_type(payload_ty) {
                        quote! {
                            let __out: #ok_ty = self.#name(__payload).await;
                            nestrs::serde_json::to_value(__out)
                                .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}")))
                        }
                    } else {
                        quote! {
                            let __out: #ok_ty = self.#name(__payload).await;
                            nestrs::serde_json::to_value(__out)
                                .map_err(|e| nestrs::microservices::TransportError::new(format!("serialize response failed: {e}")))
                        }
                    }
                }
                (None, Some(ok_ty), Some(err_ty)) => {
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
                (Some(payload_ty), Some(_ok_ty), Some(err_ty)) => {
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
            let decode = if let Some(payload_ty) = payload_ty.clone() {
                if is_serde_json_value_type(&payload_ty) {
                    quote! { let __payload = payload.clone(); }
                } else {
                    quote! {
                        let __payload: #payload_ty = match nestrs::serde_json::from_value(payload.clone()) {
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
                Some(payload_ty) => {
                    if is_serde_json_value_type(payload_ty) {
                        quote! { let _ = self.#name(__payload).await; }
                    } else {
                        quote! { let _ = self.#name(__payload).await; }
                    }
                }
            };

            event_arms.push(quote! {
                #pattern => {
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
#[proc_macro_attribute]
pub fn serialize(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut method = parse_macro_input!(item as ImplItemFn);
    let output = method.sig.output.clone();
    let block = method.block;

    method.sig.output = syn::parse_quote!(-> axum::response::Response);

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
                        Ok(__ok) => axum::response::IntoResponse::into_response(axum::Json(__ok)),
                        Err(__err) => axum::response::IntoResponse::into_response(__err),
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

fn convert_dto_field_attrs(field: &Field) -> Vec<syn::Attribute> {
    let mut out = Vec::new();

    for attr in &field.attrs {
        let Some(name) = attr.path().get_ident().map(|v| v.to_string()) else {
            out.push(attr.clone());
            continue;
        };

        match name.as_str() {
            "IsEmail" => out.push(syn::parse_quote!(#[validate(email)])),
            "IsNotEmpty" => out.push(syn::parse_quote!(#[validate(length(min = 1))])),
            "IsString" => {
                // Type-level no-op in Rust, retained for Nest-like readability.
            }
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
            // Nest `@IsOptional()` maps to `Option<T>` in Rust; strip the marker attribute.
            "IsOptional" => {}
            "ValidateNested" => out.push(syn::parse_quote!(#[validate(nested)])),
            _ => out.push(attr.clone()),
        }
    }

    out
}

#[proc_macro_attribute]
pub fn dto(_attr: TokenStream, item: TokenStream) -> TokenStream {
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

    let field_defs = fields.named.iter().map(|field| {
        let attrs = convert_dto_field_attrs(field);
        let field_ident = field.ident.clone();
        let ty = field.ty.clone();
        quote! {
            #(#attrs)*
            pub #field_ident: #ty
        }
    });

    let expanded = quote! {
        #[derive(Debug, Clone, serde::Deserialize, serde::Serialize, validator::Validate, nestrs::NestDto)]
        #vis struct #ident {
            #(#field_defs,)*
        }
    };

    expanded.into()
}

#[proc_macro_derive(
    NestDto,
    attributes(IsString, IsEmail, IsNotEmpty, MinLength, MaxLength, Length)
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
