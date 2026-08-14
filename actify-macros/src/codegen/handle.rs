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

    // Bounds and `const` declarations belong only in the impl's parameter list.
    // The trait reference takes the parameter names alone, which is what
    // TypeGenerics renders; the full Generics would emit `T: Clone` there.
    let (_, trait_generics, where_clause) = info.generics.split_for_impl();

    // The trait promises the returned future is Send, and the future holds
    // `&Handle<T, __V>`, which is Send only if the handle is Sync. Every handle
    // that can exist already satisfies this: `Handle::new` requires it of the
    // broadcast type.
    let mut generics_with_broadcast = info.generics.clone();
    generics_with_broadcast
        .params
        .push(syn::parse_quote!(__V: Send + Sync + 'static));
    let (impl_generics, _, _) = generics_with_broadcast.split_for_impl();

    let call_prefix = build_call_prefix(info);
    let methods = info
        .methods
        .iter()
        .map(|m| method_body(m, &call_prefix, info));

    quote! {
        #(#impl_attrs)*
        #[allow(unused_parens)]
        impl #impl_generics #handle_trait_ident #trait_generics for ::actify::Handle<#impl_type, __V> #where_clause
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
    // A trait method written `async fn` returns a future the caller knows
    // nothing about, so code generic over this trait cannot spawn the call. The
    // desugared form promises `Send`, which is what makes that possible.
    let output_type = &method.output_type;

    quote! {
        #(#attrs)*
        fn #ident #method_generics(&self, #(#arg_names: #arg_types),*)
            -> impl ::std::future::Future<Output = #output_type> + Send
        #where_clause;
    }
}

/// Generate the handle trait method implementation body.
/// Boxes args, sends job to actor, the actor downcasts args, calls the original
/// method, optionally broadcasts, and boxes the result.
fn method_body(
    method: &MethodInfo,
    call_prefix: &proc_macro2::TokenStream,
    info: &ImplInfo,
) -> proc_macro2::TokenStream {
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

    let broadcast = if method.broadcasts {
        Some(quote! { __actify_s.broadcast(#ident_string); })
    } else {
        None
    };

    // The __actify_ prefix prevents collisions with user argument names, which
    // are bound as-is inside the generated body (e.g. an argument named `s`).
    quote! {
        #(#attrs)*
        async fn #ident #method_generics(&self, #(#arg_names: #arg_types),*) #return_type #where_clause {
            let __actify_res = self.__send_job(
                ::std::boxed::Box::new(|__actify_s: &mut ::actify::__private::Actor<#impl_type>, __actify_args: ::std::boxed::Box<dyn ::std::any::Any + Send>|
                ::std::boxed::Box::pin(async move {
                    let (#(#arg_names),*): (#(#arg_types),*) = *__actify_args
                        .downcast()
                        .expect("Downcasting failed due to an error in the Actify macro");

                    let __actify_result: #output_type = #call_prefix::#ident(&#mutability __actify_s.inner, #(#arg_names),*)#awaiter;

                    #broadcast

                    ::std::boxed::Box::new(__actify_result) as ::std::boxed::Box<dyn ::std::any::Any + Send>
                })),
                ::std::boxed::Box::new((#(#arg_names),*)),
            ).await;

            *__actify_res.downcast().expect("Downcasting failed due to an error in the Actify macro")
        }
    }
}

/// Build the fully qualified syntax prefix for calling the original method.
/// This is the same for every method in the impl block:
/// - Direct impl: `<Type>`
/// - Trait impl: `<Type as Trait>`
///
/// The self type is used exactly as written, so `impl<T> Wrapper<Vec<T>>` calls
/// `<Wrapper<Vec<T>>>::method`. Rebuilding it from the impl block's parameter
/// list would instead produce `Wrapper::<T>`, which names a different type.
fn build_call_prefix(info: &ImplInfo) -> proc_macro2::TokenStream {
    let impl_type = &info.impl_type;

    match &info.trait_path {
        None => quote! { <#impl_type> },
        Some(path) => quote! { <#impl_type as #path> },
    }
}

/// Quote a return type, omitting the `->` for unit returns.
fn quote_return_type(ty: &syn::Type) -> proc_macro2::TokenStream {
    match ty {
        syn::Type::Tuple(tuple) if tuple.elems.is_empty() => quote! {},
        _ => quote! { -> #ty },
    }
}
