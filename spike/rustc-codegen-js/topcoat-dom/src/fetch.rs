//! The one request a procedure client makes.

use js_macro::js;
use view_abi::JsValue;

/// Posts `body` to `url` with the procedure media type and answers a promise of the reply text, or
/// one that rejects with a message.
///
/// The request is written as a `js!{}` block rather than declared as a host function, and the
/// reason is the whole reason the macro exists: `fetch(url, init)` takes an options object holding
/// `{ method, headers: { "content-type": .. }, body }`, and the value model makes a `#[repr(C)]`
/// struct an object keyed by FIELD NAME. `content-type` is not a Rust field name, so a declared
/// interface cannot build the headers object, and this call used to be one host function standing
/// in for a shape Rust could not spell. The block spells it, and the request shape is now stated
/// where it is built rather than on the host side of a name.
///
/// `url`, `content_type` and `body` are the captures. `fetch` is reached through `globalThis`,
/// `r` is bound by the arrow, and everything else is a key or a property, so nothing else crosses.
pub fn post(url: &str, content_type: &str, body: &str) -> JsValue {
    js! {
        globalThis.fetch(url, {
            method: "POST",
            headers: { "content-type": content_type },
            body,
        }).then(r => {
            // A status the server rejected on is an `Err` the caller can render, which is what a
            // reply parsed as JSON would not have been.
            if (!r.ok) {
                throw "the procedure returned " + r.status;
            }
            return r.text();
        })
    }
}
