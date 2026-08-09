//! Procedural macros for the [actify](https://docs.rs/actify) crate.
//!
//! Depend on `actify` rather than this crate: everything here is re-exported
//! from there, and the generated code refers to `actify`'s types.
#![warn(missing_docs)]

mod codegen;
mod parse;

use proc_macro::TokenStream;

/// Marks a method inside an `#[actify]` impl block as not broadcasting.
///
/// ```ignore
/// #[actify]
/// impl Counter {
///     #[actify::skip_broadcast]
///     fn bump_quietly(&mut self) { self.count += 1 }
/// }
/// ```
///
/// Expands to nothing: `#[actify]` reads it and strips it from the output.
#[proc_macro_attribute]
pub fn skip_broadcast(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Restores broadcasting for one method of an `#[actify(skip_broadcast)]` block.
///
/// ```ignore
/// #[actify(skip_broadcast)]
/// impl Counter {
///     #[actify::broadcast]
///     fn bump_loudly(&mut self) { self.count += 1 }
/// }
/// ```
///
/// Expands to nothing: `#[actify]` reads it and strips it from the output.
#[proc_macro_attribute]
pub fn broadcast(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// Parsed arguments from `#[actify(...)]`.
struct ActifyArgs {
    skip_broadcast: bool,
    custom_name: Option<syn::LitStr>,
}

impl syn::parse::Parse for ActifyArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut args = ActifyArgs {
            skip_broadcast: false,
            custom_name: None,
        };

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            if ident == "skip_broadcast" {
                args.skip_broadcast = true;
            } else if ident == "name" {
                input.parse::<syn::Token![=]>()?;
                let name: syn::LitStr = input.parse()?;
                args.custom_name = Some(name);
            } else if ident == "broadcast" {
                return Err(syn::Error::new_spanned(
                    ident,
                    "`#[actify(broadcast)]` is not supported; methods taking &mut self broadcast by default, and a method taking &self opts in with `#[actify::broadcast]`",
                ));
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "unknown actify attribute; expected `skip_broadcast` or `name = \"...\"`",
                ));
            }

            if !input.is_empty() {
                input.parse::<syn::Token![,]>()?;
            }
        }

        Ok(args)
    }
}

/// Emit diagnostics together with the impl block they came from.
///
/// Returning only the errors would delete every method of the type, so each
/// call site would report a further "no method named ..." error and bury the
/// diagnostic that actually explains the problem.
fn report(error: syn::Error, impl_block: &syn::ItemImpl) -> TokenStream {
    let compile_errors = error.to_compile_error();
    quote::quote! {
        #compile_errors
        #impl_block
    }
    .into()
}

/// The actify macro expands an impl block of a rust struct to support usage in an actor model.
/// Effectively, this macro allows to remotely call an actor method through a handle.
/// By using traits, the methods on the handle have the same signatures, so that type checking is enforced
#[proc_macro_attribute]
pub fn actify(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut impl_block = syn::parse_macro_input!(item as syn::ItemImpl);

    let args = match syn::parse::<ActifyArgs>(attr) {
        Ok(args) => args,
        Err(error) => return report(error, &impl_block),
    };

    match parse::ImplInfo::from_impl_block(&mut impl_block, args.skip_broadcast, args.custom_name) {
        Ok(info) => codegen::generate(&info).into(),
        Err(error) => report(error, &impl_block),
    }
}
