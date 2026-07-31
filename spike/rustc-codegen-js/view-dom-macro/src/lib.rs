//! The spike's exerciser for the DOM emitter.
//!
//! `dom_view!` parses the same body `view!` does and lowers it with
//! [`view_dom`], so the emitter can be driven end to end without touching the
//! upstream macro. Upstream, `view!` itself picks the emitter that matches the
//! target it is compiled for.

use proc_macro::TokenStream;
use topcoat_view_grammar::view::View;
use view_dom::{ClientComponent, DomOutput, Island};

/// Declares a view the server renders and the client takes over.
///
/// The function's body is a single `view!` invocation, which is lowered twice:
/// to a `#[component]` that renders the view as an island instance, and to the
/// client entry point the island loader calls. See [`view_dom::Island`].
#[proc_macro_attribute]
pub fn island(attr: TokenStream, item: TokenStream) -> TokenStream {
    match Island::parse(attr.into(), item.into()) {
        Ok(island) => island.expand().into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Declares a component both the server and the client render.
///
/// Only `#[component(client)]` is accepted: the argument is what says the
/// component has a client half at all. The component is lowered twice, to the
/// ordinary `#[component]` and to a plain function a client view calls. See
/// [`view_dom::ClientComponent`].
#[proc_macro_attribute]
pub fn component(attr: TokenStream, item: TokenStream) -> TokenStream {
    match ClientComponent::parse(attr.into(), item.into()) {
        Ok(component) => component.expand().into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Expands a view body to its template statics and the client body that
/// instantiates them, in hydratable mode.
#[proc_macro]
pub fn dom_view(input: TokenStream) -> TokenStream {
    expand(input, true)
}

/// Expands a view body the way `dom_view!` does, for a client that renders the
/// markup itself instead of claiming what a server wrote.
#[proc_macro]
pub fn dom_view_client_only(input: TokenStream) -> TokenStream {
    expand(input, false)
}

fn expand(input: TokenStream, hydratable: bool) -> TokenStream {
    let view = match syn::parse::<View>(input) {
        Ok(view) => view,
        Err(error) => return error.to_compile_error().into(),
    };

    match DomOutput::build(&view, hydratable) {
        Ok(output) => output.expand().into(),
        Err(error) => error.to_compile_error().into(),
    }
}
