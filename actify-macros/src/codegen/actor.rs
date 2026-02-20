use crate::parse::{self, ImplInfo, MethodInfo};
use quote::quote;

/// Generate the actor trait definition (private).
pub fn generate_trait(info: &ImplInfo) -> proc_macro2::TokenStream {
    let impl_attrs = parse::filter_codegen_attrs(&info.attributes);
    let actor_trait_ident = &info.actor_trait_ident;

    let methods = info.methods.iter().map(method_signature);

    quote! {
        #(#impl_attrs)*
        trait #actor_trait_ident
        {
            #(#methods)*
        }
    }
}

/// Generate the actor trait implementation for `Actor<T>`.
pub fn generate_trait_impl(info: &ImplInfo) -> proc_macro2::TokenStream {
    let impl_attrs = parse::filter_codegen_attrs(&info.attributes);
    let actor_trait_ident = &info.actor_trait_ident;
    let impl_type = &info.impl_type;
    let generics = &info.generics;
    let where_clause = &info.generics.where_clause;

    // Compute the fully qualified call prefix once for all methods
    let call_prefix = build_call_prefix(info);

    let methods = info.methods.iter().map(|m| method_body(info, m, &call_prefix));

    quote! {
        #(#impl_attrs)*
        #[allow(unused_parens)]
        impl #generics #actor_trait_ident for actify::Actor<#impl_type> #where_clause
        {
            #(#methods)*
        }
    }
}

/// Generate an actor trait method signature.
/// e.g. `async fn _foo(&mut self, args: Box<dyn Any + Send>) -> Box<dyn Any + Send>;`
fn method_signature(method: &MethodInfo) -> proc_macro2::TokenStream {
    let attrs = parse::filter_codegen_attrs(&method.attributes);
    let actor_ident = &method.actor_ident;
    let method_generics = &method.method_generics;
    let where_clause = &method.method_generics.where_clause;

    quote! {
        #(#attrs)*
        async fn #actor_ident #method_generics(&mut self, args: Box<dyn std::any::Any + Send>) -> Box<dyn std::any::Any + Send> #where_clause;
    }
}

/// Generate the actor trait method implementation body.
/// Downcasts args, calls original method, optionally broadcasts, boxes result.
fn method_body(
    info: &ImplInfo,
    method: &MethodInfo,
    call_prefix: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let attrs = parse::filter_codegen_attrs(&method.attributes);
    let actor_ident = &method.actor_ident;
    let method_generics = &method.method_generics;
    let where_clause = &method.method_generics.where_clause;
    let arg_names = &method.arg_names;
    let arg_types = &method.arg_types;
    let output_type = &method.output_type;
    let fn_ident = &method.ident;

    let ident_string = format!("{}::{}", info.type_ident, fn_ident);

    let awaiter = if method.is_async {
        Some(quote! { .await })
    } else {
        None
    };

    let mutability = if method.is_mutable {
        Some(quote! { mut })
    } else {
        None
    };

    let broadcast = if method.skip_broadcast {
        None
    } else {
        Some(quote! { self.broadcast(#ident_string); })
    };

    quote! {
        #(#attrs)*
        async fn #actor_ident #method_generics(&mut self, args: Box<dyn std::any::Any + Send>) -> Box<dyn std::any::Any + Send> #where_clause {
            let (#arg_names): (#arg_types) = *args
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro");

            let result: #output_type = #call_prefix::#fn_ident(&#mutability self.inner, #arg_names)#awaiter;

        #broadcast

        Box::new(result)
        }
    }
}

/// Build the fully qualified syntax prefix for calling the original method.
/// This is the same for every method in the impl block:
/// - Direct impl, no generics: `TypeName`
/// - Direct impl, with generics: `TypeName::<T>`
/// - Trait impl: `<Type as Trait>`
fn build_call_prefix(info: &ImplInfo) -> proc_macro2::TokenStream {
    let type_ident = &info.type_ident;

    match &info.trait_path {
        None => {
            if info.generics.params.is_empty() {
                quote! { #type_ident }
            } else {
                let generics = &info.generics;
                quote! { #type_ident::#generics }
            }
        }
        Some(path) => {
            let impl_type = &info.impl_type;
            quote! { <#impl_type as #path> }
        }
    }
}
