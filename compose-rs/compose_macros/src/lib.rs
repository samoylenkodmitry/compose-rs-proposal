use proc_macro::TokenStream;
use quote::quote;
use seahash::SeaHasher;
use std::hash::Hash;
use syn::{
    parse_macro_input,
    spanned::Spanned,
    visit_mut::{self, VisitMut},
    Expr, ExprCall, ItemFn,
};

struct ComposableCallVisitor {
    call_site_counter: u64,
}

impl ComposableCallVisitor {
    fn new() -> Self {
        Self {
            call_site_counter: 0,
        }
    }

    fn next_key(&mut self, call: &ExprCall) -> u64 {
        let mut hasher = SeaHasher::new();
        call.span().source_file().path().to_string_lossy().hash(&mut hasher);
        call.span().start().line.hash(&mut hasher);
        call.span().start().column.hash(&mut hasher);
        self.call_site_counter.hash(&mut hasher);
        self.call_site_counter += 1;
        hasher.finish()
    }
}

impl VisitMut for ComposableCallVisitor {
    fn visit_expr_mut(&mut self, i: &mut Expr) {
        if let Expr::Call(call) = i {
            if let Expr::Path(path) = &*call.func {
                if let Some(segment) = path.path.segments.last() {
                    let ident = segment.ident.to_string();
                    if ident.starts_with(char::is_uppercase) {
                        let key = self.next_key(call);
                        call.args
                            .insert(0, syn::parse_quote!(::compose_core::Key(#key)));
                        call.args.insert(0, syn::parse_quote!(composer));
                    }
                }
            }
        }

        visit_mut::visit_expr_mut(self, i);
    }
}

#[proc_macro_attribute]
pub fn composable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut func = parse_macro_input!(item as ItemFn);

    // 1. Inject `composer` and `key` into the function signature.
    func.sig
        .inputs
        .insert(0, syn::parse_quote!(key: ::compose_core::Key));
    func.sig
        .inputs
        .insert(0, syn::parse_quote!(composer: &mut ::compose_core::Composer));

    // 2. Rewrite calls to other composables within the function body.
    let mut visitor = ComposableCallVisitor::new();
    visitor.visit_block_mut(&mut func.block);

    // 3. Wrap the body in `start_group`/`end_group` calls.
    let block = &func.block;
    func.block = Box::new(
        syn::parse2(quote! {{
            let start_idx = composer.slots.start_group(key);
            let result = #block;
            composer.slots.end_group(start_idx);
            result
        }})
        .unwrap(),
    );

    TokenStream::from(quote!(#func))
}