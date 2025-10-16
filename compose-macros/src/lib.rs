use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, FnArg, Ident, ItemFn, Pat, PatType, ReturnType, Type};

/// Check if a type is Fn-like (impl FnMut/Fn/FnOnce, Box<dyn FnMut>, generic with Fn bound, etc.)
/// For generic type parameters (e.g., `F` where F: FnMut()), we need to check the bounds.
fn is_fn_like_type(ty: &Type) -> bool {
    match ty {
        // impl FnMut(...) + 'static, impl Fn(...), etc.
        Type::ImplTrait(impl_trait) => {
            impl_trait.bounds.iter().any(|bound| {
                if let syn::TypeParamBound::Trait(trait_bound) = bound {
                    let path = &trait_bound.path;
                    if let Some(segment) = path.segments.last() {
                        let ident_str = segment.ident.to_string();
                        return ident_str == "FnMut" || ident_str == "Fn" || ident_str == "FnOnce";
                    }
                }
                false
            })
        }
        // Box<dyn FnMut(...)>
        Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last() {
                if segment.ident == "Box" {
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        if let Some(syn::GenericArgument::Type(Type::TraitObject(trait_obj))) =
                            args.args.first()
                        {
                            return trait_obj.bounds.iter().any(|bound| {
                                if let syn::TypeParamBound::Trait(trait_bound) = bound {
                                    let path = &trait_bound.path;
                                    if let Some(segment) = path.segments.last() {
                                        let ident_str = segment.ident.to_string();
                                        return ident_str == "FnMut"
                                            || ident_str == "Fn"
                                            || ident_str == "FnOnce";
                                    }
                                }
                                false
                            });
                        }
                    }
                }
            }
            false
        }
        // bare fn(...) -> ...
        Type::BareFn(_) => true,
        _ => false,
    }
}

/// Check if a generic type parameter has Fn-like bounds by looking at the where clause and bounds
fn is_generic_fn_like(ty: &Type, generics: &syn::Generics) -> bool {
    // Extract the ident for Type::Path that might be a generic param
    let type_ident = match ty {
        Type::Path(type_path) if type_path.path.segments.len() == 1 => {
            &type_path.path.segments[0].ident
        }
        _ => return false,
    };

    // Check if it's a type parameter with Fn bounds
    for param in &generics.params {
        if let syn::GenericParam::Type(type_param) = param {
            if type_param.ident == *type_ident {
                // Check the bounds on the type parameter
                for bound in &type_param.bounds {
                    if let syn::TypeParamBound::Trait(trait_bound) = bound {
                        if let Some(segment) = trait_bound.path.segments.last() {
                            let ident_str = segment.ident.to_string();
                            if ident_str == "FnMut" || ident_str == "Fn" || ident_str == "FnOnce" {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }

    // Also check where clause
    if let Some(where_clause) = &generics.where_clause {
        for predicate in &where_clause.predicates {
            if let syn::WherePredicate::Type(pred) = predicate {
                if let Type::Path(bounded_type) = &pred.bounded_ty {
                    if bounded_type.path.segments.len() == 1
                        && bounded_type.path.segments[0].ident == *type_ident
                    {
                        for bound in &pred.bounds {
                            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                                if let Some(segment) = trait_bound.path.segments.last() {
                                    let ident_str = segment.ident.to_string();
                                    if ident_str == "FnMut"
                                        || ident_str == "Fn"
                                        || ident_str == "FnOnce"
                                    {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    false
}

/// Unified check: is this type Fn-like, either syntactically or via generic bounds?
fn is_fn_param(ty: &Type, generics: &syn::Generics) -> bool {
    is_fn_like_type(ty) || is_generic_fn_like(ty, generics)
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
    let mut param_info = Vec::new();

    for (index, arg) in func.sig.inputs.iter_mut().enumerate() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            // For impl Trait types, we can't create intermediate bindings
            // Keep the original pattern and use it directly
            let is_impl_trait = matches!(**ty, Type::ImplTrait(_));

            if is_impl_trait {
                // Keep original pattern for impl Trait params
                let original_pat: Box<Pat> = pat.clone();
                if let Pat::Ident(pat_ident) = &**pat {
                    param_info.push((pat_ident.ident.clone(), original_pat, (*ty).clone()));
                }
            } else {
                // Rename other params to __argN
                let ident = Ident::new(&format!("__arg{}", index), Span::call_site());
                let original_pat: Box<Pat> = pat.clone();
                *pat = Box::new(syn::parse_quote! { #ident });
                param_info.push((ident, original_pat, (*ty).clone()));
            }
        }
    }

    let original_block = func.block.clone();
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };

    // Rebinds will be generated later in the helper_body context where we have access to slots
    let rebinds_for_no_skip: Vec<_> = param_info
        .iter()
        .map(|(ident, pat, _)| {
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
        .map(|(ident, _pat, ty)| quote! { #ident: #ty })
        .collect();

    // Check if any params are impl Trait - if so, can't use skip optimization
    let has_impl_trait = param_info.iter().any(|(_, _, ty)| matches!(**ty, Type::ImplTrait(_)));

    if enable_skip && !has_impl_trait {
        let helper_ident = Ident::new(
            &format!("__compose_impl_{}", func.sig.ident),
            Span::call_site(),
        );
        let generics = func.sig.generics.clone();
        let (impl_generics, _ty_generics, where_clause) = generics.split_for_impl();

        // Helper function signature: all params except impl Trait (which can't be named)
        let helper_inputs: Vec<TokenStream2> = param_info
            .iter()
            .filter_map(|(ident, _pat, ty)| {
                let is_impl_trait = matches!(**ty, Type::ImplTrait(_));
                if is_impl_trait {
                    None
                } else {
                    Some(quote! { #ident: #ty })
                }
            })
            .collect();

        // Separate Fn-like params from regular params
        let param_state_ptrs: Vec<Ident> = (0..param_info.len())
            .map(|index| Ident::new(&format!("__param_state_ptr{}", index), Span::call_site()))
            .collect();

        let param_setup: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .filter_map(|((ident, _pat, ty), ptr_ident)| {
                // Skip impl Trait types - can't create intermediate bindings for them
                let is_impl_trait = matches!(**ty, Type::ImplTrait(_));
                if is_impl_trait {
                    // For impl Trait, always mark as changed (can't track)
                    return Some(quote! { __changed = true; });
                }

                if is_fn_param(ty, &generics) {
                    // Fn-like parameters: use ParamSlot (no PartialEq/Clone required)
                    // Store by move, always mark as changed
                    Some(quote! {
                        let #ptr_ident: *mut compose_core::ParamSlot<#ty> = {
                            let __slot_ref = __composer
                                .remember(|| compose_core::ParamSlot::<#ty>::default());
                            __slot_ref as *mut compose_core::ParamSlot<#ty>
                        };
                        // Move the closure into the slot; always mark as changed
                        unsafe { (&mut *#ptr_ident).set(#ident) };
                        __changed = true;
                    })
                } else {
                    // Regular parameters: use ParamState with PartialEq comparison
                    Some(quote! {
                        let #ptr_ident: *mut compose_core::ParamState<#ty> = {
                            let __slot_ref = __composer
                                .remember(|| compose_core::ParamState::<#ty>::default());
                            __slot_ref as *mut compose_core::ParamState<#ty>
                        };
                        if unsafe { (&mut *#ptr_ident).update(&#ident) } {
                            __changed = true;
                        }
                    })
                }
            })
            .collect();

        // Generate rebinds: regular params get normal rebinds, Fn params get rebound from slots
        let rebinds: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .map(|((ident, pat, ty), ptr_ident)| {
                let is_impl_trait = matches!(**ty, Type::ImplTrait(_));
                if is_impl_trait {
                    // impl Trait: no rebind needed, already has original name
                    quote! {}
                } else if is_fn_param(ty, &generics) {
                    // Fn-like param: rebind as &mut from slot
                    quote! {
                        let #pat = unsafe { (&mut *#ptr_ident).as_mut() };
                    }
                } else {
                    // Regular rebind
                    quote! {
                        let #pat = #ident;
                    }
                }
            })
            .collect();

        // Recompose args: for Fn params take from ParamSlot, for regular params clone from ParamState
        let recompose_args: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .filter_map(|((_, _, ty), ptr_ident)| {
                let is_impl_trait = matches!(**ty, Type::ImplTrait(_));
                if is_impl_trait {
                    // impl Trait: can't pass through recompose callback
                    None
                } else if is_fn_param(ty, &generics) {
                    // Fn-like params: take from ParamSlot (will be set again by param_setup)
                    Some(quote! {
                        unsafe { (&mut *#ptr_ident).take() }
                    })
                } else {
                    // Regular params: clone from ParamState
                    Some(quote! {
                        unsafe {
                            (&*#ptr_ident)
                                .value()
                                .expect("composable parameter missing for recomposition")
                        }
                    })
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

        // Wrapper args: pass all params except impl Trait on initial call
        let wrapper_args: Vec<TokenStream2> = param_info
            .iter()
            .filter_map(|(ident, _pat, ty)| {
                let is_impl_trait = matches!(**ty, Type::ImplTrait(_));
                if is_impl_trait {
                    None
                } else {
                    Some(quote! { #ident })
                }
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
        // no_skip path: still uses simple rebinds
        let wrapped = quote!({
            compose_core::with_current_composer(|__composer: &mut compose_core::Composer<'_>| {
                __composer.with_group(#key_expr, |__scope: &mut compose_core::Composer<'_>| {
                    #(#rebinds_for_no_skip)*
                    #original_block
                })
            })
        });
        func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
        TokenStream::from(quote! { #func })
    }
}
