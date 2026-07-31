use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use topcoat_view_grammar::{
    template::{TemplateElse, TemplateForLoop, TemplateIf, TemplateMatch},
    view::{KeySite, Node, Nodes},
};

use crate::{DomWriter, Fill, WriteDom};

/// The expression a node-position `if` chain fills its hole with, and how it is
/// filled.
///
/// The test is handed over on its own, as `view_abi::cond`'s first closure, so
/// the backend can hoist it into a memo and rebuild a branch only when the
/// test's answer changes rather than whenever anything a branch read does. A
/// conditional written as one closure reaches the backend as a single opaque
/// function with no test left in it to hoist.
///
/// An `else if` nests: the `else` closure's body is another `cond`, which is how
/// the reference compiler nests its own memos.
///
/// # `if let`
///
/// A test that binds cannot be handed over on its own: `cond`'s test is a
/// `Fn() -> bool`, and `let Some(x) = xs` is an expression only inside the `if`
/// it belongs to. Such a chain stays one closure the backend re-runs, exactly as
/// before, and gets no memo. Splitting it would mean matching twice, once for the
/// test and once to bind, which is a different program from the one that was
/// written.
pub(crate) fn if_tokens(
    node: &TemplateIf<Nodes>,
    writer: &mut DomWriter<'_>,
) -> (TokenStream, Fill) {
    let tokens = if_chain(node, writer);
    // A `let` test is the one shape that cannot be hoisted; see the note above.
    let fill = match binds(node) {
        true => Fill::Effect,
        false => Fill::ControlFlow,
    };
    (tokens, fill)
}

/// The chain itself, without deciding how the hole is filled.
fn if_chain(node: &TemplateIf<Nodes>, writer: &mut DomWriter<'_>) -> TokenStream {
    writer.key(KeySite::ReactiveScope);

    let cond = &node.cond;
    let then_branch = writer.branch(|writer| node.then_branch.children.write(writer));
    let else_branch = match &node.else_branch {
        Some(TemplateElse::ElseIf { template_if, .. }) => if_chain(template_if, writer),
        Some(TemplateElse::Else { then_branch, .. }) => {
            writer.branch(|writer| then_branch.children.write(writer))
        }
        // A missing `else` renders nothing.
        None => quote! { ::view_abi::content(()) },
    };

    if binds(node) {
        return quote! { if #cond { #then_branch } else { #else_branch } };
    }
    quote! {
        ::view_abi::cond(
            || #cond,
            || #then_branch,
            || #else_branch,
        )
    }
}

/// Whether any test in this chain binds, and so cannot be a closure of its own.
fn binds(node: &TemplateIf<Nodes>) -> bool {
    if matches!(&node.cond, syn::Expr::Let(_)) {
        return true;
    }
    match &node.else_branch {
        Some(TemplateElse::ElseIf { template_if, .. }) => binds(template_if),
        _ => false,
    }
}

/// The expression a node-position `match` fills its hole with. A guard stays on
/// the arm it was written on.
///
/// A `match` stays one closure the backend re-runs, and so gets no memo. Its
/// arms select on a *pattern* rather than on a boolean, and an arm's body reads
/// what the pattern bound, so there is no test to hand over separately: turning
/// the arms into `cond` tests would mean matching once to choose the arm and
/// again inside it to bind, which is a different program from the one that was
/// written. `if`/`else if` is the shape that hoists; see [`if_tokens`].
pub(crate) fn match_tokens(node: &TemplateMatch<Node>, writer: &mut DomWriter<'_>) -> TokenStream {
    writer.key(KeySite::ReactiveScope);

    let expr = &node.expr;
    let mut arms = TokenStream::new();
    for arm in &node.arms {
        let pat = &arm.pat;
        let guard = arm
            .guard
            .as_ref()
            .map(|(if_token, cond)| quote! { #if_token #cond });
        let body = writer.branch(|writer| arm.body.write(writer));
        arms.extend(quote! { #pat #guard => #body, });
    }

    quote! { match #expr { #arms } }
}

/// The expression a node-position `for` fills its hole with, and how it is
/// filled.
///
/// The loop stays Rust: it runs where it is written and appends each row to the
/// list `view_abi::list` starts. See that marker for what this renders.
///
/// A `key (expr)` clause appends through `view_abi::push_keyed` instead, which
/// matches each row against the row of the same key from the previous render. See
/// that marker for what a key buys and what it costs.
///
/// The key expression is evaluated inside the loop, once per row, where the
/// pattern's bindings are in scope: a key is almost always a field of the item.
///
/// # When the list re-runs
///
/// A loop over something that reads a signal is filled through `view_abi::effect`
/// rather than `view_abi::hole`, which hands `_$insert` an accessor instead of an
/// array: the runtime subscribes to it, so writing the signal renders the list
/// again with the rows the collection now holds. Every other loop is written once,
/// exactly as before.
///
/// What decides is the ITERATED EXPRESSION, and only it. That expression is what
/// the set of rows comes from, and it is what the reference compiler tracks too:
/// its `each` prop is the accessor, and a row's own reactivity is the row's. A
/// `$(..)` inside the body is already a reactive hole of the row's own template,
/// so re-running the list for it would rebuild every row and fire that hole as
/// well; a plain `(..)` inside the body is written once because that is what a
/// plain hole means everywhere else in a view.
pub(crate) fn for_tokens(
    node: &TemplateForLoop<Nodes>,
    writer: &mut DomWriter<'_>,
) -> (TokenStream, Fill) {
    writer.key(KeySite::ReactiveScope);

    // Asked before the body is lowered, because a row is not part of the answer.
    let fill = match writer.reads_a_signal(&node.expr) {
        true => Fill::Effect,
        false => Fill::ControlFlow,
    };

    let expr = &node.expr;
    let pat = &node.pat;
    let row = writer.branch(|writer| node.body.children.write(writer));
    let rows = format_ident!("__tc_rows");

    let push = match &node.key {
        Some(key) => {
            let key = &key.expr;
            quote! { ::view_abi::push_keyed(&#rows, #key, #row) }
        }
        None => quote! { ::view_abi::push(&#rows, #row) },
    };

    let tokens = quote! {
        {
            let #rows = ::view_abi::list();
            for #pat in #expr {
                #push;
            }
            #rows
        }
    };
    (tokens, fill)
}
