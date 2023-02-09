use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::{
    punctuated::Punctuated, spanned::Spanned, token::Comma, FnArg, Ident, ImplItem, ImplItemMethod,
    ItemImpl, ReturnType, Token, Type,
};

// TODO what happens with generics inside the struct impl including lifetimes?

#[proc_macro_attribute]
pub fn actify(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let impl_block = syn::parse_macro_input!(item as syn::ItemImpl);

    let result = match parse_macro(&impl_block) {
        Ok(parsed) => parsed,
        Err(error) => error,
    };

    println!("{:?}", result.to_string());

    result.into()
}

fn parse_macro(
    impl_block: &ItemImpl,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let impl_type = get_impl_type(&impl_block)?;

    let handle_trait = generate_handle_trait(&impl_block, impl_type)?;

    // let mut impl_tokenstream = proc_macro2::TokenStream::new();
    // for item in &impl_block.items {
    //     match item {
    //         ImplItem::Method(method) => impl_tokenstream.extend(get_method_token_streams(method)),
    //         _ => {}
    //     }
    // }

    let result = quote! {

        #handle_trait

        // #impl_block // TODO this enables copy paste of the original impl block! --> guarantees ok code?
    };

    Ok(result)
}

fn generate_handle_trait(
    impl_block: &ItemImpl,
    impl_type: String,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let trait_name = syn::Ident::new(&format!("{}Handle", impl_type), impl_block.span());

    let mut methods = proc_macro2::TokenStream::new();
    for item in &impl_block.items {
        match item {
            ImplItem::Method(method) => methods.extend(generate_handle_trait_method(method)?),
            _ => {}
        }
    }

    let result = quote! {
        #[async_trait::async_trait]
        pub trait #trait_name
        {
            #methods
        }
    };

    Ok(result)
}

/// This method creates a copy of the original method on the actor, but modifies some parts to make it suitable for the handle.
/// First, all methods to the handle are async by default, to allow communication with the actor.
/// Second, it is checked if a receiver is present and its mutability is removed as that is unnecessary.
/// Thirdly, the output type is wrapped in a result. The default type is converted to () in all cases.
fn generate_handle_trait_method(
    method: &ImplItemMethod,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    // Remove any mutability, as a handle is never required to be so even if the actor method is
    let mut modified_method = method.clone();
    let Some(FnArg::Receiver(receiver)) = modified_method
        .sig
        .inputs
        .iter_mut()
        .find(|a| matches!(a, FnArg::Receiver(_))) else {
            return Err(quote_spanned! {
                modified_method.span() =>
                compile_error!("Static method cannot be actified: the method requires a receiver to the impl type, using either &self or &mut self");
            });
        };
    receiver.mutability = None;
    let modified_inputs = &modified_method.sig.inputs;

    let name = &method.sig.ident;
    let result = match &method.sig.output {
        ReturnType::Default => {
            quote! {
                async fn #name(#modified_inputs) -> Result<(), ActorError>;
            }
        }
        ReturnType::Type(_, output_type) => {
            quote! {
               async fn #name(#modified_inputs) -> Result<#output_type, ActorError>;
            }
        }
    };

    Ok(result)
}

fn get_impl_type(impl_block: &ItemImpl) -> Result<String, proc_macro2::TokenStream> {
    match &*impl_block.self_ty {
        Type::Path(type_path) => {
            if let Some(last_segment) = type_path.path.segments.last() {
                return Ok(last_segment.ident.to_string()); // Take the last element from a path like crate:: or super::
            }
        }
        _ => {} // Do not allow any other types than regular structs
    }

    Err(quote_spanned! {
        impl_block.self_ty.span() =>
        compile_error!("The impl type should be a regular struct");
    })
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
