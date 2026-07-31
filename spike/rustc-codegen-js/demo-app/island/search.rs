//! The search island: an input, a debounced call to the server, and the reply
//! rendered as a list.
//!
//! Like `counter.rs` this file is a module of two crates and nothing in it is
//! written for either target in particular, with one exception that is the whole
//! interest of the island: the two functions the view calls are declared twice,
//! once per target, because asking a server for something is a thing only the
//! browser half can do.
//!
//! # Where the query lives, and why not in the reply
//!
//! Two signals, both of a type that exists on both halves. `query` holds what
//! was typed, and `replies` counts the answers that have come back. The result
//! list reads both, so it is rebuilt when the user types and again when an
//! answer lands, which is the whole update loop.
//!
//! The reply itself is not a Rust value. It is JSON the browser already parsed,
//! and the rows are read out of it one at a time by [`hits`]. That is what lets
//! this island run with no allocator: the island crate is `#![no_std]` with no
//! heap, so a `Vec<String>` of results is not something it could hold even if
//! the reply were decoded in Rust.
//!
//! # What the view may say
//!
//! A `$( ... )` value is compiled by the server's expression language as well as
//! by the client compiler, and that language has no free function calls. A `let`
//! binding, a `for` loop's iterable and a `signal` initialiser are plain Rust on
//! both halves, so that is where the two host calls are written from. It is also
//! why the handler is spelled `e.target_value()` rather than `e.target.value`:
//! one call, which both halves have.
//!
//! The handler's parameter is left unannotated so that each compiler can give it
//! the event type it has, and its type is then fixed inside the body by a `let`
//! against [`Ev`], which is declared once per target. Naming the type in the
//! parameter instead would name one target's type in both.

/// Where a query is answered.
///
/// A written-down route rather than the procedure's own, because a procedure's
/// id is minted when the macro expands and this file is expanded twice: the
/// server's id and the client's would not be the same one. The route serves the
/// same wire, decoded by the same `serde_args`.
#[cfg(topcoat_client)]
const ENDPOINT: &str = "/demo/search";

/// How long the island waits after the last keystroke before asking.
#[cfg(topcoat_client)]
const DEBOUNCE_MS: f64 = 150.0;

/// The event a handler is handed.
///
/// The browser's compiler hands a handler a borrowed event and the server's
/// hands it an owned one, so the type is declared once per target and the
/// handler names only this. The lifetime is the borrow on the client side and
/// unused on the server's.
#[cfg(topcoat_client)]
type Ev<'a> = &'a ::view_abi::Event;

#[cfg(not(topcoat_client))]
type Ev<'a> = ::topcoat::runtime::Event;

/// The query a fresh island starts with.
///
/// The server's signal holds a `String`, because that is the type its runtime
/// can serialize; the browser's holds a `&'static str`, because its crate has no
/// heap. The view names this rather than a literal so each half gets its own.
#[cfg(topcoat_client)]
fn no_query() -> &'static str {
    ""
}

#[cfg(not(topcoat_client))]
fn no_query() -> String {
    String::new()
}

/// The catalogue entries matching `query`.
///
/// The server renders an island's starting view and has asked nothing, so its
/// answer is the empty list. The browser's answer is the last reply the host
/// holds, and asking for the next one is a side effect of reading this: the
/// reply cannot arrive during the read, so the list it returns is always the one
/// before, and `replies` is what brings it back when the next one lands.
#[cfg(not(topcoat_client))]
fn hits(_query: String, _replies: &::topcoat::runtime::Signal<f64>) -> Vec<&'static str> {
    Vec::new()
}

#[cfg(topcoat_client)]
fn hits(query: &'static str, replies: ::view_abi::Sig<f64>) -> Hits {
    // Read for the subscription and not for the number: this is what makes the
    // list rebuild when an answer lands rather than only when a key is pressed.
    let _ = replies.get();
    unsafe { search_ask(ENDPOINT, query, DEBOUNCE_MS, replies) };
    Hits {
        at: 0.0,
        len: unsafe { search_len() },
    }
}

/// The rows of the last reply, one borrowed string at a time.
///
/// An iterator rather than a collection, because the rows are in the host's
/// parsed JSON and there is nowhere to copy them to.
#[cfg(topcoat_client)]
struct Hits {
    at: f64,
    len: f64,
}

#[cfg(topcoat_client)]
impl Iterator for Hits {
    type Item = &'static str;

    fn next(&mut self) -> Option<&'static str> {
        if self.at >= self.len {
            return None;
        }
        let row = unsafe { search_row(self.at) };
        self.at += 1.0;
        Some(row)
    }
}

// The page lends the island three functions. An `extern "C"` declaration in a
// crate the backend compiles is a call to `__rt.<name>(...)`, and `__rt` is the
// module the import map resolves for this crate, which demo-app serves itself.
#[cfg(topcoat_client)]
#[allow(improper_ctypes)]
unsafe extern "C" {
    /// Asks for `query`, no sooner than `ms` after the last time it changed, and
    /// bumps `replies` when the answer is in.
    ///
    /// Asking twice for the same query does nothing, which is what keeps reading
    /// the answer from asking for it again.
    fn search_ask(url: &str, query: &str, ms: f64, replies: ::view_abi::Sig<f64>);

    /// How many rows the last answer carried.
    fn search_len() -> f64;

    /// One row of it.
    fn search_row(index: f64) -> &'static str;
}

/// A search box whose results come from the server, rendered by the server and
/// taken over by the browser.
#[::view_dom_macro::island]
pub async fn search() -> ::topcoat::Result {
    view! {
        <div class="island">
            signal query = no_query();
            signal replies = 0.0;

            <input
                class="search-input"
                type="text"
                placeholder="search the catalogue"
                @input=$(|e| {
                    let e: Ev = e;
                    query.set(e.target_value())
                })
            >

            <ul class="search-results">
                for item in hits(query.get(), replies) {
                    <li class="search-result">(item)</li>
                }
            </ul>
        </div>
    }
}
