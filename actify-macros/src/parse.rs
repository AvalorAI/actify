use proc_macro2::Span;
use quote::quote_spanned;
use syn::{
    Attribute, FnArg, Generics, Ident, ImplItem, ImplItemFn, ItemImpl, Path, Receiver, ReturnType,
    Type, punctuated::Punctuated, spanned::Spanned, token::Comma,
};

/// Intermediate representation for an entire impl block processed by `#[actify]`.
pub struct ImplInfo {
    /// The full impl type, e.g. `TestStruct<T>`.
    pub impl_type: Box<Type>,
    /// Just the type name, e.g. `TestStruct`.
    pub type_ident: Ident,
    /// Generated handle trait name, e.g. `TestStructHandle`.
    pub handle_trait_ident: Ident,
    /// Impl-level generics (where clause guaranteed present via `make_where_clause`).
    pub generics: Generics,
    /// If this is a trait impl, the trait path (e.g. `ActorVec<T>`).
    pub trait_path: Option<Path>,
    /// Filtered cfg/doc attributes from the impl block.
    pub attributes: Vec<Attribute>,
    /// Parsed methods.
    pub methods: Vec<MethodInfo>,
    /// The original (mutated) impl block, included in output for passthrough.
    pub original_impl: ItemImpl,
}

impl ImplInfo {
    /// Parse an `ItemImpl` into the intermediate representation.
    /// Mutates the impl block to ensure a where clause exists.
    pub fn from_impl_block(
        impl_block: &mut ItemImpl,
        skip_all_broadcasts: bool,
        custom_name: Option<syn::LitStr>,
    ) -> Result<ImplInfo, proc_macro2::TokenStream> {
        let type_ident = get_impl_type_ident(&impl_block.self_ty)?;

        // Ensure the where clause always exists so we can unwrap safely
        impl_block.generics.make_where_clause();

        let handle_trait_ident = if let Some(lit) = custom_name {
            let name = lit.value();
            syn::parse_str::<Ident>(&name).map_err(|_| {
                quote_spanned! {
                    lit.span() =>
                    compile_error!("invalid `name` value: must be a valid Rust identifier");
                }
            })?
        } else {
            Ident::new(&format!("{type_ident}Handle"), Span::call_site())
        };

        let trait_path = impl_block.trait_.as_ref().map(|(_, path, _)| path.clone());

        let attributes = filter_attributes(&impl_block.attrs);

        let mut methods = Vec::new();
        for item in &impl_block.items {
            if let ImplItem::Fn(method) = item {
                methods.push(MethodInfo::from_impl_method(method, skip_all_broadcasts)?);
            }
        }

        Ok(ImplInfo {
            impl_type: impl_block.self_ty.clone(),
            type_ident,
            handle_trait_ident,
            generics: impl_block.generics.clone(),
            trait_path,
            attributes,
            methods,
            original_impl: impl_block.clone(),
        })
    }
}

/// Intermediate representation for a single method within the impl block.
pub struct MethodInfo {
    /// Original method name.
    pub ident: Ident,
    /// Whether the method takes `&mut self`.
    pub is_mutable: bool,
    /// Whether the method is async.
    pub is_async: bool,
    /// Whether `#[actify::skip_broadcast]` is present.
    pub skip_broadcast: bool,
    /// Argument identifiers. For destructuring patterns a positional name is generated.
    pub arg_names: Punctuated<Ident, Comma>,
    /// Argument types.
    pub arg_types: Punctuated<Type, Comma>,
    /// Return type (defaults to `()`).
    pub output_type: Box<Type>,
    /// Method-level generics (including where clause).
    pub method_generics: Generics,
    /// Filtered cfg/doc attributes.
    pub attributes: Vec<Attribute>,
}

impl MethodInfo {
    /// Parse a single `ImplItemFn` into its intermediate representation.
    fn from_impl_method(
        method: &ImplItemFn,
        skip_all_broadcasts: bool,
    ) -> Result<MethodInfo, proc_macro2::TokenStream> {
        let ident = method.sig.ident.clone();

        let is_mutable = method.sig.inputs.iter().any(|arg| {
            matches!(
                arg,
                FnArg::Receiver(Receiver {
                    mutability: Some(_),
                    ..
                })
            )
        });
        let is_async = method.sig.asyncness.is_some();

        let skip_attr = method.attrs.iter().find(|attr| {
            attr.path()
                .segments
                .iter()
                .any(|seg| seg.ident == "skip_broadcast")
        });
        let broadcast_attr = method.attrs.iter().find(|attr| {
            attr.path()
                .segments
                .iter()
                .any(|seg| seg.ident == "broadcast")
        });

        if skip_all_broadcasts {
            if let Some(attr) = skip_attr {
                return Err(quote_spanned! {
                    attr.span() =>
                    compile_error!("#[skip_broadcast] is superfluous: the impl block already skips all broadcasts via #[actify(skip_broadcast)]");
                });
            }
        } else if let Some(attr) = broadcast_attr {
            return Err(quote_spanned! {
                attr.span() =>
                compile_error!("#[broadcast] is superfluous: methods already broadcast by default; use #[actify(skip_broadcast)] on the impl block to change the default");
            });
        }

        let skip_broadcast = if skip_all_broadcasts {
            broadcast_attr.is_none()
        } else {
            skip_attr.is_some()
        };

        validate_has_receiver(method)?;

        let (arg_names, arg_types) = transform_args(&method.sig.inputs)?;

        let output_type = match &method.sig.output {
            ReturnType::Type(_, ty) => ty.clone(),
            ReturnType::Default => Box::new(syn::parse_quote! { () }),
        };

        let attributes = filter_attributes(&method.attrs);

        Ok(MethodInfo {
            ident,
            is_mutable,
            is_async,
            skip_broadcast,
            arg_names,
            arg_types,
            output_type,
            method_generics: method.sig.generics.clone(),
            attributes,
        })
    }
}

/// Extract the type name from a named type path (e.g. `MyStruct` from `MyStruct<T>`).
/// Returns the last path segment's ident, so `crate::module::Foo<T>` yields `Foo`.
fn get_impl_type_ident(impl_type: &Type) -> Result<Ident, proc_macro2::TokenStream> {
    if let Type::Path(type_path) = impl_type {
        if let Some(last_segment) = type_path.path.segments.last() {
            return Ok(last_segment.ident.clone());
        }
    }

    Err(quote_spanned! {
        impl_type.span() =>
        compile_error!("The actify macro requires a named type path (e.g. `impl MyStruct`), not a reference, tuple, or other type expression");
    })
}

/// Built-in compiler attributes that are safe to propagate onto generated trait
/// signatures and handle impl methods.  Everything else (proc-macro attributes
/// like `#[instrument]`, actify-specific attributes like `#[skip_broadcast]`)
/// is stripped so it only appears on the original impl method where it belongs.
const PROPAGATED_ATTRIBUTES: &[&str] = &[
    "doc",
    "allow",
    "warn",
    "deny",
    "forbid",
    "cfg",
    "cfg_attr",
    "deprecated",
    "must_use",
];

/// Returns `true` if the attribute is in the [`PROPAGATED_ATTRIBUTES`] whitelist.
///
/// Only single-segment paths are checked (all built-in compiler attributes are
/// single-segment). Multi-segment paths like `tracing::instrument` or
/// `actify::skip_broadcast` are always excluded.
fn is_propagated_attribute(attr: &Attribute) -> bool {
    let segments = &attr.path().segments;
    segments.len() == 1
        && segments
            .first()
            .map_or(false, |seg| PROPAGATED_ATTRIBUTES.contains(&seg.ident.to_string().as_str()))
}

/// Keep only whitelisted built-in attributes for generated code.
///
/// Proc-macro attributes (e.g. `#[instrument]`) and actify-specific attributes
/// (e.g. `#[skip_broadcast]`) are stripped — the former because they transform
/// function bodies and are semantically wrong on generated plumbing code, the
/// latter because they are consumed during parsing.
fn filter_attributes(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| is_propagated_attribute(attr))
        .cloned()
        .collect()
}

/// Verify the method has a receiver (`&self` or `&mut self`).
fn validate_has_receiver(method: &ImplItemFn) -> Result<(), proc_macro2::TokenStream> {
    let has_receiver = method
        .sig
        .inputs
        .iter()
        .any(|arg| matches!(arg, FnArg::Receiver(_)));

    if !has_receiver {
        return Err(quote_spanned! {
            method.span() =>
            compile_error!("Static method cannot be actified: the method requires a receiver to the impl type, using either &self or &mut self");
        });
    }

    Ok(())
}

/// Extract and validate argument names and types from method inputs.
/// For ident patterns, uses the original name. For non-ident patterns (e.g.
/// destructuring `(a, b): (i32, i32)`), generates a positional name so the
/// handle can box/unbox the value; the original method destructures at the call site.
#[allow(clippy::type_complexity, clippy::single_match)]
fn transform_args(
    args: &Punctuated<FnArg, Comma>,
) -> Result<(Punctuated<Ident, Comma>, Punctuated<Type, Comma>), proc_macro2::TokenStream> {
    let mut arg_names: Punctuated<Ident, Comma> = Punctuated::new();
    let mut arg_types: Punctuated<Type, Comma> = Punctuated::new();

    for (i, arg) in args.iter().enumerate() {
        match arg {
            syn::FnArg::Typed(pat_type) => {
                validate_arg_type(&pat_type.ty, pat_type.ty.span())?;

                let ident = match &*pat_type.pat {
                    syn::Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                    _ => Ident::new(&format!("_arg{}", i), Span::call_site()),
                };

                arg_names.push(ident);
                arg_types.push(*pat_type.ty.clone());
            }
            #[cfg_attr(test, deny(clippy::non_exhaustive_omitted_patterns))]
            _ => {}
        }
    }

    Ok((arg_names, arg_types))
}

/// Validate that an argument type is supported for actor method arguments.
fn validate_arg_type(ty: &Type, span: proc_macro2::Span) -> Result<(), proc_macro2::TokenStream> {
    match ty {
        // Valid owned types
        Type::Path(_)
        | Type::Tuple(_)
        | Type::Array(_)
        | Type::BareFn(_)
        | Type::Paren(_)
        | Type::Group(_) => Ok(()),

        Type::Reference(_) => Err(quote_spanned! {
            span =>
            compile_error!("Input arguments of actor model methods must be owned types, not references (e.g. use String instead of &str)");
        }),

        Type::Ptr(_) => Err(quote_spanned! {
            span =>
            compile_error!("Raw pointer types (*const T, *mut T) are not supported as actor method arguments because they are not Send");
        }),

        Type::ImplTrait(_) => Err(quote_spanned! {
            span =>
            compile_error!("impl Trait is not supported as an actor method argument; use a named generic type parameter with trait bounds instead (e.g. fn method<F: Fn()>(&self, f: F))");
        }),

        _ => Err(quote_spanned! {
            span =>
            compile_error!("Unsupported argument type for actor method; use a concrete owned type (e.g. String, Vec<T>, (A, B), [T; N])");
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    fn attr(tokens: Attribute) -> Attribute {
        tokens
    }

    #[test]
    fn whitelisted_attributes_are_propagated() {
        let attrs: Vec<Attribute> = vec![
            attr(parse_quote!(#[doc = "hello"])),
            attr(parse_quote!(#[allow(unused)])),
            attr(parse_quote!(#[warn(missing_docs)])),
            attr(parse_quote!(#[deny(warnings)])),
            attr(parse_quote!(#[forbid(unsafe_code)])),
            attr(parse_quote!(#[cfg(test)])),
            attr(parse_quote!(#[cfg_attr(test, ignore)])),
            attr(parse_quote!(#[deprecated])),
            attr(parse_quote!(#[must_use])),
        ];

        let filtered = filter_attributes(&attrs);
        assert_eq!(filtered.len(), attrs.len(), "all whitelisted attributes should pass through");
    }

    #[test]
    fn instrument_is_stripped() {
        let attrs: Vec<Attribute> = vec![
            attr(parse_quote!(#[doc = "keep me"])),
            attr(parse_quote!(#[instrument(level = "debug", skip_all)])),
        ];

        let filtered = filter_attributes(&attrs);
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].path().is_ident("doc"));
    }

    #[test]
    fn qualified_instrument_is_stripped() {
        let attrs: Vec<Attribute> = vec![
            attr(parse_quote!(#[tracing::instrument(skip_all)])),
            attr(parse_quote!(#[cfg(feature = "tracing")])),
        ];

        let filtered = filter_attributes(&attrs);
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].path().is_ident("cfg"));
    }

    #[test]
    fn actify_attrs_are_stripped() {
        let attrs: Vec<Attribute> = vec![
            attr(parse_quote!(#[skip_broadcast])),
            attr(parse_quote!(#[broadcast])),
            attr(parse_quote!(#[actify::skip_broadcast])),
            attr(parse_quote!(#[doc = "visible"])),
        ];

        let filtered = filter_attributes(&attrs);
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].path().is_ident("doc"));
    }

    #[test]
    fn unknown_single_segment_attr_is_stripped() {
        let attrs: Vec<Attribute> = vec![
            attr(parse_quote!(#[serde(rename_all = "camelCase")])),
            attr(parse_quote!(#[deprecated])),
        ];

        let filtered = filter_attributes(&attrs);
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].path().is_ident("deprecated"));
    }
}
