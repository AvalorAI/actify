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

/// Generate the handle trait implementation for `Handle<T, V>`.
///
/// Adds an unconstrained `__V` type parameter so that the generated trait
/// implementation works for all broadcast types, not just `Handle<T, T>`.
pub fn generate_trait_impl(info: &ImplInfo) -> proc_macro2::TokenStream {
    let impl_attrs = &info.attributes;
    let handle_trait_ident = &info.handle_trait_ident;
    let impl_type = &info.impl_type;
    let trait_generics = &info.generics;
    let where_clause = &info.generics.where_clause;

    let mut impl_generics = info.generics.clone();
    impl_generics.params.push(syn::parse_quote!(__V));

    let methods = info.methods.iter().map(|m| method_body(info, m));

    quote! {
        #(#impl_attrs)*
        #[allow(unused_parens)]
        impl #impl_generics #handle_trait_ident #trait_generics for actify::Handle<#impl_type, __V> #where_clause
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
/// Boxes args, sends job to actor, the actor downcasts args, calls the original
/// method, optionally broadcasts, and boxes the result.
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
    let output_type = &method.output_type;
    let return_type = quote_return_type(output_type);

    let call_prefix = build_call_prefix(info);
    let ident_string = format!("{}::{}", info.type_ident, ident);

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
        Some(quote! { s.broadcast(#ident_string); })
    };

    quote! {
        #(#attrs)*
        async fn #ident #method_generics(&self, #(#arg_names: #arg_types),*) #return_type #where_clause {
            let res = self
                .send_job(
                    Box::new(
                        |s: &mut actify::Actor<#impl_type>, args: Box<dyn std::any::Any + Send>|
                        Box::pin(async move {
                            let (#(#arg_names),*): (#(#arg_types),*) = *args
                                .downcast()
                                .expect("Downcasting failed due to an error in the Actify macro");

                            let result: #output_type = #call_prefix::#ident(&#mutability s.inner, #(#arg_names),*)#awaiter;

                            #broadcast

                            Box::new(result) as Box<dyn std::any::Any + Send>
                        })),
                    Box::new((#(#arg_names),*)),
                )
                .await;

            *res
                .downcast()
                .expect("Downcasting failed due to an error in the Actify macro")
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

/// Quote a return type, omitting the `->` for unit returns.
fn quote_return_type(ty: &syn::Type) -> proc_macro2::TokenStream {
    if let syn::Type::Tuple(tuple) = ty {
        if tuple.elems.is_empty() {
            return quote! {};
        }
    }
    quote! { -> #ty }
}
