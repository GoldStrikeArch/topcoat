//! The counter island: one view, compiled twice.
//!
//! This file is a module of two crates. The server crate reads it as ordinary
//! Topcoat, where the view renders to HTML carrying the hydration keys the
//! client finds its nodes by. The client crate reads it with `topcoat_client`
//! set, where the same view lowers to a dom-expressions template and the whole
//! crate is compiled to JavaScript by the rustc backend.
//!
//! Nothing below is written for either target in particular. The signal, the
//! handlers, and the interpolation are the ones the runtime guide's counter
//! uses; what changes is which compiler reads them.

/// A counter starting at `start`, rendered by the server and taken over by the
/// browser.
#[::view_dom_macro::island]
pub async fn counter(start: f64) -> ::topcoat::Result {
    view! {
        <div class="island">
            signal count = start;

            <p class="island-count">
                "count "
                $(count.get())
            </p>

            <div class="island-controls">
                <button
                    class="island-step"
                    type="button"
                    :disabled=$(count.get() <= 0.0)
                    @click=$(|_e| count.set(count.get() - 1.0))
                >
                    "-1"
                </button>
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
