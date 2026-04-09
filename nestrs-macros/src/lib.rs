use proc_macro::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, DeriveInput, Field, Fields, Ident, ImplItemFn, Item, ItemStruct, LitInt,
    LitStr, Meta, Result, Token, Type,
};

struct ModuleArgs {
    imports: Vec<Type>,
    controllers: Vec<Type>,
    providers: Vec<Type>,
    exports: Vec<Type>,
    re_exports: Vec<Type>,
}

impl Parse for ModuleArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut imports = Vec::new();
        let mut controllers = Vec::new();
        let mut providers = Vec::new();
        let mut exports = Vec::new();
        let mut re_exports = Vec::new();

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            let content;
            syn::bracketed!(content in input);
            let values: Punctuated<Type, Token![,]> =
                content.parse_terminated(Type::parse, Token![,])?;
            let values = values.into_iter().collect::<Vec<_>>();

            match key.to_string().as_str() {
                "imports" => imports = values,
                "controllers" => controllers = values,
                "providers" => providers = values,
                "exports" => exports = values,
                "re_exports" => re_exports = values,
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

    let imports = args.imports;
    let controllers = args.controllers;
    let providers = args.providers;
    let exports = args.exports;
    let re_exports = args.re_exports;

    let expanded = quote! {
        #module_struct

        impl nestrs::core::Module for #name {
            fn build() -> (nestrs::core::ProviderRegistry, axum::Router) {
                let mut registry = nestrs::core::ProviderRegistry::new();
                let mut router = axum::Router::new();

                #(
                    {
                        let (child_registry, child_router) = <#imports as nestrs::core::Module>::build();
                        let child_exports = <#imports as nestrs::core::Module>::exports();
                        registry.absorb_exported(child_registry, &child_exports);
                        router = router.merge(child_router);
                    }
                )*

                #(
                    registry.register::<#providers>();
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
    };

    expanded.into()
}

#[proc_macro_attribute]
pub fn injectable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(item as ItemStruct);
    let name = &item_struct.ident;
    let expanded = quote! {
        #item_struct

        impl nestrs::core::Injectable for #name {
            fn construct(_registry: &nestrs::core::ProviderRegistry) -> std::sync::Arc<Self> {
                std::sync::Arc::new(Self::default())
            }
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

    let (prefix, mut version) = if attr_tokens.is_empty() {
        ("/".to_string(), "".to_string())
    } else if let Ok(v) = syn::parse2::<LitStr>(attr_tokens.clone()) {
        (v.value(), "".to_string())
    } else {
        let mut prefix = "/".to_string();
        let mut version = "".to_string();
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("prefix") {
                let value: LitStr = meta.value()?.parse()?;
                prefix = value.value();
                Ok(())
            } else if meta.path.is_ident("version") {
                let value: LitStr = meta.value()?.parse()?;
                version = value.value();
                Ok(())
            } else {
                Err(meta.error("unknown controller key; expected `prefix` or `version`"))
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
        (prefix, version)
    };
    if version.is_empty() {
        if let Some(v) = version_from_attr {
            version = v;
        }
    }

    let name = &item_struct.ident;
    let expanded = quote! {
        #item_struct

        impl #name {
            pub fn __nestrs_prefix() -> &'static str {
                #prefix
            }

            pub fn __nestrs_version() -> &'static str {
                #version
            }
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
