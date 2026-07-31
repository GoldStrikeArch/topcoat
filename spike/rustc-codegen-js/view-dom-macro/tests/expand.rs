//! The expansion has to type-check against the real ABI, even though the
//! marker functions only mean something to the backend and never run.

use view_dom_macro::{dom_view, dom_view_client_only};

fn counter(label: &str, count: u32) -> view_abi::Node {
    dom_view! {
        <button class="counter" title=(label) @click=$(count + 1)>
            "count: " (count)
        </button>
    }
}

fn siblings(name: &str) -> (view_abi::Node, view_abi::Node) {
    dom_view! {
        <h1>(name)</h1>
        <p class="lead">"welcome"</p>
    }
}

fn signals() -> view_abi::Node {
    dom_view! {
        signal count = 0u32;
        <button @click=$(count.set(count.get() + 1))>
            "count: " $(count.get())
        </button>
    }
}

fn conditional(flag: bool, name: &str) -> view_abi::Node {
    dom_view! {
        <div>
            if flag { <b>"on"</b> } else if name.is_empty() { "anonymous" } else { <i>(name)</i> }
        </div>
    }
}

fn matching(state: Option<u32>) -> view_abi::Node {
    dom_view! {
        <div>
            match state {
                Some(n) if n > 10 => <b>(n)</b>,
                Some(n) => (n),
                None => "none",
            }
        </div>
    }
}

fn listing(items: Vec<String>) -> view_abi::Node {
    dom_view! {
        <ul>
            let total = items.len();
            <li class="count">(total)</li>
            // A tracked closure may re-run, so the iterable is borrowed rather
            // than consumed.
            for item in &items { <li title=(item.clone())>(item)</li> }
        </ul>
    }
}

fn bound(value: &str, disabled: bool) -> view_abi::Node {
    dom_view_client_only! {
        <input :value=$(value) :disabled=$(disabled) :class=$("field")>
    }
}

/// A client component's two declarations, written out here the way
/// `#[component(client)]` emits them under `topcoat_client`.
struct BadgeProps<'a> {
    title: &'a str,
    count: u32,
}

fn badge(props: BadgeProps<'_>) -> view_abi::Node {
    let BadgeProps { title, count } = props;
    dom_view! { <span class="badge" title=(title)>(count)</span> }
}

fn calls_a_component(name: &str) -> view_abi::Node {
    dom_view! {
        <div>
            <h1>(name)</h1>
            badge(title: name, count: 1)
        </div>
    }
}

#[test]
fn the_expansion_type_checks() {
    // Nothing is called: the markers are `unreachable!` outside the backend.
    let _ = (
        counter,
        siblings,
        signals,
        conditional,
        matching,
        listing,
        bound,
    );
    let _ = (badge, calls_a_component);
}
