//! Support for views a client renders for itself.
//!
//! Turning on the `dom` feature is what makes `view!` lower a view for the DOM
//! as well as for the server, and this module is what the code that lowering
//! generates calls into. Nothing here is called by hand.

pub use topcoat_dom::*;
