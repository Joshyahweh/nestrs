//! Proc-macros for `nestrs-oauth2` — Wave 7.13.
//!
//! Two surface items:
//!
//! - `#[derive(HashOnNew)]` — on a struct with one or more fields tagged
//!   `#[hash]`, generates an inherent `new_with_hashed(...)` constructor
//!   that hashes the marked fields via `nestrs_oauth2::password::hash`
//!   before storing them. Backend auto-detection is delegated to the
//!   runtime (see `nestrs_oauth2::password::hash` for prefix rules).
//!
//! - `#[hash]` — inert field-level attribute recognised only by the
//!   `HashOnNew` derive. It accepts no arguments; the field type is
//!   expected to be `String`, and the generated constructor takes
//!   `String` for that parameter (the plain input that gets hashed).
//!
//! Example:
//!
//! ```ignore
//! use nestrs_oauth2_macros::HashOnNew;
//!
//! #[derive(HashOnNew)]
//! pub struct UserRow {
//!     pub email: String,
//!     #[hash]
//!     pub password: String,
//! }
//!
//! let row = UserRow::new_with_hashed("a@b".into(), "hunter2".into());
//! assert!(nestrs_oauth2::password::verify("hunter2", &row.password));
//! ```
//!
//! Generated shape (schematic):
//!
//! ```ignore
//! impl UserRow {
//!     pub fn new_with_hashed(email: String, password: String) -> Self {
//!         Self {
//!             email,
//!             password: ::nestrs_oauth2::password::hash(&password),
//!         }
//!     }
//! }
//! ```

use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{parse_macro_input, spanned::Spanned, Attribute, Data, DeriveInput, Field, Fields, Meta};

/// Derive `HashOnNew` on a named-field struct. Generates an inherent
/// `new_with_hashed(...)` constructor that hashes every field tagged
/// `#[hash]` via `nestrs_oauth2::password::hash` before storing it.
#[proc_macro_derive(HashOnNew, attributes(hash))]
pub fn derive_hash_on_new(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    match impl_hash_on_new(&ast) {
        Ok(ts) => ts,
        Err(e) => e.to_compile_error(),
    }
}

/// Inert helper attribute consumed by `#[derive(HashOnNew)]`. Accepts
/// no arguments — the backend choice is made at hash time via
/// `nestrs_oauth2::password::hash`'s prefix auto-detection (or by
/// passing an explicit `Backend` argument there). Keeping the
/// attribute inert lets the derive own its parsing and avoids a
/// `Parse` impl just to reject unknown keys.
///
/// If used without `#[derive(HashOnNew)]` in scope, Rust will emit an
/// "unused attribute" warning — which is the right signal: the derive
/// is the only consumer.
#[proc_macro_attribute]
pub fn hash(_attr: TokenStream, _item: TokenStream) -> TokenStream {
    TokenStream::new()
}

/// Per-field metadata computed once per derive, then reused for the
/// constructor parameter list and the field-init expressions.
struct FieldMeta {
    ident: syn::Ident,
    ty: syn::Type,
    is_hashed: bool,
}

fn impl_hash_on_new(ast: &DeriveInput) -> syn::Result<TokenStream> {
    let name = &ast.ident;
    let generics = &ast.generics;
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let raw_fields = match &ast.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    name,
                    "`#[derive(HashOnNew)]` only supports named-field structs",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                "`#[derive(HashOnNew)]` only supports structs (not enums or unions)",
            ));
        }
    };

    // Precompute per-field metadata once.
    let metas: Vec<FieldMeta> = raw_fields
        .iter()
        .map(|f| {
            let is_hashed = has_hash_attr(&f.attrs)?;
            let ident = f
                .ident
                .clone()
                .ok_or_else(|| syn::Error::new_spanned(f, "expected named field"))?;
            Ok(FieldMeta {
                ident,
                ty: f.ty.clone(),
                is_hashed,
            })
        })
        .collect::<syn::Result<_>>()?;

    if !metas.iter().any(|m| m.is_hashed) {
        return Err(syn::Error::new_spanned(
            name,
            "`#[derive(HashOnNew)]` requires at least one field marked `#[hash]` \
             (otherwise `new_with_hashed` is just `new`)",
        ));
    }

    // Constructor parameter list — every field appears in struct order.
    // Hashed fields take `String` (the plain input); other fields take
    // their declared type verbatim.
    let ctor_params = metas.iter().map(|m| {
        let fname = &m.ident;
        if m.is_hashed {
            quote! { #fname: String }
        } else {
            let fty = &m.ty;
            quote! { #fname: #fty }
        }
    });

    // Field initialisers — hashed fields go through `password::hash`,
    // plain fields pass through by name. Use `quote_spanned!` on the
    // hash call so that, if the user hasn't enabled the `password`
    // feature on `nestrs-oauth2`, the error points at the field that
    // requested the hash.
    let field_inits = metas.iter().map(|m| {
        let fname = &m.ident;
        if m.is_hashed {
            let span = fname.span();
            quote_spanned! { span =>
                #fname: ::nestrs_oauth2::password::hash(&#fname)
            }
        } else {
            quote! { #fname: #fname }
        }
    });

    let expanded = quote! {
        impl #impl_generics #name #ty_generics #where_clause {
            #[doc = concat!(
                "Construct a `", stringify!(#name),
                "` from raw field values, hashing every field marked `#[hash]` \
                 via [`::nestrs_oauth2::password::hash`] before storage. \
                 Generated by `#[derive(HashOnNew)]` (Wave 7.13)."
            )]
            pub fn new_with_hashed(#(#ctor_params),*) -> Self {
                Self {
                    #(#field_inits),*
                }
            }
        }
    };

    Ok(expanded)
}

/// Returns `Ok(true)` if the field is marked with the bare-marker form
/// `#[hash]`, `Ok(false)` if no `#[hash]` attribute is present, and
/// `Err` if `#[hash(...)]` carries any arguments or `#[hash = "..."]`
/// is used.
fn has_hash_attr(attrs: &[Attribute]) -> syn::Result<bool> {
    for attr in attrs {
        if !attr.path().is_ident("hash") {
            continue;
        }
        match &attr.meta {
            Meta::Path(_) => return Ok(true),
            Meta::List(_) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`#[hash]` accepts no arguments — use the bare marker form. \
                     Backend selection happens at hash time via \
                     `nestrs_oauth2::password::hash(plain, backend)` or via \
                     prefix auto-detection in `verify_any`.",
                ));
            }
            Meta::NameValue(_) => {
                return Err(syn::Error::new_spanned(
                    attr,
                    "`#[hash]` accepts no arguments — use the bare marker form",
                ));
            }
        }
    }
    Ok(false)
}
