mod actor;
mod handle;

use crate::parse::ImplInfo;
use quote::quote;

/// Generate all output code from the parsed IR.
pub fn generate(info: &ImplInfo) -> proc_macro2::TokenStream {
    let handle_trait = handle::generate_trait(info);
    let handle_trait_impl = handle::generate_trait_impl(info);
    let actor_trait = actor::generate_trait(info);
    let actor_trait_impl = actor::generate_trait_impl(info);
    let original_impl = &info.original_impl;

    quote! {
        #handle_trait
        #handle_trait_impl
        #actor_trait
        #actor_trait_impl
        #original_impl
    }
}
