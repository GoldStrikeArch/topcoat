//! Island instance allocation for a request.
//!
//! An island is a subtree that is rendered on the server and then hydrated on
//! the client. Every node the client has to find again carries a hydration key,
//! and every key is scoped to the island instance that produced it, so a page
//! with several islands hydrates each one without the keys of one colliding
//! with another.

use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, PoisonError, RwLock};

use crate::context::Cx;

/// One island instance within a request.
///
/// Instances are handed out by [`Islands::next_instance`] in the order the
/// islands render, so the first island on a page is `i0`, the second `i1`, and
/// so on. The identity is stable for a single render of a single request and
/// means nothing across requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IslandInstance(u32);

impl IslandInstance {
    /// The instance at position `index` in a request.
    ///
    /// Reconstructs an instance from an id that was rendered earlier; allocate
    /// a new one with [`Islands::next_instance`] instead.
    #[inline]
    #[must_use]
    pub fn from_index(index: u32) -> Self {
        Self(index)
    }

    /// The instance's position in the request, counting from zero.
    #[inline]
    #[must_use]
    pub fn index(self) -> u32 {
        self.0
    }

    /// The prefix that every hydration key minted inside this island starts
    /// with.
    ///
    /// A client hydrates one island by keeping only the keys that start with
    /// this prefix, so the trailing separator is part of it: matching on `i1`
    /// alone would also match every key of `i10`.
    #[must_use]
    pub fn key_prefix(self) -> String {
        format!("{self}.")
    }

    /// Writes the hydration key for `ordinal` inside this island.
    ///
    /// The key is [`key_prefix`](Self::key_prefix), a letter, and the ordinal,
    /// which is the position the site holds in the view's key plan. The letter
    /// encodes how many digits the ordinal has, and is left out for a single
    /// digit: ordinal 9 is `i0.9` and ordinal 10 is `i0.a10`.
    ///
    /// That letter is what makes keys prefix free, so a key stays readable when
    /// something is concatenated after it: without it, `i0.1` would be a prefix
    /// of `i0.10`. The client mints the keys it looks up with, so this is not a
    /// choice: it is the encoding the client runtime uses, and a server that
    /// writes anything else describes nodes the client will not ask for.
    ///
    /// # Errors
    ///
    /// Returns whatever error `out` reports while being written to.
    pub fn write_key(self, out: &mut impl fmt::Write, ordinal: u32) -> fmt::Result {
        self.write_key_in(out, &KeyContext::ROOT, ordinal)
    }

    /// The hydration key for `ordinal` inside this island.
    #[must_use]
    pub fn key(self, ordinal: u32) -> String {
        self.key_in(&KeyContext::ROOT, ordinal)
    }

    /// Writes the hydration key for `ordinal` inside `context` in this island.
    ///
    /// The context sits between the island's prefix and the ordinal, so a key
    /// minted in the root context is exactly what [`write_key`](Self::write_key)
    /// produces.
    ///
    /// # Errors
    ///
    /// Returns whatever error `out` reports while being written to.
    pub fn write_key_in(
        self,
        out: &mut impl fmt::Write,
        context: &KeyContext,
        ordinal: u32,
    ) -> fmt::Result {
        write!(
            out,
            "{self}.{}{}{ordinal}",
            context.as_str(),
            Self::length_letter(ordinal),
        )
    }

    /// The hydration key for `ordinal` inside `context` in this island.
    #[must_use]
    pub fn key_in(self, context: &KeyContext, ordinal: u32) -> String {
        format!(
            "{self}.{}{}{ordinal}",
            context.as_str(),
            Self::length_letter(ordinal),
        )
    }

    /// The instance and ordinal a key was written from, if `key` is one.
    ///
    /// The inverse of [`key`](Self::key), and strict about it: a string that is
    /// not exactly what `key` would have produced, such as one carrying the
    /// wrong letter for its digit count, is not a key. Only a key minted in the
    /// root context parses, which is every key that names a value numbered
    /// against the island itself rather than against a component inside it.
    #[must_use]
    pub fn parse_key(key: &str) -> Option<(Self, u32)> {
        let (instance, ordinal) = key.split_once('.')?;
        let instance = Self(instance.strip_prefix('i')?.parse().ok()?);
        let digits = ordinal
            .strip_prefix(|letter: char| letter.is_ascii_lowercase())
            .unwrap_or(ordinal);
        let ordinal = digits.parse().ok()?;
        // Re-encoding is the whole check: it rejects a wrong letter, a leading
        // zero, and anything else that reads as this pair without being it.
        (instance.key(ordinal) == key).then_some((instance, ordinal))
    }

    /// The letter recording how many digits `ordinal` is written with.
    ///
    /// One letter per digit past the first, of which a [`u32`] has at most nine.
    fn length_letter(ordinal: u32) -> &'static str {
        const LETTERS: [&str; 10] = ["", "a", "b", "c", "d", "e", "f", "g", "h", "i"];
        let past_the_first = ordinal.checked_ilog10().unwrap_or(0) as usize;
        LETTERS[past_the_first]
    }
}

impl fmt::Display for IslandInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "i{}", self.0)
    }
}

/// The component nesting a hydration key is minted inside.
///
/// A component renders a view of its own, which numbers its keys from zero the
/// way the island's own view does. What keeps a component's keys from colliding
/// with the keys around it is a context: calling the component takes the next
/// ordinal from the numbering its caller had reached, and that ordinal becomes
/// the context every key inside the component is written in.
///
/// The context of an island's own view is [`ROOT`](Self::ROOT), which is empty,
/// so a view that calls no component writes exactly the keys
/// [`IslandInstance::key`] produces.
///
/// ```
/// use topcoat_core::island::{IslandInstance, KeyContext};
///
/// let island = IslandInstance::from_index(0);
/// // The island's view claims ordinal 0, then calls a component at ordinal 1.
/// assert_eq!(island.key_in(&KeyContext::ROOT, 0), "i0.0");
/// let inside = KeyContext::ROOT.enter(1);
/// // The component's own view starts again from zero, under that ordinal.
/// assert_eq!(island.key_in(&inside, 0), "i0.10");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyContext(String);

impl KeyContext {
    /// The context of an island's own view, which every key of a view that
    /// calls no component is minted in.
    pub const ROOT: Self = Self(String::new());

    /// Returns `true` for the context of an island's own view.
    #[inline]
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The context nested inside this one at `ordinal`, which is the ordinal the
    /// caller's numbering had reached when it called the component.
    #[must_use]
    pub fn enter(&self, ordinal: u32) -> Self {
        Self(format!(
            "{}{}{ordinal}",
            self.0,
            IslandInstance::length_letter(ordinal),
        ))
    }

    /// The context as it appears in a key, between the island's prefix and the
    /// ordinal.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The island state of one request.
///
/// Reach it through [`Cx::islands`]. It hands out an [`IslandInstance`] per
/// island rendered and tracks which island is rendering right now, so the
/// values built along the way (signals, reactive scopes, hydration keys) can
/// number themselves against it.
///
/// # The sequential render invariant
///
/// Numbering is positional: an island's contents are numbered by the order in
/// which they are built, and the server and the client agree only because both
/// walk the view in the same order. A Topcoat view renders as a sequential
/// chain of awaits, so exactly one island is being built at any moment and the
/// order is the document order. Rendering two views concurrently (joining or
/// spawning them) would interleave their allocations and silently produce keys
/// that no longer describe the document. [`enter`](Self::enter) checks what it
/// can: its guard asserts on drop that the island being left is the one it
/// entered, which catches interleaving in debug builds.
#[derive(Debug, Default)]
pub struct Islands {
    next: AtomicU32,
    current: RwLock<Option<Arc<IslandScope>>>,
}

impl Islands {
    /// Allocates the next island instance for this request.
    pub fn next_instance(&self) -> IslandInstance {
        IslandInstance(self.next.fetch_add(1, Ordering::Relaxed))
    }

    /// Makes `instance` the island being rendered until the returned guard is
    /// dropped.
    ///
    /// Entering restores the previously current island on drop, so an island
    /// nested inside another leaves the outer one intact.
    pub fn enter(&self, instance: IslandInstance) -> IslandGuard<'_> {
        let scope = Arc::new(IslandScope::new(instance));
        let previous = self.replace(Some(Arc::clone(&scope)));
        IslandGuard {
            islands: self,
            previous,
            entered: instance,
        }
    }

    /// The island being rendered, or `None` outside every island.
    #[must_use]
    pub fn current(&self) -> Option<Arc<IslandScope>> {
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn replace(&self, scope: Option<Arc<IslandScope>>) -> Option<Arc<IslandScope>> {
        let mut current = self.current.write().unwrap_or_else(PoisonError::into_inner);
        std::mem::replace(&mut current, scope)
    }
}

/// The numbering state of one island instance while it renders.
#[derive(Debug)]
pub struct IslandScope {
    instance: IslandInstance,
    next_signal: AtomicU32,
    next_reactive_scope: AtomicU32,
}

impl IslandScope {
    fn new(instance: IslandInstance) -> Self {
        Self {
            instance,
            next_signal: AtomicU32::new(0),
            next_reactive_scope: AtomicU32::new(0),
        }
    }

    /// The instance this scope numbers for.
    #[inline]
    #[must_use]
    pub fn instance(&self) -> IslandInstance {
        self.instance
    }

    /// The ordinal for the next signal declared in this island.
    pub fn next_signal(&self) -> u32 {
        self.next_signal.fetch_add(1, Ordering::Relaxed)
    }

    /// The ordinal for the next reactive scope opened in this island.
    pub fn next_reactive_scope(&self) -> u32 {
        self.next_reactive_scope.fetch_add(1, Ordering::Relaxed)
    }
}

/// Keeps an island current for as long as it is held.
///
/// Returned by [`Islands::enter`].
#[derive(Debug)]
pub struct IslandGuard<'a> {
    islands: &'a Islands,
    previous: Option<Arc<IslandScope>>,
    entered: IslandInstance,
}

impl IslandGuard<'_> {
    /// The instance this guard made current.
    #[inline]
    #[must_use]
    pub fn instance(&self) -> IslandInstance {
        self.entered
    }
}

impl Drop for IslandGuard<'_> {
    fn drop(&mut self) {
        let restored = self.islands.replace(self.previous.take());
        debug_assert_eq!(
            restored.map(|scope| scope.instance()),
            Some(self.entered),
            "left island {} while a different island was current: views must render as a \
             sequential chain of awaits, never joined or spawned",
            self.entered,
        );
    }
}

/// The island state of the request `cx` belongs to.
#[inline]
#[must_use]
pub fn islands(cx: &Cx) -> &Islands {
    cx.islands()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instances_are_numbered_in_allocation_order() {
        let islands = Islands::default();
        let instances = [
            islands.next_instance(),
            islands.next_instance(),
            islands.next_instance(),
        ];
        assert_eq!(
            instances.map(|instance| instance.to_string()),
            ["i0", "i1", "i2"],
        );
    }

    #[test]
    fn keys_are_the_instance_and_the_ordinal() {
        let islands = Islands::default();
        assert_eq!(islands.next_instance().key(3), "i0.3");
    }

    #[test]
    fn an_ordinal_past_one_digit_carries_its_length() {
        let instance = IslandInstance::from_index(0);
        let keys: Vec<_> = [0, 9, 10, 99, 100, 999, 1000, u32::MAX]
            .map(|ordinal| instance.key(ordinal))
            .to_vec();
        assert_eq!(
            keys,
            [
                "i0.0",
                "i0.9",
                "i0.a10",
                "i0.a99",
                "i0.b100",
                "i0.b999",
                "i0.c1000",
                "i0.i4294967295",
            ],
        );
    }

    #[test]
    fn keys_are_prefix_free_across_the_ten_boundary() {
        let instance = IslandInstance::from_index(0);
        // The reason for the letter: a key must not read as the start of
        // another one, or a walk that concatenates them cannot be undone.
        for shorter in 0..12u32 {
            for longer in 0..40u32 {
                assert!(
                    shorter == longer || !instance.key(longer).starts_with(&instance.key(shorter)),
                    "key({longer}) starts with key({shorter})",
                );
            }
        }
    }

    #[test]
    fn the_root_context_writes_the_keys_the_island_writes() {
        let instance = IslandInstance::from_index(0);
        assert!(KeyContext::ROOT.is_root());
        for ordinal in [0, 9, 10, 100, 1234] {
            assert_eq!(
                instance.key_in(&KeyContext::ROOT, ordinal),
                instance.key(ordinal)
            );
        }
    }

    #[test]
    fn entering_a_context_nests_the_ordinal_that_opened_it() {
        // The nesting the client reaches through `nextHydrateContext`: the
        // ordinal a component call took becomes the prefix of every key the
        // component mints, and the component starts again from zero.
        let instance = IslandInstance::from_index(0);
        let first = KeyContext::ROOT.enter(0);
        let second = first.enter(0);
        let third = second.enter(0);
        assert_eq!(
            [first.as_str(), second.as_str(), third.as_str(),],
            ["0", "00", "000"],
        );
        assert_eq!(instance.key_in(&third, 0), "i0.0000");
        assert!(!first.is_root());
    }

    #[test]
    fn a_nested_context_carries_the_length_letter_of_each_segment() {
        let instance = IslandInstance::from_index(3);
        let context = KeyContext::ROOT.enter(1).enter(10).enter(100);
        assert_eq!(context.as_str(), "1a10b100");
        assert_eq!(instance.key_in(&context, 100_000), "i3.1a10b100e100000");
    }

    #[test]
    fn a_nested_key_never_reads_as_a_key_of_the_context_around_it() {
        // The letter is what makes the segments separable: without it the
        // component at ordinal 1 and the island's own ordinal 10 would write
        // the same key.
        let instance = IslandInstance::from_index(0);
        let inside = KeyContext::ROOT.enter(1);
        assert_eq!(instance.key_in(&inside, 0), "i0.10");
        assert_eq!(instance.key(10), "i0.a10");
        for outer in 0..12u32 {
            for inner in 0..12u32 {
                let nested = instance.key_in(&KeyContext::ROOT.enter(outer), inner);
                for ordinal in 0..40u32 {
                    assert_ne!(nested, instance.key(ordinal));
                }
            }
        }
    }

    #[test]
    fn a_key_parses_back_to_what_wrote_it() {
        let instance = IslandInstance::from_index(7);
        for ordinal in [0, 1, 9, 10, 42, 100, 1234] {
            let key = instance.key(ordinal);
            assert_eq!(IslandInstance::parse_key(&key), Some((instance, ordinal)));
        }
    }

    #[test]
    fn a_string_that_is_not_a_key_does_not_parse_as_one() {
        for encoded in [
            "", "i0", "0.1", "ix.1", "i0.x",
            "i0.1.2", // The letter has to agree with the digit count.
            "i0.a1", "i0.10", "i0.b10", // And the ordinal has to be written the one way.
            "i0.01", "i00.1",
        ] {
            assert!(
                IslandInstance::parse_key(encoded).is_none(),
                "`{encoded}` should not parse as a key",
            );
        }
    }

    #[test]
    fn a_key_prefix_keeps_neighbouring_instances_apart() {
        let islands = Islands::default();
        let first = islands.next_instance();
        for _ in 0..9 {
            islands.next_instance();
        }
        let tenth = islands.next_instance();
        assert_eq!(tenth.to_string(), "i10");

        // Without the separator, `i1` would also match every key of `i10`,
        // which is what the client's prefix filter would pull into the wrong
        // island.
        assert!(!tenth.key(0).starts_with(&first.key_prefix()));
        assert!(tenth.key(0).starts_with(&tenth.key_prefix()));
    }

    #[test]
    fn no_island_is_current_by_default() {
        let islands = Islands::default();
        assert!(islands.current().is_none());
    }

    #[test]
    fn entering_makes_an_island_current_until_the_guard_drops() {
        let islands = Islands::default();
        let instance = islands.next_instance();
        {
            let _guard = islands.enter(instance);
            assert_eq!(
                islands.current().map(|scope| scope.instance()),
                Some(instance),
            );
        }
        assert!(islands.current().is_none());
    }

    #[test]
    fn a_nested_island_restores_the_one_around_it() {
        let islands = Islands::default();
        let outer = islands.next_instance();
        let outer_guard = islands.enter(outer);

        let inner = islands.next_instance();
        {
            let _inner_guard = islands.enter(inner);
            assert_eq!(islands.current().map(|scope| scope.instance()), Some(inner));
        }

        assert_eq!(islands.current().map(|scope| scope.instance()), Some(outer));
        drop(outer_guard);
        assert!(islands.current().is_none());
    }

    #[test]
    fn each_island_numbers_its_own_signals_and_scopes() {
        let islands = Islands::default();

        let first = islands.enter(islands.next_instance());
        let scope = islands.current().unwrap();
        assert_eq!([scope.next_signal(), scope.next_signal()], [0, 1]);
        assert_eq!(scope.next_reactive_scope(), 0);
        drop(first);

        let _second = islands.enter(islands.next_instance());
        let scope = islands.current().unwrap();
        assert_eq!(scope.next_signal(), 0);
    }

    #[test]
    fn islands_are_reachable_from_a_context() {
        let cx = Cx::default();
        assert_eq!(islands(&cx).next_instance().to_string(), "i0");
        assert_eq!(cx.islands().next_instance().to_string(), "i1");
    }
}
