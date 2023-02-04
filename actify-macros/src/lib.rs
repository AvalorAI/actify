use proc_macro::TokenStream;
use quote::{quote, quote_spanned, ToTokens};
use syn::{
    punctuated::Punctuated, spanned::Spanned, token::Comma, FnArg, Ident, ImplItem, ImplItemMethod,
};

#[proc_macro_attribute]
pub fn actify(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemImpl);

    // let name = input.self_ty;
    let mut name_tokens = proc_macro2::TokenStream::new();
    input.self_ty.to_tokens(&mut name_tokens);
    let name_str = name_tokens.to_string();
    println!("name_str {:?}", name_str);
    let handle_name = syn::Ident::new(&format!("{}Handle", name_tokens.to_string()), input.span());

    let mut impl_tokenstream = proc_macro2::TokenStream::new();
    for item in &input.items {
        match item {
            ImplItem::Method(method) => impl_tokenstream.extend(get_method_token_streams(method)),
            _ => {}
        }
    }

    let impl_name = input.self_ty;
    // let handle_trait = format!("{}Handle", impl_name.to_string());

    let result = quote! {
        #[async_trait::async_trait]
        pub trait #handle_name
        {

        }


    impl #impl_name {
        #impl_tokenstream
    }
    };

    println!("{:?}", result.to_string());

    result.into()
}

fn get_method_token_streams(method: &ImplItemMethod) -> proc_macro2::TokenStream {
    let fn_name = &method.sig.ident;
    let body = &method.block;
    let args = &method.sig.inputs;
    if let Some(err) = check_for_references(&args) {
        return err;
    }

    let input_args = transform_args(&args);

    let method_name = syn::Ident::new(&format!("handle_{}", fn_name.to_string()), fn_name.span());
    let result = quote! {
        fn #method_name(#args) {
            println!("Calling the handle");
            self.#fn_name(#input_args);
        }

        fn #fn_name(#args) #body

    };
    result
}

fn check_for_references(args: &Punctuated<FnArg, Comma>) -> Option<proc_macro2::TokenStream> {
    for arg in args {
        if let syn::FnArg::Typed(pat_type) = arg {
            if let syn::Type::Reference(_) = &*pat_type.ty {
                return Some(quote_spanned! {
                    pat_type.ty.span() =>
                    compile_error!("Input arguments of actor model methods must be owned types and not referenced");
                });
            }
        }
    }
    None
}

fn transform_args(args: &Punctuated<FnArg, Comma>) -> Punctuated<Ident, Comma> {
    // 1. Filter the args, so that only typed arguments remain
    // 2. Extract the ident (in case the pattern type is ident)
    let idents = args.iter().filter_map(|param| {
        if let syn::FnArg::Typed(pat_type) = param {
            if let syn::Pat::Ident(pat_ident) = *pat_type.pat.clone() {
                return Some(pat_ident.ident);
            }
        }
        None
    });

    // Add all idents to a Punctuated => param1, param2, ...
    let mut input_args: Punctuated<syn::Ident, Comma> = Punctuated::new();
    idents.for_each(|ident| input_args.push(ident));
    input_args
}
