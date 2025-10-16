use std::collections::HashSet;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    parse_macro_input, Error, FnArg, GenericParam, Ident, ItemFn, Pat, PatType, ReturnType, Type,
    TypeParamBound, WherePredicate,
};

#[proc_macro_attribute]
pub fn composable(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_tokens = TokenStream2::from(attr);
    let mut enable_skip = true;
    if !attr_tokens.is_empty() {
        match syn::parse2::<Ident>(attr_tokens) {
            Ok(ident) if ident == "no_skip" => enable_skip = false,
            Ok(other) => {
                return syn::Error::new_spanned(other, "unsupported composable attribute")
                    .to_compile_error()
                    .into();
            }
            Err(err) => {
                return err.to_compile_error().into();
            }
        }
    }

    let mut func = parse_macro_input!(item as ItemFn);
    let mut param_info = Vec::new();

    for (index, arg) in func.sig.inputs.iter_mut().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let ident = Ident::new(&format!("__arg{}", index), Span::call_site());
            let original_pat: Box<Pat> = pat.clone();
            *pat = Box::new(syn::parse_quote! { #ident });
            param_info.push((ident, original_pat, (*ty).clone()));
        }
    }

    let original_block = func.block.clone();
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };

    let fn_type_params = collect_fn_type_params(&func.sig.generics);
    let fn_like_flags: Vec<bool> = param_info
        .iter()
        .map(|(_, _, ty)| is_fn_like_type(ty, &fn_type_params))
        .collect();
    let binding_idents: Vec<Option<Ident>> = param_info
        .iter()
        .map(|(_, pat, _)| binding_ident(pat.as_ref()))
        .collect();

    for (((_, pat, _), is_fn_like), binding_ident) in param_info
        .iter()
        .zip(fn_like_flags.iter())
        .zip(binding_idents.iter())
    {
        if *is_fn_like && binding_ident.is_none() {
            let err = Error::new_spanned(
                pat,
                "function-like composable parameters must bind to an identifier",
            );
            return err.to_compile_error().into();
        }
    }

    let return_ty: syn::Type = match &func.sig.output {
        ReturnType::Default => syn::parse_quote! { () },
        ReturnType::Type(_, ty) => ty.as_ref().clone(),
    };
    let _helper_ident = Ident::new(
        &format!("__compose_impl_{}", func.sig.ident),
        Span::call_site(),
    );
    let generics = func.sig.generics.clone();
    let (_impl_generics, _ty_generics, _where_clause) = generics.split_for_impl();

    let _helper_inputs: Vec<TokenStream2> = param_info
        .iter()
        .map(|(ident, _pat, ty)| quote! { #ident: #ty })
        .collect();

    if enable_skip {
        let helper_ident = Ident::new(
            &format!("__compose_impl_{}", func.sig.ident),
            Span::call_site(),
        );
        let generics = func.sig.generics.clone();
        let (impl_generics, _ty_generics, where_clause) = generics.split_for_impl();

        let helper_inputs: Vec<TokenStream2> = param_info
            .iter()
            .map(|(ident, _pat, ty)| quote! { #ident: #ty })
            .collect();

        let param_ptrs: Vec<Ident> = (0..param_info.len())
            .map(|index| Ident::new(&format!("__param_ptr{}", index), Span::call_site()))
            .collect();

        let param_setup: Vec<TokenStream2> = param_info
            .iter()
            .zip(fn_like_flags.iter())
            .zip(param_ptrs.iter())
            .map(|(((ident, _pat, ty), is_fn_like), ptr_ident)| {
                if *is_fn_like {
                    quote! {
                        let #ptr_ident: *mut compose_core::ParamSlot<#ty> = {
                            let __state_ref = __composer
                                .remember(|| compose_core::ParamSlot::<#ty>::default());
                            __state_ref as *mut compose_core::ParamSlot<#ty>
                        };
                        unsafe { (&mut *#ptr_ident).set(#ident); }
                        __changed = true;
                    }
                } else {
                    quote! {
                        let #ptr_ident: *mut compose_core::ParamState<#ty> = {
                            let __state_ref = __composer
                                .remember(|| compose_core::ParamState::<#ty>::default());
                            __state_ref as *mut compose_core::ParamState<#ty>
                        };
                        if unsafe { (&mut *#ptr_ident).update(&#ident) } {
                            __changed = true;
                        }
                    }
                }
            })
            .collect();

        let rebinds: Vec<TokenStream2> = param_info
            .iter()
            .zip(fn_like_flags.iter())
            .zip(param_ptrs.iter())
            .map(|(((ident, pat, _), is_fn_like), ptr_ident)| {
                if *is_fn_like {
                    quote! { let #pat = unsafe { (&mut *#ptr_ident).take() }; }
                } else {
                    quote! { let #pat = #ident; }
                }
            })
            .collect();

        let param_store_back: Vec<TokenStream2> = param_info
            .iter()
            .zip(fn_like_flags.iter())
            .zip(binding_idents.iter())
            .zip(param_ptrs.iter())
            .map(
                |((((_ident, pat, _ty), is_fn_like), binding_ident), ptr_ident)| {
                    if *is_fn_like {
                        let binding_ident = binding_ident
                            .as_ref()
                            .expect("validated function-like parameter binding");
                        quote! {
                            unsafe {
                                (&mut *#ptr_ident).set(#binding_ident);
                            }
                        }
                    } else {
                        let _ = pat; // suppress unused warning
                        quote! {}
                    }
                },
            )
            .collect();

        let recompose_args: Vec<TokenStream2> = param_ptrs
            .iter()
            .enumerate()
            .zip(fn_like_flags.iter())
            .map(|((index, ptr_ident), is_fn_like)| {
                if *is_fn_like {
                    quote! {
                        unsafe { (&mut *#ptr_ident).take() }
                    }
                } else {
                    let message =
                        format!("composable parameter {} missing for recomposition", index);
                    quote! {
                        unsafe {
                            (&*#ptr_ident)
                                .value()
                                .expect(#message)
                        }
                    }
                }
            })
            .collect();

        let helper_body = quote! {
            let __current_scope = __composer
                .current_recompose_scope()
                .expect("missing recompose scope");
            let mut __changed = __current_scope.should_recompose();
            #(#param_setup)*
            let __result_slot_ptr: *mut compose_core::ReturnSlot<#return_ty> = {
                let __slot_ref = __composer
                    .remember(|| compose_core::ReturnSlot::<#return_ty>::default());
                __slot_ref as *mut compose_core::ReturnSlot<#return_ty>
            };
            let __has_previous = unsafe { (&*__result_slot_ptr).get().is_some() };
            if !__changed && __has_previous {
                __composer.skip_current_group();
                let __result = unsafe {
                    (&*__result_slot_ptr)
                        .get()
                        .expect("composable return value missing during skip")
                };
                return __result;
            }
            #(#rebinds)*
            let __value: #return_ty = { #original_block };
            #(#param_store_back)*
            unsafe {
                (*__result_slot_ptr).store(__value.clone());
            }
            {
                let __impl_fn = #helper_ident;
                __composer.set_recompose_callback(move |
                    __composer: &mut compose_core::Composer<'_>|
                {
                    __impl_fn(
                        __composer
                        #(, #recompose_args)*
                    );
                });
            }
            __value
        };

        let helper_fn = quote! {
            #[allow(non_snake_case)]
            fn #helper_ident #impl_generics (
                __composer: &mut compose_core::Composer<'_>
                #(, #helper_inputs)*
            ) -> #return_ty #where_clause {
                #helper_body
            }
        };

        let wrapper_args: Vec<TokenStream2> = param_info
            .iter()
            .map(|(ident, _pat, _)| quote! { #ident })
            .collect();

        let wrapped = quote!({
            compose_core::with_current_composer(|__composer: &mut compose_core::Composer<'_>| {
                __composer.with_group(#key_expr, |__composer: &mut compose_core::Composer<'_>| {
                    #helper_ident(__composer #(, #wrapper_args)*)
                })
            })
        });
        func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
        TokenStream::from(quote! {
            #helper_fn
            #func
        })
    } else {
        let rebinds: Vec<TokenStream2> = param_info
            .iter()
            .map(|(ident, pat, _)| {
                quote! { let #pat = #ident; }
            })
            .collect();
        let wrapped = quote!({
            compose_core::with_current_composer(|__composer: &mut compose_core::Composer<'_>| {
                __composer.with_group(#key_expr, |__scope: &mut compose_core::Composer<'_>| {
                    #(#rebinds)*
                    #original_block
                })
            })
        });
        func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
        TokenStream::from(quote! { #func })
    }
}

fn collect_fn_type_params(generics: &syn::Generics) -> HashSet<String> {
    let mut fn_params = HashSet::new();
    for param in generics.params.iter() {
        if let GenericParam::Type(ty_param) = param {
            if ty_param.bounds.iter().any(is_fn_trait_bound) {
                fn_params.insert(ty_param.ident.to_string());
            }
        }
    }
    if let Some(where_clause) = &generics.where_clause {
        for predicate in where_clause.predicates.iter() {
            if let WherePredicate::Type(pred) = predicate {
                if let Type::Path(path) = &pred.bounded_ty {
                    if let Some(ident) = path.path.get_ident() {
                        if pred.bounds.iter().any(is_fn_trait_bound) {
                            fn_params.insert(ident.to_string());
                        }
                    }
                }
            }
        }
    }
    fn_params
}

fn is_fn_like_type(ty: &Type, fn_type_params: &HashSet<String>) -> bool {
    match ty {
        Type::BareFn(_) => true,
        Type::ImplTrait(impl_trait) => impl_trait.bounds.iter().any(is_fn_trait_bound),
        Type::TraitObject(obj) => obj.bounds.iter().any(is_fn_trait_bound),
        Type::Path(path) => path
            .path
            .get_ident()
            .map(|ident| fn_type_params.contains(&ident.to_string()))
            .unwrap_or(false),
        Type::Reference(reference) => is_fn_like_type(&reference.elem, fn_type_params),
        Type::Paren(paren) => is_fn_like_type(&paren.elem, fn_type_params),
        Type::Group(group) => is_fn_like_type(&group.elem, fn_type_params),
        _ => false,
    }
}

fn is_fn_trait_bound(bound: &TypeParamBound) -> bool {
    match bound {
        TypeParamBound::Trait(trait_bound) => trait_bound
            .path
            .segments
            .last()
            .map(|segment| {
                let ident = segment.ident.to_string();
                ident == "Fn" || ident == "FnMut" || ident == "FnOnce"
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn binding_ident(pat: &Pat) -> Option<Ident> {
    match pat {
        Pat::Ident(pat_ident) => Some(pat_ident.ident.clone()),
        _ => None,
    }
}
