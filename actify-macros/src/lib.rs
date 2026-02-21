mod codegen;
mod parse;

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn skip_broadcast(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

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
                    "methods already broadcast by default; `#[actify(broadcast)]` is unnecessary",
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

/// The actify macro expands an impl block of a rust struct to support usage in an actor model.
/// Effectively, this macro allows to remotely call an actor method through a handle.
/// By using traits, the methods on the handle have the same signatures, so that type checking is enforced
#[proc_macro_attribute]
pub fn actify(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as ActifyArgs);

    let mut impl_block = syn::parse_macro_input!(item as syn::ItemImpl);
    match parse::ImplInfo::from_impl_block(
        &mut impl_block,
        args.skip_broadcast,
        args.custom_name,
    ) {
        Ok(info) => codegen::generate(&info).into(),
        Err(err) => err.into(),
    }
}
