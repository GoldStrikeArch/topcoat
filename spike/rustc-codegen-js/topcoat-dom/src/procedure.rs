//! Calling a procedure from compiled client code.

use alloc::string::{String, ToString};

use crate::fetch::post;
use crate::{Error, Result};

/// Where procedures are served, from `contract/PROCEDURES.md`.
const ROUTE: &str = "/_topcoat/procedures/";

/// The media type that puts a call on the serde wire.
///
/// `topcoat_runtime::procedure::SERDE_CONTENT_TYPE`. The distinct type is what tells a serde call
/// from a surrogate one on the same route, so it is not an `application/json` that happens to work.
const SERDE_CONTENT_TYPE: &str = "application/topcoat+json";

/// Posts `body` to where the procedure `id` is served and answers the reply.
///
/// The item the framework's `#[procedure(serde)]` client expansion awaits, and the one item the
/// server half of this crate cannot have. Two properties are the client runtime's to keep, and
/// both are kept here:
///
/// * **Nothing on the path that builds a view may wait for a call.** Taking over server-rendered
///   markup is one uninterrupted pass, so this is only ever reached from an event handler or an
///   effect. `view_async` says the same at more length.
/// * **A call that suspends resumes with no tracking context**, so whatever touches reactive values
///   afterwards must have captured its owner before suspending. `view_async::spawn` does that, and
///   this future is only ever driven by it.
///
/// A dropped call does not cancel its request. Nothing in the pinned runtime cancels anything and
/// a host promise has no counterpart, so the request finishes and its answer is discarded, which is
/// what the existing browser client does too.
///
/// # Errors
///
/// Returns the reason the request failed, as the host reported it.
pub async fn call(id: &str, body: &str) -> Result<String> {
    // Concatenated rather than formatted: `format!` pulls in the whole of `core::fmt` for a join
    // of two strings, and an island pays for every byte of it.
    let mut url = String::with_capacity(ROUTE.len() + id.len());
    url.push_str(ROUTE);
    url.push_str(id);

    let promise = post(&url, SERDE_CONTENT_TYPE, body);
    // The reply crosses as a `&str` because a JavaScript string IS one in this value model; it is
    // copied into an owned `String` because the caller is handed something that outlives the call.
    match view_async::await_promise::<&str>(promise).await {
        Ok(reply) => Ok(reply.to_string()),
        Err(reason) => Err(Error::new(reason)),
    }
}
