//! What the runtime writes inside an island, which is nothing.
//!
//! An island's subtree belongs to whatever hydrates it. This runtime is not
//! that, so it writes none of its own markers there: the browser scanner skips
//! an island subtree, which would make them dead weight on its own, and the
//! comment nodes a marker adds would move the nodes the island's own client
//! walks to.
//!
//! The value a marker was carrying is still rendered. A runtime expression
//! inside an island renders as its evaluated value and a bound attribute as its
//! evaluated attribute, so the markup the server sends is the markup the island
//! is server-rendered as.

use topcoat_core::context::Cx;

/// Returns `true` while the render is inside an island.
///
/// Views are built as a sequential chain of awaits, so the island that is
/// current while a part is built is the island that part belongs to.
#[inline]
#[must_use]
pub fn in_island(cx: &Cx) -> bool {
    cx.islands().current().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_inside_an_island_by_default() {
        assert!(!in_island(&Cx::default()));
    }

    #[test]
    fn entering_an_island_is_what_makes_it_current() {
        let cx = Cx::default();
        let islands = cx.islands();
        let guard = islands.enter(islands.next_instance());
        assert!(in_island(&cx));
        drop(guard);
        assert!(!in_island(&cx));
    }
}
