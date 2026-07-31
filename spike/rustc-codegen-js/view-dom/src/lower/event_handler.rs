use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use topcoat_view_grammar::{
    attributes::{AttributeKey, EventHandler, EventHandlerValue},
    template::TemplateOrRuntimeExpr,
};

use crate::{DomWriter, WriteDom};

impl WriteDom for EventHandler {
    fn write(&self, writer: &mut DomWriter<'_>) {
        let AttributeKey::Ident(key) = &self.key else {
            let span = self
                .key
                .as_expr()
                .map_or_else(|| self.at.span, |expr| expr.paren.span.join());
            writer.unsupported(span, "an event handler with an expression name");
            return;
        };

        let value = match &self.value {
            EventHandlerValue::Expr(value) => match value.as_ref() {
                TemplateOrRuntimeExpr::Template(inner) => inner.expr.to_token_stream(),
                TemplateOrRuntimeExpr::Runtime(inner) => inner.expr.to_token_stream(),
            },
            EventHandlerValue::LitStr(value) => {
                writer.unsupported(value.span(), "an event handler written as JavaScript");
                return;
            }
        };

        writer.event_hole(&key.to_string(), handler_closure(value), self.at.span);
    }
}

/// The handler as a closure over the event, which is what `view_abi::handler`
/// takes.
///
/// Three forms reach here, and each has to end up as something callable with the
/// event:
///
/// * A **closure written in place** is the handler. Its parameter is annotated if it was written
///   bare, because nothing else gives it a type: a handler is passed through a marker generic in
///   the closure, so `|event|` has no signature to be inferred from. One that annotates its own
///   parameter keeps the annotation it was written with.
/// * A **name** is the function to call, and is passed through: `@click=$(reset)` names `reset`
///   rather than evaluating it.
/// * **Any other expression** is the body to run when the event fires, which is what Topcoat's
///   `@click=$(count.set(1))` means, so it becomes the body of a closure over the event. The event
///   is named `__tc_event`, which the wrapped expression is free to ignore, and the value is
///   discarded the way a DOM listener's is.
///
/// Either closure form is emitted as a `move` closure, for the reason
/// [`crate::Fill::Effect`] gives: what a handler captures is a runtime handle,
/// which is one `Copy` machine word, and copying it into the environment is
/// smaller than capturing a reference to the local that holds it.
fn handler_closure(value: TokenStream) -> TokenStream {
    let Ok(closure) = syn::parse2::<syn::ExprClosure>(value.clone()) else {
        // A name is a function to call; anything else is a body to run.
        if matches!(
            syn::parse2::<syn::Expr>(value.clone()),
            Ok(syn::Expr::Path(_))
        ) {
            return value;
        }
        return quote! { move |__tc_event: &::view_abi::Event| #value };
    };

    let syn::ExprClosure {
        attrs,
        lifetimes,
        constness,
        movability,
        asyncness,
        inputs,
        output,
        body,
        ..
    } = &closure;
    let parameters = match inputs.iter().collect::<Vec<_>>()[..] {
        // A handler taking nothing is written `||` rather than `| |`, which is
        // what a bare pair of pipes tokenizes as when nothing separates them.
        [] => quote! { || },
        [syn::Pat::Ident(parameter)]
            if parameter.by_ref.is_none() && parameter.subpat.is_none() =>
        {
            quote! { |#parameter: &::view_abi::Event| }
        }
        _ => quote! { |#inputs| },
    };
    quote! {
        #(#attrs)* #lifetimes #constness #movability #asyncness move
        #parameters #output #body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(source: &str) -> String {
        handler_closure(source.parse().expect("the handler parses")).to_string()
    }

    #[test]
    fn a_bare_parameter_is_annotated() {
        assert_eq!(
            typed("|event| count.set(1)"),
            "move | event : & :: view_abi :: Event | count . set (1)"
        );
    }

    #[test]
    fn an_annotated_parameter_keeps_its_annotation() {
        assert_eq!(typed("|event: u32| event"), "move | event : u32 | event");
    }

    #[test]
    fn a_name_is_the_function_to_call() {
        assert_eq!(typed("on_click"), "on_click");
        assert_eq!(typed("handlers::reset"), "handlers :: reset");
    }

    #[test]
    fn any_other_expression_becomes_the_body_of_a_handler() {
        assert_eq!(
            typed("count.set(1)"),
            "move | __tc_event : & :: view_abi :: Event | count . set (1)"
        );
        // The wrapped expression keeps its own value; nothing reads it, exactly as
        // nothing reads a DOM listener's.
        assert_eq!(
            typed("count + 1"),
            "move | __tc_event : & :: view_abi :: Event | count + 1"
        );
    }

    #[test]
    fn only_a_single_parameter_is_annotated() {
        assert_eq!(typed("|a, b| a"), "move | a , b | a");
        assert_eq!(typed("|| 1"), "move || 1");
    }

    #[test]
    fn every_closure_form_captures_by_move() {
        // A handler captures runtime handles, which are one `Copy` word each, so
        // copying them into the environment is what keeps the local unboxed. A
        // handler already written `move` is unchanged by it.
        for source in [
            "|event| event",
            "|event: u32| event",
            "count.set(1)",
            "|| 1",
        ] {
            assert!(typed(source).starts_with("move "), "{source}");
        }
        assert_eq!(
            typed("move |event| event"),
            "move | event : & :: view_abi :: Event | event"
        );
        // A name is not a closure, so there is nothing to capture.
        assert_eq!(typed("on_click"), "on_click");
    }
}
