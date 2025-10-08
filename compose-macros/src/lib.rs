use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_quote, FnArg, Ident, ItemFn, Pat, PatType, Type, TypePath};

#[proc_macro_attribute]
pub fn composable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut func = parse_macro_input!(item as ItemFn);
    let composer_ident = ensure_composer_arg(&mut func);
    let original_block = func.block;
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };
    let scope_ident = Ident::new("__compose_scope", proc_macro2::Span::call_site());
    let wrapped = quote!({
        #composer_ident.with_group(#key_expr, |#scope_ident: &mut compose_core::Composer<'_>| {
            let #composer_ident = #scope_ident;
            #original_block
        })
    });
    func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
    TokenStream::from(quote! { #func })
}

fn ensure_composer_arg(func: &mut ItemFn) -> Ident {
    for arg in func.sig.inputs.iter() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            if let (Pat::Ident(pat_ident), Type::Reference(reference)) = (&**pat, &**ty) {
                if reference.mutability.is_some() {
                    if let Type::Path(TypePath { path, .. }) = &*reference.elem {
                        if path.segments.last().map(|seg| seg.ident == "Composer").unwrap_or(false) {
                            return pat_ident.ident.clone();
                        }
                    }
                }
            }
        }
    }
    let ident = Ident::new("__composer", proc_macro2::Span::call_site());
    let arg: FnArg = parse_quote! { #ident: &mut compose_core::Composer<'_> };
    func.sig.inputs.insert(0, arg);
    ident
}
