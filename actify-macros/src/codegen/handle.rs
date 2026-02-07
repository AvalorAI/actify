use crate::parse::{ImplInfo, MethodInfo};
use quote::quote;

/// Generate the public handle trait definition.
/// e.g. `pub trait TestStructHandle<T> where T: ... { async fn foo(&self, ...) -> ...; }`
pub fn generate_trait(info: &ImplInfo) -> proc_macro2::TokenStream {
    let impl_attrs = &info.attributes;
    let handle_trait_ident = &info.handle_trait_ident;
    let generics = &info.generics;
    let where_clause = &info.generics.where_clause;

    let methods = info.methods.iter().map(method_signature);

    quote! {
        #(#impl_attrs)*
        pub trait #handle_trait_ident #generics #where_clause
        {
            #(#methods)*
        }
    }
}

/// Generate the handle trait implementation for `Handle<T>`.
pub fn generate_trait_impl(info: &ImplInfo) -> proc_macro2::TokenStream {
    let impl_attrs = &info.attributes;
    let handle_trait_ident = &info.handle_trait_ident;
    let impl_type = &info.impl_type;
    let generics = &info.generics;
    let where_clause = &info.generics.where_clause;

    let methods = info.methods.iter().map(|m| method_body(info, m));

    quote! {
        #(#impl_attrs)*
        impl #generics #handle_trait_ident #generics for actify::Handle<#impl_type> #where_clause
        {
            #(#methods)*
        }
    }
}

/// Generate a handle trait method signature.
/// e.g. `async fn foo(&self, i: i32) -> f64;`
fn method_signature(method: &MethodInfo) -> proc_macro2::TokenStream {
    let attrs = &method.attributes;
    let ident = &method.ident;
    let arg_names: Vec<_> = method.arg_names.iter().collect();
    let arg_types: Vec<_> = method.arg_types.iter().collect();
    let method_generics = &method.method_generics;
    let where_clause = &method.method_generics.where_clause;
    let return_type = quote_return_type(&method.output_type);

    quote! {
        #(#attrs)*
        async fn #ident #method_generics(&self, #(#arg_names: #arg_types),*) #return_type #where_clause;
    }
}

/// Generate the handle trait method implementation body.
/// Boxes args, sends job to actor, downcasts result.
fn method_body(info: &ImplInfo, method: &MethodInfo) -> proc_macro2::TokenStream {
    // #[deprecated] is a hard error on trait impl methods, #[must_use] triggers
    // a "has no effect" warning. Both only belong on the trait definition.
    let attrs: Vec<_> = method
        .attributes
        .iter()
        .filter(|a| !a.path().is_ident("deprecated") && !a.path().is_ident("must_use"))
        .collect();
    let ident = &method.ident;
    let arg_names: Vec<_> = method.arg_names.iter().collect();
    let arg_types: Vec<_> = method.arg_types.iter().collect();
    let method_generics = &method.method_generics;
    let where_clause = &method.method_generics.where_clause;
    let impl_type = &info.impl_type;
    let actor_trait_ident = &info.actor_trait_ident;
    let actor_ident = &method.actor_ident;
    let generic_type_params = &method.generic_type_params;
    let return_type = quote_return_type(&method.output_type);

    quote! {
        #(#attrs)*
        async fn #ident #method_generics(&self, #(#arg_names: #arg_types),*) #return_type #where_clause {
            let res = self
            .send_job(
                Box::new(
                    |s: &mut actify::Actor<#impl_type>, args: Box<dyn std::any::Any + Send>|
                    Box::pin(async move { #actor_trait_ident::#actor_ident::<#(#generic_type_params),*>(s, args).await })),
                Box::new((#(#arg_names),*)),
            )
            .await;

            *res
            .downcast()
            .expect("Downcasting failed due to an error in the Actify macro")
        }
    }
}

/// Quote a return type, omitting the `->` for unit returns.
fn quote_return_type(ty: &syn::Type) -> proc_macro2::TokenStream {
    if let syn::Type::Tuple(tuple) = ty {
        if tuple.elems.is_empty() {
            return quote! {};
        }
    }
    quote! { -> #ty }
}
