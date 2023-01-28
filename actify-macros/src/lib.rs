use proc_macro::TokenStream;
use quote::quote;
use syn::{punctuated::Punctuated, token::Comma, FnArg, Ident, ImplItem, ImplItemMethod};

#[proc_macro_attribute]
pub fn actify(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemImpl);

    let mut impl_tokenstream = proc_macro2::TokenStream::new();
    for item in &input.items {
        match item {
            ImplItem::Method(method) => impl_tokenstream.extend(get_method_token_streams(method)),
            _ => {}
        }
    }

    let impl_name = input.self_ty;

    let result = quote! {
    impl #impl_name {
        #impl_tokenstream
    }
    };
    result.into()
}

fn get_method_token_streams(method: &ImplItemMethod) -> proc_macro2::TokenStream {
    let fn_name = &method.sig.ident;
    let body = &method.block;
    let args = &method.sig.inputs;
    let input_args = transform_params(&args);

    let handle_name = syn::Ident::new(&format!("handle_{}", fn_name.to_string()), fn_name.span());
    let result = quote! {
        fn #handle_name(#args) {
            println!("Calling the handle");
            self.#fn_name(#input_args);
        }

        fn #fn_name(#args) #body

    };
    result
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
