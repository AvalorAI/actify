use proc_macro::TokenStream;
use quote::quote;
use syn::{punctuated::Punctuated, token::Comma, FnArg, Ident};

#[proc_macro_attribute]
pub fn actify(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemFn);
    let fn_name = &input.sig.ident;
    let body = &input.block;

    let args = input.sig.inputs;
    let input_args = transform_params(&args);

    let handle_name = syn::Ident::new(&format!("handle_{}", fn_name.to_string()), fn_name.span());

    let result = quote! {

        fn #handle_name(#args) {
            println!("Calling the handle");
            self.#fn_name(#input_args);
        }

        fn #fn_name(#args) #body

    };
    result.into()
}

fn transform_params(params: &Punctuated<FnArg, Comma>) -> Punctuated<Ident, Comma> {
    // 1. Filter the params, so that only typed arguments remain
    // 2. Extract the ident (in case the pattern type is ident)
    let idents = params.iter().filter_map(|param| {
        if let syn::FnArg::Typed(pat_type) = param {
            if let syn::Pat::Ident(pat_ident) = *pat_type.pat.clone() {
                return Some(pat_ident.ident);
            }
        }
        None
    });

    // Add all idents to a Punctuated => param1, param2, ...
    let mut punctuated: Punctuated<syn::Ident, Comma> = Punctuated::new();
    idents.for_each(|ident| punctuated.push(ident));

    punctuated
}
