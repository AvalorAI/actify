use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, quote_spanned};
use syn::{
    punctuated::Punctuated, spanned::Spanned, token::Comma, FnArg, Ident, ImplItem, ImplItemMethod,
    ItemImpl, ItemTrait, ReturnType, TraitItem, TraitItemMethod, Type,
};

// TODO what happens with generics inside the struct impl including lifetimes?

#[proc_macro_attribute]
pub fn actify(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let impl_block = syn::parse_macro_input!(item as syn::ItemImpl);

    let result = match parse_macro(&impl_block) {
        Ok(parsed) => parsed,
        Err(error) => error,
    };

    // println!("{:?}", result.to_string());

    result.into()
}

fn parse_macro(
    impl_block: &ItemImpl,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let impl_type = get_impl_type(&impl_block)?;
    let handle_trait_ident = syn::Ident::new(&format!("{}Handle", impl_type), Span::call_site());

    let container_trait_ident =
        syn::Ident::new(&format!("{}Container", impl_type), Span::call_site());

    let handle_trait = generate_handle_trait(&impl_block, &handle_trait_ident)?;
    let handle_trait_parsed = syn::parse(handle_trait.clone().into())
        .expect("Parsing the handle trait in the Actify macro failed");

    let handle_trait_impl =
        generate_handle_trait_impl(&impl_block, &container_trait_ident, &handle_trait_parsed)?;

    let container_trait = generate_container_trait(&impl_block, &container_trait_ident)?;
    let container_trait_parsed = syn::parse(container_trait.clone().into())
        .expect("Parsing the container trait in the Actify macro failed");

    let container_trait_impl = generate_container_trait_impl(&impl_block, &container_trait_parsed)?;

    let result = quote! {

        #handle_trait // Defines the custom function signatures that should be added to the handle

        #handle_trait_impl // Implement the function on the handle, and call the function on the container

        #container_trait // Defines the custom function wrappers that call the original methods on the actor

        #container_trait_impl // Implement the function on the container

        #impl_block // Extend the original functions
    };

    Ok(result)
}

fn generate_container_trait_impl(
    impl_block: &ItemImpl,
    container_trait: &ItemTrait,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let impl_type = &impl_block.self_ty;

    let mut methods = proc_macro2::TokenStream::new();

    let mut index = 0; // TODO this counting is stupid. Refactor to generate all method at once, instead of per trait / impl
    for item in &container_trait.items {
        match item {
            TraitItem::Method(method) => {
                let original_item = &impl_block.items[index];
                if let ImplItem::Method(original_method) = original_item {
                    methods.extend(generate_container_trait_method_impl(
                        method,
                        original_method,
                    )?)
                }
            }
            _ => {}
        }
        index += 1
    }

    let trait_name = &container_trait.ident;
    let result = quote! {
        #[allow(unused_parens)]
        impl #trait_name for Container<#impl_type>
        {
            #methods
        }
    };

    Ok(result)
}

fn generate_container_trait_method_impl(
    method: &TraitItemMethod,
    original_method: &ImplItemMethod,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let (input_arg_names, input_arg_types) = transform_args(&original_method.sig.inputs)?;

    let container_method_ident = &method.sig.ident;

    let fn_ident = &original_method.sig.ident;

    let ReturnType::Type(_, original_output_type) = &original_method.sig.output else {panic!("Actify macro could not unwrap result output")};

    let result = quote! {
        fn #container_method_ident(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
            let (#input_arg_names): (#input_arg_types) = *args
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro");

            let result: #original_output_type = self
            .inner
            .as_mut()
            .ok_or(ActorError::NoValueSet(
                std::any::type_name::<MyActor>().to_string(),
            ))?.
            #fn_ident(#input_arg_names);

        self.broadcast(); // TODO make this optional!

        Ok(Box::new(result))
        }
    };

    Ok(result)
}

// fn unwrap_result_output(output: &ReturnType) -> Option<Type> {
//     let ReturnType::Type(_, wrapped_output)  = output else {return None};

//     let Type::Path(type_path) = &**wrapped_output else { return None};

//     let PathArguments::AngleBracketed(inner_args) = &type_path.path.segments[0].arguments else {
//         return None;
//     };

//     let GenericArgument::Type(original_type) = &inner_args.args[0] else {return None};

//     Some(original_type.clone())
// }

/// This function creates a trait for the Container, derived from the impl type.
/// It modifies all method in the impl block and adds them to the trait.
fn generate_container_trait(
    impl_block: &ItemImpl,
    container_trait_ident: &Ident,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let mut methods = proc_macro2::TokenStream::new();
    for item in &impl_block.items {
        match item {
            ImplItem::Method(method) => methods.extend(generate_container_trait_method(method)?),
            _ => {}
        }
    }

    let result = quote! {

        trait #container_trait_ident
        {
            #methods
        }
    };

    Ok(result)
}

// All container
fn generate_container_trait_method(
    method: &ImplItemMethod,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let container_method_ident = syn::Ident::new(
        &format!("_{}", &method.sig.ident.to_string()),
        Span::call_site(),
    );

    let result = quote! {
        fn #container_method_ident(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;
    };

    Ok(result)
}

fn generate_handle_trait_impl(
    impl_block: &ItemImpl,
    container_trait_ident: &Ident,
    handle_trait: &ItemTrait,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let impl_type = &impl_block.self_ty;

    let mut methods = proc_macro2::TokenStream::new();
    for item in &handle_trait.items {
        match item {
            TraitItem::Method(method) => methods.extend(generate_handle_trait_method_impl(
                method,
                container_trait_ident,
            )?),
            _ => {}
        }
    }

    let trait_name = &handle_trait.ident;
    let result = quote! {
        #[async_trait]
        impl #trait_name for Handle<#impl_type>
        where
            #impl_type: Clone,
        {
            #methods
        }
    };

    Ok(result)
}

fn generate_handle_trait_method_impl(
    method: &TraitItemMethod,
    container_trait_ident: &Ident,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let signature = &method.sig;
    let (input_arg_names, _) = transform_args(&method.sig.inputs)?;

    // TODO what does this call_site do? Is it then equal to the actual container method?
    let container_method_name = syn::Ident::new(
        &format!("_{}", &method.sig.ident.to_string()),
        Span::call_site(),
    );

    let result = quote! {
        #signature {
            let res = self
            .send_job(
                FnType::Inner(Box::new(#container_trait_ident::#container_method_name)),
                Box::new((#input_arg_names)),
            )
            .await?;

            Ok(*res
                .downcast()
                .expect("Downcasting failed due to an error in the Actify macro"))
        }
    };

    Ok(result)
}

/// This function creates a trait for the Handle, derived from the impl type.
/// It modifies all method in the impl block and adds them to the trait.
fn generate_handle_trait(
    impl_block: &ItemImpl,
    handle_trait_ident: &Ident,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let mut methods = proc_macro2::TokenStream::new();
    for item in &impl_block.items {
        match item {
            ImplItem::Method(method) => methods.extend(generate_handle_trait_method(method)?),
            _ => {}
        }
    }

    let result = quote! {
        #[async_trait::async_trait]
        pub trait #handle_trait_ident
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
        .find(|arg| matches!(arg, FnArg::Receiver(_))) else {
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

/// This function collects the input arguments    
/// 1. Filter the args, so that only typed arguments remain
/// 2. Check if the ident does not contain a reference
/// 3. Extract the ident (in case the pattern type is an owned ident)
/// TODO is allowing only the Ident pattern to prohibitive?
fn transform_args(
    args: &Punctuated<FnArg, Comma>,
) -> Result<(Punctuated<Ident, Comma>, Punctuated<Ident, Comma>), proc_macro2::TokenStream> {
    // Add all idents to a Punctuated => param1, param2, ...
    let mut input_arg_names: Punctuated<syn::Ident, Comma> = Punctuated::new();
    let mut input_arg_types: Punctuated<syn::Ident, Comma> = Punctuated::new();

    for arg in args {
        if let syn::FnArg::Typed(pat_type) = arg {
            if let syn::Pat::Ident(pat_ident) = *pat_type.pat.clone() {
                match &*pat_type.ty {
                    Type::Reference(_) => {
                        return Err(quote_spanned! {
                            pat_type.ty.span() =>
                            compile_error!("Input arguments of actor model methods must be owned types and not referenced");
                        })
                    }
                    Type::Path(type_path) => {
                        let var_type = type_path
                            .path
                            .segments
                            .last()
                            .expect("Actify macro expected a valid type");
                        input_arg_names.push(pat_ident.ident);
                        input_arg_types.push(var_type.ident.clone());
                    }
                    _ => panic!(
                        "Actify macro cannot yet handle the type: {:?}",
                        *pat_type.ty
                    ),
                }
            }
        }
    }

    Ok((input_arg_names, input_arg_types))
}
