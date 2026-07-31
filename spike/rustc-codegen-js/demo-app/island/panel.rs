//! A component that takes child content, so the page exercises the side buffer
//! a component's children are rendered into.
//!
//! Child content is a prop, so it is built where the component is CALLED rather
//! than where the component renders it. The server therefore renders it aside
//! before it enters the component, and hands the component's own view a stand-in
//! that writes those bytes wherever the content appears. That is what puts the
//! content's hydration keys before the component's own boundary, which is the
//! order an eager argument has on both sides.
//!
//! It is called from the nested island's PAGE rather than from inside an island:
//! the client emitter does not lower child content at a call site yet, so this
//! declares both halves and exercises the server's buffer. The client half is
//! compiled and dropped, because nothing calls it.

/// A titled panel wrapping whatever it was called with.
///
/// The child parameter is declared as the type the SERVER takes it as; the
/// client half replaces it with the built node, which is why naming `topcoat`
/// here does not reach the client crate.
#[::view_dom_macro::component(client)]
pub async fn panel(title: &str, child: ::topcoat::view::View) -> ::topcoat::Result {
    view! {
        <section class="panel">
            <h2>(title)</h2>
            (child)
        </section>
    }
}
