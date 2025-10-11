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

    let original_block = func.block;
    let original_block_clone = original_block.clone();
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

    let skip_logic = if enable_skip && !param_info.is_empty() {
        let param_updates = param_info.iter().map(|(ident, _pat, ty)| {
            quote! {
                let __state = __scope.remember(|| compose_core::ParamState::<#ty>::default());
                if __state.update(&#ident) {
                    __changed = true;
                }
            }
        });
        quote! {
            let mut __changed = false;
            #(#param_updates)*
            let __result_slot_ptr: *mut compose_core::ReturnSlot<#return_ty> = {
                let __slot_ref = __scope
                    .remember(|| compose_core::ReturnSlot::<#return_ty>::default());
                __slot_ref as *mut compose_core::ReturnSlot<#return_ty>
            };
            if !__changed {
                __scope.skip_current_group();
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
            __value
        }
    } else {
        quote! {
            #(#rebinds)*
            #original_block_clone
        }
    };

    let wrapped = quote!({
        compose_core::with_current_composer(|__composer: &mut compose_core::Composer<'_>| {
            __composer.with_group(#key_expr, |__scope: &mut compose_core::Composer<'_>| {
                #skip_logic
            })
        })
    });
    func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
    TokenStream::from(quote! { #func })
}
