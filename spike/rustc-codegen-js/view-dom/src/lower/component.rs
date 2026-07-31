use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use topcoat_view_grammar::component::props_ident;
use topcoat_view_grammar::view::{Component, NamedArgValue};

use crate::DomWriter;

/// The call a component invocation lowers to, as a closure the runtime calls
/// once it has opened the component's own key context.
///
/// Every component a client view calls is a client component: a component
/// declared for the server alone does not exist under `topcoat_client`, so a call
/// to one fails to resolve and names the component that is missing. That is why
/// the emitter needs no registry of its own to tell the two apart.
///
/// Props are named rather than ordered, so they are handed over as the props
/// struct both halves declare. That is what lets the emitter build a call without
/// knowing the order the component declared its props in.
pub(crate) fn component_tokens(
    node: &Component,
    writer: &mut DomWriter<'_>,
) -> Option<TokenStream> {
    if !node.children.is_empty() {
        writer.error(
            node.paren_token.span.span(),
            "child nodes of a component are not lowered yet: the server writes a key when a node \
             renders and the client claims one when a node is built, and for an eager child \
             argument those fall on opposite sides of the component's own view",
        );
        return None;
    }

    let path = &node.path;
    let last = node.path.segments.last()?;
    let props = props_ident(&last.ident);
    let mut props_path = node.path.clone();
    if let Some(segment) = props_path.segments.last_mut() {
        segment.ident = props;
        segment.arguments = syn::PathArguments::None;
    }

    let mut fields = Vec::with_capacity(node.named_args.len());
    for arg in &node.named_args {
        let name = &arg.ident;
        match &arg.value {
            NamedArgValue::Expr(value) => fields.push(quote! { #name: #value }),
            NamedArgValue::Runtime(value) => {
                writer.error(
                    value.span(),
                    "a runtime expression is not a component prop yet: a prop is evaluated once \
                     where the component is called, so one that has to stay reactive is passed as \
                     a value that carries its own reactivity",
                );
                return None;
            }
        }
    }

    // The call is a closure so that whatever opens the component's key context
    // can run before the component's own view is built.
    Some(quote_spanned! {node.paren_token.span.span()=>
        move || #path(#props_path { #(#fields,)* })
    })
}
