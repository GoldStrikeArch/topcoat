//! Lowers a parsed `view!` body to the template ABI the codegen backend reads.
//!
//! A view becomes one [`Template`] per element that no other element encloses:
//! an HTML skeleton that survives an `innerHTML` round trip, a hole per dynamic
//! position located by a `firstChild`/`nextSibling` walk, and the Rust
//! expressions that fill them. Control flow nests: each branch of an `if` or
//! `match`, and the row of a `for`, is a template of its own, instantiated
//! inside the closure that fills the enclosing hole. A branch body starts a
//! template the way a fresh view does, which is how the key plan walks one too,
//! and the two have to agree site for site.
//!
//! Only the constructs the emitter can lower are handled so far; every other
//! node reports a spanned error rather than emitting something wrong.
//!
//! # What the backend is left to do
//!
//! The emitter stops where a decision needs types or the target's own tables,
//! and hands the backend what it cannot work out for itself.
//!
//! - **Memo hoisting.** An `if` hands its test over as a closure of its own, through
//!   `::view_abi::cond`, and the backend is what wraps it in `_$memo` so an unrelated signal does
//!   not rebuild the branch. A `match`, and an `if` whose test binds, stay one closure the backend
//!   re-runs: their test is a pattern and an arm's body reads what it bound, so there is nothing to
//!   hand over separately.
//! - **The rows of a `for`.** The loop stays Rust: `::view_abi::list` starts the list, one
//!   `::view_abi::push` per row appends to it, and the list of nodes is what the hole is filled
//!   with. What the backend adds is the JavaScript array they become.
//! - **Effect grouping.** Reactive holes are emitted one `::view_abi::effect` call each. Holes that
//!   share an effect group belong to one element and are meant to become a single effect carrying a
//!   previous-value record; a group of one needs no record.
//! - **Marker semantics.** A hydratable template brackets each dynamic child of a multi-child
//!   element with `<!$>`/`<!/>`, and the hole's walk reaches the opening marker. The backend hands
//!   the node after it to `_$getNextMarker` and passes both results to `_$insert`.
//! - **Empty content.** A branch that renders nothing is `::view_abi::content(())`, which the
//!   backend lowers to an insert of nothing rather than to a template with no HTML.
//!
//! # Where this differs from upstream
//!
//! Outside hydratable mode, upstream emits a `<!>` anchor only for an
//! expression sandwiched between two text nodes, and lets adjacent expressions
//! share one; every other dynamic child is inserted before the next walked
//! sibling. This emitter anchors every dynamic child of a multi-child element
//! instead. Both are correct, and the contract records the anchor-per-
//! expression form as such, but the templates are not byte identical to the
//! reference output. The hydratable path, where the server and the client have
//! to agree node for node, follows upstream exactly.

mod client_component;
mod common;
mod contract;
mod dom_writer;
mod island;
mod lower;
mod template;

pub use client_component::*;
pub use contract::*;
pub use dom_writer::*;
pub use island::*;
pub use template::*;
