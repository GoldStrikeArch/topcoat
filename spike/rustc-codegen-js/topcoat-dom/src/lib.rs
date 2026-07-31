//! The client half of the `topcoat-dom` seam: what a crate compiled for the browser calls into.
//!
//! `crates/topcoat-dom` in the framework repository is the SERVER half of the same crate, and its
//! own documentation says why one item is missing from it: `procedure::call` posts a body to where
//! a procedure is served, and a server has nothing to answer itself with. This is the other half,
//! and it exists so that the client expansion of `#[procedure(serde)]` can be compiled and run
//! rather than reasoned about.
//!
//! # What the expansion needs, and nothing more
//!
//! The framework's macro emits exactly this, and every name in it is here:
//!
//! ```text
//! async fn search(query: String) -> ::topcoat_dom::Result<<Result<Hits> as ::topcoat_dom::ResultExt>::T> {
//!     let __topcoat_reply = ::topcoat_dom::procedure::call(ID, &::topcoat_dom::json::to_json(&(&query,))?).await?;
//!     ::topcoat_dom::json::from_json(&__topcoat_reply)
//! }
//! ```
//!
//! # The JSON is not serde's, and that is the one gap
//!
//! The server half's `json` module is `serde_json`. Compiling `serde` and `serde_json` through this
//! backend is a probe of its own (it needs cargo to build them, and the fixture harness compiles
//! single files), so the module here is a small one with traits of its own. The expansion does not
//! name the bound -- it calls `to_json` and `from_json` and nothing else -- so it compiles unchanged
//! against either, and what stays unverified is serde itself rather than the seam.

#![no_std]

extern crate alloc;

mod error;
mod fetch;

pub mod json;
pub mod procedure;

pub use error::*;
