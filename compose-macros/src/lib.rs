use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

#[proc_macro_attribute]
pub fn composable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut func = parse_macro_input!(item as ItemFn);
    let original_block = func.block;
    let key_expr = quote! { compose_core::location_key(file!(), line!(), column!()) };
    let wrapped = quote!({
        compose_core::with_current_composer(|__composer: &mut compose_core::Composer<'_>| {
            __composer.with_group(#key_expr, |_scope: &mut compose_core::Composer<'_>| #original_block)
        })
    });
    func.block = Box::new(syn::parse2(wrapped).expect("failed to build block"));
    TokenStream::from(quote! { #func })
}
