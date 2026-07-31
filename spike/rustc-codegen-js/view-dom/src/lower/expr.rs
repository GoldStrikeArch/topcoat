use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use topcoat_view_grammar::{
    template::{RuntimeExpr, TemplateExpr},
    view::KeySite,
};

use crate::{DomWriter, Fill, WriteDom};

impl WriteDom for TemplateExpr {
    fn write(&self, writer: &mut DomWriter<'_>) {
        writer.child_hole(
            self.expr.to_token_stream(),
            Fill::Once,
            self.paren.span.join(),
        );
    }
}

impl WriteDom for RuntimeExpr {
    fn write(&self, writer: &mut DomWriter<'_>) {
        writer.key(KeySite::ReactiveScope);
        let span = self.paren.span.join();
        // A reactive conditional hands its test over on its own, so the backend can hoist it into a
        // memo; anything else is one closure the backend re-runs.
        match cond_tokens(&self.expr) {
            Some(value) => writer.child_hole(value, Fill::ControlFlow, span),
            None => writer.child_hole(self.expr.to_token_stream(), Fill::Effect, span),
        }
    }
}

/// `$(if a { x } else { y })` as a `view_abi::cond`, or `None` for anything else.
///
/// The value a `$(...)` hole is filled with is an ordinary Rust expression, and one written as an
/// `if` is the same conditional the markup-bodied form is: its test decides which branch is read,
/// and hoisting that test into a memo is what keeps the branch that was not taken from being
/// rebuilt whenever anything else the hole reads changes. The branches here are values rather than
/// markup, which is what `content` erases to one type.
///
/// A test that binds is left alone, for the reason [`crate::lower::if_tokens`] gives.
fn cond_tokens(expr: &syn::Expr) -> Option<TokenStream> {
    let syn::Expr::If(node) = expr else {
        return None;
    };
    if matches!(*node.cond, syn::Expr::Let(_)) {
        return None;
    }
    let test = &node.cond;
    let then_branch = &node.then_branch;
    let else_branch = match &node.else_branch {
        // An `else if` nests as another `cond`, exactly as a markup-bodied chain does.
        Some((_, els)) => cond_tokens(els).unwrap_or_else(|| quote! { ::view_abi::content(#els) }),
        None => quote! { ::view_abi::content(()) },
    };
    Some(quote! {
        ::view_abi::cond(
            || #test,
            || ::view_abi::content(#then_branch),
            || #else_branch,
        )
    })
}
