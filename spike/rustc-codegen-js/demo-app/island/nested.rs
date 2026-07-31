//! The nesting island: an island whose view calls components, which call
//! components.
//!
//! Like `counter.rs` this file is a module of two crates, and nothing in it is
//! written for either target in particular. What it adds to the counter is a
//! component boundary: `labelled_card` is a `#[component(client)]` the island
//! calls, and `badge` is one that `labelled_card` calls in turn, so the keys the
//! server writes nest two contexts deep and the browser has to open the same two
//! to find those nodes again.
//!
//! The shape is `contract/fixtures/corpus/07b-nested-components`'s
//! `nested_in_element` case, which the parity fixture
//! `contract/parity/fixtures/nested` measures against. Its keys are `0` for the
//! island's own root, `10` for the card the call at slot 1 opens, and `110` for
//! the badge the call at slot 1 of THAT context opens.
//!
//! The signal and the button are here so the fixture measures a live island
//! rather than a static one: a click must change the count and touch nothing
//! inside the two components. The components themselves are deliberately not
//! reactive, because a signal prop and a `$(...)` prop value are separate
//! features and folding them in would make one failure look like another.

/// A label in a pill, the innermost component.
#[::view_dom_macro::component(client)]
pub async fn badge(label: &str) -> ::topcoat::Result {
    view! { <span class="badge">(label)</span> }
}

/// A titled card with a badge inside it, so the island's call nests twice.
#[::view_dom_macro::component(client)]
pub async fn labelled_card(title: &str) -> ::topcoat::Result {
    view! {
        <section class="card">
            <h2>(title)</h2>
            badge(label: "Active")
        </section>
    }
}

/// A counter with a nested card in it, rendered by the server and taken over by
/// the browser.
#[::view_dom_macro::island]
pub async fn nested(start: f64) -> ::topcoat::Result {
    view! {
        <div class="island">
            signal count = start;

            <p class="island-count">
                "count "
                $(count.get())
            </p>

            labelled_card(title: "Profile")

            <div class="island-controls">
                <button
                    class="island-step"
                    type="button"
                    @click=$(|_e| count.set(count.get() + 1.0))
                >
                    "+1"
                </button>
            </div>
        </div>
    }
}
