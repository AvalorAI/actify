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

/// The actify macro expands an impl block of a rust struct to support usage in an actor model.
/// Effectively, this macro allows to remotely call an actor method through a handle.
/// By using traits, the methods on the handle have the same signatures, so that type checking is enforced
#[proc_macro_attribute]
pub fn actify(attr: TokenStream, item: TokenStream) -> TokenStream {
    let skip_all_broadcasts = if attr.is_empty() {
        false
    } else {
        let ident = syn::parse_macro_input!(attr as syn::Ident);
        if ident == "skip_broadcast" {
            true
        } else if ident == "broadcast" {
            return syn::Error::new_spanned(ident, "methods already broadcast by default; `#[actify(broadcast)]` is unnecessary")
                .to_compile_error()
                .into();
        } else {
            return syn::Error::new_spanned(ident, "unknown actify attribute; expected `skip_broadcast`")
                .to_compile_error()
                .into();
        }
    };

    let mut impl_block = syn::parse_macro_input!(item as syn::ItemImpl);
    match parse::ImplInfo::from_impl_block(&mut impl_block, skip_all_broadcasts) {
        Ok(info) => codegen::generate(&info).into(),
        Err(err) => err.into(),
    }
}
