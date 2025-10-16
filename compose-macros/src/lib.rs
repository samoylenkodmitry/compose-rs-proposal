use std::collections::HashSet;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    parse_macro_input, FnArg, GenericParam, Generics, Ident, ItemFn, Pat, PatIdent, PatType,
    ReturnType, Type,
};

fn path_is_fn_trait(path: &syn::Path) -> bool {
    path.segments
        .last()
        .map(|seg| {
            let ident = seg.ident.to_string();
            ident == "Fn" || ident == "FnMut" || ident == "FnOnce"
        })
        .unwrap_or(false)
}

fn bound_is_fn_like(bound: &syn::TypeParamBound) -> bool {
    match bound {
        syn::TypeParamBound::Trait(trait_bound) => path_is_fn_trait(&trait_bound.path),
        _ => false,
    }
}

fn collect_fn_like_generics(generics: &Generics) -> HashSet<String> {
    let mut result = HashSet::new();
    for param in generics.params.iter() {
        if let GenericParam::Type(type_param) = param {
            if type_param.bounds.iter().any(bound_is_fn_like) {
                result.insert(type_param.ident.to_string());
            }
        }
    }
    if let Some(where_clause) = &generics.where_clause {
        for predicate in where_clause.predicates.iter() {
            if let syn::WherePredicate::Type(pred_type) = predicate {
                if let Type::Path(type_path) = &pred_type.bounded_ty {
                    if let Some(ident) = type_path.path.get_ident() {
                        if pred_type.bounds.iter().any(bound_is_fn_like) {
                            result.insert(ident.to_string());
                        }
                    }
                }
            }
        }
    }
    result
}

fn type_is_fn_like(ty: &Type, fn_like_generics: &HashSet<String>) -> bool {
    match ty {
        Type::BareFn(_) => true,
        Type::TraitObject(trait_obj) => trait_obj.bounds.iter().any(bound_is_fn_like),
        Type::ImplTrait(impl_trait) => impl_trait.bounds.iter().any(bound_is_fn_like),
        Type::Path(type_path) => {
            if path_is_fn_trait(&type_path.path) {
                return true;
            }
            if let Some(ident) = type_path.path.get_ident() {
                return fn_like_generics.contains(&ident.to_string());
            }
            false
        }
        Type::Reference(reference) => type_is_fn_like(&reference.elem, fn_like_generics),
        Type::Paren(paren) => type_is_fn_like(&paren.elem, fn_like_generics),
        Type::Group(group) => type_is_fn_like(&group.elem, fn_like_generics),
        _ => false,
    }
}

fn extract_binding_ident(pat: &Pat) -> Option<Ident> {
    match pat {
        Pat::Ident(PatIdent { ident, .. }) => Some(ident.clone()),
        _ => None,
    }
}

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
    let fn_like_generics = collect_fn_like_generics(&func.sig.generics);

    struct ParamInfo {
        ident: Ident,
        original_pat: Box<Pat>,
        ty: Type,
        is_fn_like: bool,
        binding_ident: Option<Ident>,
    }

    let mut param_info: Vec<ParamInfo> = Vec::new();

    for (index, arg) in func.sig.inputs.iter_mut().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let ident = Ident::new(&format!("__arg{}", index), Span::call_site());
            let original_pat: Box<Pat> = pat.clone();
            let ty_clone: Type = ty.as_ref().clone();
            let is_fn_like = type_is_fn_like(&ty_clone, &fn_like_generics);
            let binding_ident = extract_binding_ident(&original_pat);
            *pat = Box::new(syn::parse_quote! { #ident });
            param_info.push(ParamInfo {
                ident,
                original_pat,
                ty: ty_clone,
                is_fn_like,
                binding_ident,
            });
        }
    }

    let original_block = func.block.clone();
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };

    let rebinds_plain: Vec<_> = param_info
        .iter()
        .map(|info| {
            let ident = &info.ident;
            let pat = &info.original_pat;
            quote! { let #pat = #ident; }
        })
        .collect();

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
        .map(|info| {
            let ident = &info.ident;
            let ty = &info.ty;
            quote! { #ident: #ty }
        })
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
            .map(|info| {
                let ident = &info.ident;
                let ty = &info.ty;
                quote! { #ident: #ty }
            })
            .collect();

        let param_state_ptrs: Vec<Ident> = (0..param_info.len())
            .map(|index| Ident::new(&format!("__param_state_ptr{}", index), Span::call_site()))
            .collect();

        let param_setup: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .map(|(info, ptr_ident)| {
                let ident = &info.ident;
                let ty = &info.ty;
                if info.is_fn_like {
                    quote! {
                        let #ptr_ident: *mut compose_core::ParamSlot<#ty> = {
                            let __slot_ref = __composer
                                .remember(|| compose_core::ParamSlot::<#ty>::default());
                            __slot_ref as *mut compose_core::ParamSlot<#ty>
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
            .zip(param_state_ptrs.iter())
            .map(|(info, ptr_ident)| {
                let pat = &info.original_pat;
                if info.is_fn_like {
                    quote! {
                        let #pat = unsafe { (&mut *#ptr_ident).take() };
                    }
                } else {
                    let ident = &info.ident;
                    quote! { let #pat = #ident; }
                }
            })
            .collect();

        let store_back: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .filter_map(|(info, ptr_ident)| {
                if info.is_fn_like {
                    if let Some(binding_ident) = &info.binding_ident {
                        Some(quote! {
                            unsafe { (&mut *#ptr_ident).set(#binding_ident); }
                        })
                    } else {
                        Some(quote! {
                            compile_error!("function-like parameters must bind to an identifier");
                        })
                    }
                } else {
                    None
                }
            })
            .collect();

        let recompose_args: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .enumerate()
            .map(|(index, (info, ptr_ident))| {
                if info.is_fn_like {
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
            #(#store_back)*
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
            .map(|info| {
                let ident = &info.ident;
                quote! { #ident }
            })
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
        let wrapped = quote!({
            compose_core::with_current_composer(|__composer: &mut compose_core::Composer<'_>| {
                __composer.with_group(#key_expr, |__scope: &mut compose_core::Composer<'_>| {
                    #(#rebinds_plain)*
                    #original_block
                })
            })
        });
        func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
        TokenStream::from(quote! { #func })
    }
}
