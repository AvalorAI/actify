use proc_macro2::Span;
use quote::quote_spanned;
use syn::{
    Attribute, FnArg, Generics, Ident, ImplItem, ImplItemFn, ItemImpl, PatIdent, Path, Receiver,
    ReturnType, Type, punctuated::Punctuated, spanned::Spanned, token::Comma,
};

/// Intermediate representation for an entire impl block processed by `#[actify]`.
pub struct ImplInfo {
    /// The full impl type, e.g. `TestStruct<T>`.
    pub impl_type: Box<Type>,
    /// Just the type name, e.g. `TestStruct`.
    pub type_ident: Ident,
    /// Generated actor trait name, e.g. `TestStructActor`.
    pub actor_trait_ident: Ident,
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
    ) -> Result<ImplInfo, proc_macro2::TokenStream> {
        let type_ident = get_impl_type_ident(&impl_block.self_ty)?;

        // Ensure the where clause always exists so we can unwrap safely
        impl_block.generics.make_where_clause();

        let actor_trait_ident = Ident::new(&format!("{}Actor", type_ident), Span::call_site());
        let handle_trait_ident = Ident::new(&format!("{}Handle", type_ident), Span::call_site());

        let trait_path = impl_block.trait_.as_ref().map(|(_, path, _)| path.clone());

        let attributes = filter_attributes(&impl_block.attrs);

        let mut methods = Vec::new();
        for item in &impl_block.items {
            if let ImplItem::Fn(method) = item {
                methods.push(MethodInfo::from_impl_method(method)?);
            }
        }

        Ok(ImplInfo {
            impl_type: impl_block.self_ty.clone(),
            type_ident,
            actor_trait_ident,
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
    /// Actor-side method name (prefixed with `_`).
    pub actor_ident: Ident,
    /// Whether the method takes `&mut self`.
    pub is_mutable: bool,
    /// Whether the method is async.
    pub is_async: bool,
    /// Whether `#[actify::skip_broadcast]` is present.
    pub skip_broadcast: bool,
    /// Argument identifiers (mutability stripped).
    pub arg_names: Punctuated<PatIdent, Comma>,
    /// Argument types.
    pub arg_types: Punctuated<Type, Comma>,
    /// Return type (defaults to `()`).
    pub output_type: Box<Type>,
    /// Method-level generics (including where clause).
    pub method_generics: Generics,
    /// Just the type parameter idents, for turbofish calls.
    pub generic_type_params: Vec<Ident>,
    /// Filtered cfg/doc attributes.
    pub attributes: Vec<Attribute>,
}

impl MethodInfo {
    /// Parse a single `ImplItemFn` into its intermediate representation.
    fn from_impl_method(method: &ImplItemFn) -> Result<MethodInfo, proc_macro2::TokenStream> {
        let ident = method.sig.ident.clone();
        let actor_ident = Ident::new(&format!("_{}", ident), Span::call_site());

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

        let skip_broadcast = method.attrs.iter().any(|attr| {
            attr.path()
                .segments
                .iter()
                .any(|seg| seg.ident == "skip_broadcast")
        });

        let (arg_names, arg_types) = transform_args(&method.sig.inputs)?;

        let output_type = match &method.sig.output {
            ReturnType::Type(_, ty) => ty.clone(),
            ReturnType::Default => Box::new(syn::parse_quote! { () }),
        };

        let generic_type_params = method
            .sig
            .generics
            .params
            .iter()
            .filter_map(|param| {
                if let syn::GenericParam::Type(type_param) = param {
                    Some(type_param.ident.clone())
                } else {
                    None
                }
            })
            .collect();

        let attributes = filter_attributes(&method.attrs);

        validate_has_receiver(method)?;

        Ok(MethodInfo {
            ident,
            actor_ident,
            is_mutable,
            is_async,
            skip_broadcast,
            arg_names,
            arg_types,
            output_type,
            method_generics: method.sig.generics.clone(),
            generic_type_params,
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

/// Propagate all attributes except actify-specific ones (like `skip_broadcast`)
/// that are consumed during parsing and should not appear on generated methods.
fn filter_attributes(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| {
            !attr
                .path()
                .segments
                .iter()
                .any(|seg| seg.ident == "skip_broadcast")
        })
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
/// Validates that all argument types are supported (owned types only).
/// Strips `mut` from argument identifiers: `mut` on a parameter (e.g. `fn foo(mut x: i32)`)
/// is a local binding detail of the method body, not part of the argument's identity.
fn transform_args(
    args: &Punctuated<FnArg, Comma>,
) -> Result<(Punctuated<PatIdent, Comma>, Punctuated<Type, Comma>), proc_macro2::TokenStream> {
    let mut input_arg_names: Punctuated<PatIdent, Comma> = Punctuated::new();
    let mut input_arg_types: Punctuated<Type, Comma> = Punctuated::new();

    for arg in args {
        match arg {
            syn::FnArg::Typed(pat_type) => {
                if let syn::Pat::Ident(mut pat_ident) = *pat_type.pat.clone() {
                    validate_arg_type(&pat_type.ty, pat_type.ty.span())?;
                    pat_ident.mutability = None;
                    input_arg_names.push(pat_ident);
                    input_arg_types.push(*pat_type.ty.clone());
                }
            }
            #[cfg_attr(test, deny(clippy::non_exhaustive_omitted_patterns))]
            _ => {}
        }
    }

    Ok((input_arg_names, input_arg_types))
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
