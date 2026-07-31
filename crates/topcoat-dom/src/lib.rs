#![cfg_attr(docsrs, feature(doc_cfg))]
//! Support for views a client renders for itself.
//!
//! A view compiled for the DOM is the same `view!` body the server renders,
//! lowered by a second emitter into the operations a client runs. This crate is
//! the server-side half of that: the values a rendered view carries so a client
//! can find its nodes again, and the code that serves what the client needs. The
//! client-side half is a separate crate that is compiled for the client target
//! alone, which is why it does not live here.
//!
//! # Where the pieces live
//!
//! Lowering a view for the DOM is split by what each part needs to know, and the
//! split is what keeps the two emitters honest about numbering the same view the
//! same way.
//!
//! - `topcoat-view-grammar`, behind its `dom` feature, holds everything about a view's shape that
//!   both emitters agree on: the key plan that numbers every position a generated name or a
//!   hydration key belongs to, and the writers that lower a view against it. Both emitters read one
//!   plan, so neither counts for itself.
//! - `topcoat-view` holds the rendered form: the hydration sites a view writes, the island and
//!   component nesting its keys are scoped by, and the wrapper element a client finds an island in.
//! - `topcoat-view-macro` is where `view!` picks the emitter that matches the target it is compiled
//!   for.
//! - `topcoat-runtime` holds the constructs that are written once and compiled twice, beside the
//!   ones it already owns.
//! - This crate holds the server-side support the generated code calls into, and serves the script
//!   a page needs to start a client.
//!
//! The client-side ABI the second emitter targets, and the client half of this
//! crate, are compiled for the client target and distributed with the compiler
//! that reads that ABI, not with these crates.
//!
//! # What the server-side pieces are
//!
//! - The numbering both emitters agree on lives on the rendered form in `topcoat-view`:
//!   `View::component` scopes a component's keys under the ordinal its call took, and
//!   `View::component_with_child` renders child content in the caller's numbering before the call
//!   takes that ordinal, handing the component's own view the `View::child_content` stand-in that
//!   writes the result.
//! - A procedure declared on the serde wire is served by `topcoat-runtime`, which decodes its
//!   arguments with `serde_args` from a body sent as `SERDE_CONTENT_TYPE`.
//!
//! # The seam a procedure is called through
//!
//! Generated client code names this crate, so both halves carry the seam below
//! under the same paths. A procedure declared on the serde wire expands, for the
//! client target, to a plain async function whose body is a call through it.
//!
//! - [`json::to_json`] and [`json::from_json`], over `serde::Serialize` and
//!   `serde::de::DeserializeOwned`. Bounding the seam on serde's own traits is what lets the
//!   implementation be a full JSON library or a hand-written encoder without the generated code
//!   changing.
//! - [`ResultExt`], an associated type naming the `Ok` type of a `Result`, so a procedure's return
//!   type still resolves when it was written through an alias.
//! - [`Result`] and [`Error`] of this crate's own, rather than the framework's: a client has no
//!   request context to carry an error through.
//!
//! One item of the seam is the client half's alone: `procedure::call(id: &str, body: &str)`, which
//! posts `body` to where the procedure `id` is served and resolves to the reply. It is not here
//! because making a request is what a client does and a server has nothing to answer it with. Only
//! that the call is awaitable is fixed: whether the future is compiled from an `async fn` or
//! adapted from a callback is the client runtime's own business, which is what keeps the expansion
//! the same either way. Two constraints come with it, and both are the client runtime's to keep
//! rather than the expansion's: taking over server-rendered markup is one uninterrupted pass, so
//! nothing on the path that builds a view may wait for a call; and a call that suspends resumes
//! with no tracking context, so whatever touches reactive values after it has to have captured its
//! owner before suspending.

mod error;

pub mod json;

pub use error::*;
