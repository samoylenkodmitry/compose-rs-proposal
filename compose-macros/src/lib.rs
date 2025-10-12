use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, FnArg, Ident, ItemFn, Pat, PatType, ReturnType};

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

    let rebinds: Vec<_> = param_info
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

        let param_state_ptrs: Vec<Ident> = (0..param_info.len())
            .map(|index| Ident::new(&format!("__param_state_ptr{}", index), Span::call_site()))
            .collect();

        let param_setup: Vec<TokenStream2> = param_info
            .iter()
            .zip(param_state_ptrs.iter())
            .map(|((ident, _pat, ty), ptr_ident)| {
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
            })
            .collect();

        let recompose_args: Vec<TokenStream2> = param_state_ptrs
            .iter()
            .enumerate()
            .map(|(index, ptr_ident)| {
                let message = format!("composable parameter {} missing for recomposition", index);
                quote! {
                    unsafe {
                        (&*#ptr_ident)
                            .value()
                            .expect(#message)
                    }
                }
            })
            .collect();

        let helper_body = quote! {
            let __current_scope = __composer
                .current_recompose_scope()
                .expect("missing recompose scope");
            let mut __changed = __current_scope.is_invalid();
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
