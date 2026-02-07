mod codegen;
mod parse;

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn skip_broadcast(_args: TokenStream, input: TokenStream) -> TokenStream {
    input
}

/// The actify macro expands an impl block of a rust struct to support usage in an actor model.
/// Effectively, this macro allows to remotely call an actor method through a handle.
/// By using traits, the methods on the handle have the same signatures, so that type checking is enforced
#[proc_macro_attribute]
pub fn actify(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut impl_block = syn::parse_macro_input!(item as syn::ItemImpl);
    match parse::ImplInfo::from_impl_block(&mut impl_block) {
        Ok(info) => codegen::generate(&info).into(),
        Err(err) => err.into(),
    }
}
