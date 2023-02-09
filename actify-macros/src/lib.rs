use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, quote_spanned};
use syn::{
    punctuated::Punctuated, spanned::Spanned, token::Comma, FnArg, Ident, ImplItem, ImplItemMethod,
    ItemImpl, ReturnType, TraitItemMethod, Type,
};

/// The actify macro expands an impl block of a rust struct to support usage in an actor model.
/// Effecyively, this macro allows to remotely call an actor method through a handle.
/// By using traits, the methods on the handle have the same signatures, so that type checking is enforced
#[proc_macro_attribute]
pub fn actify(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut impl_block = syn::parse_macro_input!(item as syn::ItemImpl);

    let result = match parse_macro(&mut impl_block) {
        Ok(parsed) => parsed,
        Err(error) => error,
    };

    // println!("{:?}", result.to_string());

    result.into()
}

/// This function consists of the main body of the macro parsing.
/// The body consists of two traits and its implementations:
/// The handle: code the user interacts with
/// The actor: code that executes the user-defined method in the actified impl block
fn parse_macro(
    impl_block: &mut ItemImpl,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let impl_type = get_impl_type(&impl_block)?;

    impl_block.generics.make_where_clause(); // Ensures the unwraps are safe

    // TODO what span to use for the new traits?
    let actor_trait_ident = syn::Ident::new(&format!("{}Actor", impl_type), Span::call_site());

    let handle_trait_ident = syn::Ident::new(&format!("{}Handle", impl_type), Span::call_site());

    let generated_methods = generate_all_methods(impl_block, &actor_trait_ident)?;

    let handle_trait = generate_handle_trait(&handle_trait_ident, &generated_methods)?;

    let handle_trait_impl =
        generate_handle_trait_impl(&impl_block, &handle_trait_ident, &generated_methods)?;

    let actor_trait = generate_actor_trait(&actor_trait_ident, &generated_methods)?;

    let actor_trait_impl =
        generate_actor_trait_impl(&impl_block, &actor_trait_ident, &generated_methods)?;

    let result = quote! {

        #handle_trait // Defines the custom function signatures that should be added to the handle

        #handle_trait_impl // Implement the function on the handle, and call the function on the actor

        #actor_trait // Defines the custom function wrappers that call the original methods on the actor

        #actor_trait_impl // Implement the function on the actor

        #impl_block // Extend the original functions
    };

    Ok(result)
}

/// A function that generates the implementation for the containter trait
fn generate_actor_trait_impl(
    impl_block: &ItemImpl,
    actor_trait: &Ident,
    methods: &Vec<GeneratedMethods>,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let methods = GeneratedMethods::get_actor_trait_impl_methods(methods);

    let impl_type = &impl_block.self_ty;
    let generics = &impl_block.generics;
    let where_clause = impl_block.generics.where_clause.as_ref().unwrap();
    let result = quote! {
        #[allow(unused_parens)]
        impl #generics #actor_trait for Actor<#impl_type> #where_clause
        {
            #methods
        }
    };

    Ok(result)
}

/// A function that generates the implementation for each method in the actor trait
fn generate_actor_trait_method_impl(
    impl_type: &Type,
    method: &TraitItemMethod,
    original_method: &ImplItemMethod,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    // A tuple of (names): (types) is generated so that the types can be cast & downcast to the Any type for sending to the actor
    let (input_arg_names, input_arg_types) = transform_args(&original_method.sig.inputs)?;

    let actor_method_ident = &method.sig.ident;
    let fn_ident = &original_method.sig.ident;
    let ReturnType::Type(_, original_output_type) = &original_method.sig.output else {panic!("Actify macro could not unwrap result output")};

    // The generated method impls downcast the sent arguments originating from the handle back to its original types
    // Then, the arguments are used to call the method on the inner type held by the actor.
    // Optionally, the new actor value is broadcasted to all subscribed listeners
    // Lastly, the result is boxed and sent back to the calling handle
    let result = quote! {
        fn #actor_method_ident(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError> {
            let (#input_arg_names): (#input_arg_types) = *args
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro");

            let result: #original_output_type = self
            .inner
            .as_mut()
            .ok_or(ActorError::NoValueSet(
                std::any::type_name::<#impl_type>().to_string(),
            ))?.
            #fn_ident(#input_arg_names);

        self.broadcast(); // TODO make this optional!

        Ok(Box::new(result))
        }
    };

    Ok(result)
}

/// This function creates a trait for the actor, derived from the impl type.
fn generate_actor_trait(
    actor_trait_ident: &Ident,
    methods: &Vec<GeneratedMethods>,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let methods = GeneratedMethods::get_actor_trait_methods(methods);

    let result = quote! {
        trait #actor_trait_ident
        {
            #methods
        }
    };

    Ok(result)
}

// This function takes a method from the original impl block and creates the actor variant that is remotely called by the handle.
// It is preceded by _ to mark the difference between the two methods
// Its signature must be standardized to the any type, to allow it being sent by the handle without using some kind of enum
fn generate_actor_trait_method(
    method: &ImplItemMethod,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let actor_method_ident = syn::Ident::new(
        &format!("_{}", &method.sig.ident.to_string()),
        Span::call_site(),
    );

    let result = quote! {
        fn #actor_method_ident(&mut self, args: Box<dyn Any + Send>) -> Result<Box<dyn Any + Send>, ActorError>;
    };

    Ok(result)
}

/// This function generates the handle trait implementation.
/// This trait generation at compile time allows to outfit handles with various kinds of methods, depending on its coupled actor.
fn generate_handle_trait_impl(
    impl_block: &ItemImpl,
    handle_trait: &Ident,
    methods: &Vec<GeneratedMethods>,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let methods = GeneratedMethods::get_handle_trait_impl_methods(methods);

    let impl_type = &impl_block.self_ty;
    let generics = &impl_block.generics;
    let where_clause = impl_block.generics.where_clause.as_ref().unwrap();

    let result = quote! {
        #[async_trait]
        impl #generics #handle_trait for Handle<#impl_type> #where_clause
        {
            #methods
        }
    };

    Ok(result)
}

/// The method implementation for each handle has the main job of:
/// 1. boxing the arguments in a tuple, so it can be send as any type
/// 2. sending a job to the actor with the appropriate method that needs to be executed
/// 3. downcasting the return value to the original type
fn generate_handle_trait_method_impl(
    method: &TraitItemMethod,
    actor_trait_ident: &Ident,
    actor_method: &TraitItemMethod,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let signature = &method.sig;
    let (input_arg_names, _) = transform_args(&method.sig.inputs)?;

    let actor_method_name = &actor_method.sig.ident;

    let result = quote! {
        #signature {
            let res = self
            .send_job(
                FnType::Inner(Box::new(#actor_trait_ident::#actor_method_name)),
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

/// This struct holds the derived methods from each original method.
/// The methods are collected purely for sanity
#[derive(Clone, Debug)]
struct GeneratedMethods {
    handle_trait: proc_macro2::TokenStream,
    handle_trait_impl: proc_macro2::TokenStream,
    actor_trait: proc_macro2::TokenStream,
    actor_trait_impl: proc_macro2::TokenStream,
}

impl GeneratedMethods {
    /// A utility function to multiple generated method structs to a single tokenstream
    fn get_handle_trait_methods(methods: &Vec<GeneratedMethods>) -> proc_macro2::TokenStream {
        let handle_trait_methods = methods.iter().map(|m| m.handle_trait.clone()).collect();
        GeneratedMethods::flatten_token_stream(handle_trait_methods)
    }

    /// A utility function to multiple generated method structs to a single tokenstream
    fn get_handle_trait_impl_methods(methods: &Vec<GeneratedMethods>) -> proc_macro2::TokenStream {
        let handle_trait_impl_methods = methods
            .iter()
            .map(|m| m.handle_trait_impl.clone())
            .collect();
        GeneratedMethods::flatten_token_stream(handle_trait_impl_methods)
    }

    /// A utility function to multiple generated method structs to a single tokenstream
    fn get_actor_trait_methods(methods: &Vec<GeneratedMethods>) -> proc_macro2::TokenStream {
        let actor_trait_methods = methods.iter().map(|m| m.actor_trait.clone()).collect();
        GeneratedMethods::flatten_token_stream(actor_trait_methods)
    }

    /// A utility function to multiple generated method structs to a single tokenstream
    fn get_actor_trait_impl_methods(methods: &Vec<GeneratedMethods>) -> proc_macro2::TokenStream {
        let actor_trait_impl_methods = methods.iter().map(|m| m.actor_trait_impl.clone()).collect();
        GeneratedMethods::flatten_token_stream(actor_trait_impl_methods)
    }

    /// A utility function that flattens for instance a vector of trait impl methods to a single token stream
    fn flatten_token_stream(
        token_streams: Vec<proc_macro2::TokenStream>,
    ) -> proc_macro2::TokenStream {
        let mut flattened_stream = proc_macro2::TokenStream::new();
        for stream in token_streams {
            flattened_stream.extend(stream)
        }
        flattened_stream
    }
}

// Generates and collects the derived methods of each original method in the impl block
fn generate_all_methods(
    impl_block: &ItemImpl,
    actor_trait_ident: &Ident,
) -> Result<Vec<GeneratedMethods>, proc_macro2::TokenStream> {
    let mut methods = vec![];
    for item in &impl_block.items {
        match item {
            ImplItem::Method(original_method) => methods.push(generate_methods(
                &impl_block.self_ty,
                original_method,
                actor_trait_ident,
            )?),
            _ => {}
        }
    }

    Ok(methods)
}

/// A function collecting the derived methods of a single original method from the impl block
fn generate_methods(
    impl_type: &Type,
    original_method: &ImplItemMethod,
    actor_trait_ident: &Ident,
) -> Result<GeneratedMethods, proc_macro2::TokenStream> {
    let actor_trait_signature = generate_actor_trait_method(original_method)?;
    let handle_trait_signature = generate_handle_trait_method(original_method)?;

    let parsed_actor_signature = syn::parse(actor_trait_signature.clone().into())
        .expect("Parsing the actor trait in the Actify macro failed");
    let parsed_handle_signature = syn::parse(handle_trait_signature.clone().into())
        .expect("Parsing the handle trait in the Actify macro failed");

    let actor_method_impl =
        generate_actor_trait_method_impl(impl_type, &parsed_actor_signature, original_method)?;
    let handle_method_impl = generate_handle_trait_method_impl(
        &parsed_handle_signature,
        actor_trait_ident,
        &parsed_actor_signature,
    )?;

    Ok(GeneratedMethods {
        handle_trait: handle_trait_signature,
        handle_trait_impl: handle_method_impl,
        actor_trait: actor_trait_signature,
        actor_trait_impl: actor_method_impl,
    })
}

/// This function creates a trait for the Handle, derived from the impl type.
fn generate_handle_trait(
    handle_trait_ident: &Ident,
    methods: &Vec<GeneratedMethods>,
) -> Result<proc_macro2::TokenStream, proc_macro2::TokenStream> {
    let methods = GeneratedMethods::get_handle_trait_methods(methods);

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

/// This function verifies the type of impl block that the macro is placed upon.
/// If the impl is not of the right format, an error is returned
/// If correct, the type is returned as a String for usage in the trait generation
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
