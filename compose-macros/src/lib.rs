use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    parse_macro_input, FnArg, GenericParam, Generics, Ident, ItemFn, Pat, PatIdent, PatType,
    ReturnType, Type, TypeParamBound, WherePredicate,
};

fn path_is_fn_like(path: &syn::Path) -> bool {
    path.segments
        .last()
        .map(|segment| {
            let name = segment.ident.to_string();
            matches!(name.as_str(), "Fn" | "FnMut" | "FnOnce")
        })
        .unwrap_or(false)
}

fn bounds_contain_fn_trait<'a>(bounds: impl IntoIterator<Item = &'a TypeParamBound>) -> bool {
    bounds.into_iter().any(|bound| match bound {
        TypeParamBound::Trait(trait_bound) => path_is_fn_like(&trait_bound.path),
        _ => false,
    })
}

fn is_fn_trait(ty: &Type, generics: &Generics) -> bool {
    match ty {
        Type::BareFn(_) => true,
        Type::ImplTrait(impl_trait) => bounds_contain_fn_trait(&impl_trait.bounds),
        Type::TraitObject(trait_object) => bounds_contain_fn_trait(&trait_object.bounds),
        Type::Reference(reference) => is_fn_trait(&reference.elem, generics),
        Type::Group(group) => is_fn_trait(&group.elem, generics),
        Type::Paren(paren) => is_fn_trait(&paren.elem, generics),
        Type::Path(type_path) => {
            if path_is_fn_like(&type_path.path) {
                return true;
            }
            if let Some(ident) = type_path.path.get_ident() {
                for param in &generics.params {
                    if let GenericParam::Type(type_param) = param {
                        if type_param.ident == *ident && bounds_contain_fn_trait(&type_param.bounds)
                        {
                            return true;
                        }
                    }
                }
                if let Some(where_clause) = &generics.where_clause {
                    for predicate in &where_clause.predicates {
                        if let WherePredicate::Type(predicate_type) = predicate {
                            if let Type::Path(bounded_path) = &predicate_type.bounded_ty {
                                if bounded_path.path.get_ident() == Some(ident)
                                    && bounds_contain_fn_trait(&predicate_type.bounds)
                                {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            false
        }
        _ => false,
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

    struct ParamInfo {
        temp_ident: Ident,
        original_pat: Box<Pat>,
        ty: Type,
        pat_ident: Option<Ident>,
        is_fn: bool,
    }

    let mut param_info: Vec<ParamInfo> = Vec::new();

    for (index, arg) in func.sig.inputs.iter_mut().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let ident = Ident::new(&format!("__arg{}", index), Span::call_site());
            let original_pat: Box<Pat> = pat.clone();
            let pat_ident = match original_pat.as_ref() {
                Pat::Ident(PatIdent { ident, .. }) => Some(ident.clone()),
                _ => None,
            };
            *pat = Box::new(syn::parse_quote! { #ident });
            param_info.push(ParamInfo {
                temp_ident: ident,
                original_pat,
                ty: (*ty.clone()),
                pat_ident,
                is_fn: false,
            });
        }
    }

    let generics = func.sig.generics.clone();
    for info in &mut param_info {
        info.is_fn = is_fn_trait(&info.ty, &generics);
        if info.is_fn && info.pat_ident.is_none() {
            return syn::Error::new_spanned(
                info.original_pat.as_ref(),
                "closure parameters must use simple identifiers",
            )
            .to_compile_error()
            .into();
        }
    }

    let original_block = func.block.clone();
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };

    let rebinds: Vec<_> = param_info
        .iter()
        .map(|info| {
            let ident = &info.temp_ident;
            let pat = &info.original_pat;
            quote! { let #pat = #ident; }
        })
        .collect();

    let return_ty: syn::Type = match &func.sig.output {
        ReturnType::Default => syn::parse_quote! { () },
        ReturnType::Type(_, ty) => ty.as_ref().clone(),
    };

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
                let ident = &info.temp_ident;
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
                let ident = &info.temp_ident;
                let ty = &info.ty;
                if info.is_fn {
                    quote! {
                        let #ptr_ident: *mut compose_core::ClosureSlot<#ty> = {
                            let __state_ref = __composer
                                .remember(|| compose_core::ClosureSlot::<#ty>::default());
                            __state_ref as *mut compose_core::ClosureSlot<#ty>
                        };
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

        let recompose_args: Vec<TokenStream2> = param_state_ptrs
            .iter()
            .enumerate()
            .map(|(index, ptr_ident)| {
                let message = format!("composable parameter {} missing for recomposition", index);
                if param_info[index].is_fn {
                    quote! {
                        unsafe {
                            (&mut *#ptr_ident)
                                .take()
                                .expect(#message)
                        }
                    }
                } else {
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

        let store_closure_state: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .map(|(info, ptr_ident)| {
                if info.is_fn {
                    let pat_ident = info
                        .pat_ident
                        .as_ref()
                        .expect("checked above: closure params must be simple idents");
                    quote! {
                        unsafe {
                            (&mut *#ptr_ident).store(#pat_ident);
                        }
                    }
                } else {
                    quote! {}
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
            #(#store_closure_state)*
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
                let ident = &info.temp_ident;
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
                    #(#rebinds)*
                    #original_block
                })
            })
        });
        func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
        TokenStream::from(quote! { #func })
    }
}
