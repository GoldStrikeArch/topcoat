use std::{iter::empty, ops::Deref};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use topcoat_core::context::Cx;
use topcoat_view::{NodeViewParts, PartsWriter};
use uuid::Uuid;

use crate::id::Id;
use crate::{Surrogate, Surrogated};

/// The identity a signal is registered under on the client.
///
/// Inside an island the id is derived from the island instance and the signal's
/// position in it, so a client that walks the same view predicts it. Everywhere
/// else it is random, because nothing has to guess it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalId(Id);

impl SignalId {
    /// A fresh id that no other signal shares.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self(Id::Random(Uuid::new_v4()))
    }

    /// The id for the next signal declared while rendering `cx`.
    ///
    /// Signals are numbered by the order they are declared in, which the server
    /// and the client agree on because a view renders as a sequential chain of
    /// awaits.
    #[must_use]
    pub fn next(cx: &Cx) -> Self {
        match cx.islands().current() {
            Some(island) => Self(Id::Island {
                instance: island.instance(),
                ordinal: island.next_signal(),
            }),
            None => Self::new(),
        }
    }
}

impl Default for SignalId {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SignalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for SignalId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SignalId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Id::deserialize(deserializer).map(Self)
    }
}

#[derive(Debug)]
pub struct Signal<T> {
    id: SignalId,
    value: T,
}

impl<T> Signal<T> {
    #[inline]
    pub fn new(value: T) -> Self {
        Self {
            id: SignalId::new(),
            value,
        }
    }

    /// Creates a signal whose id is numbered against the island `cx` is
    /// rendering, or random outside every island.
    #[inline]
    pub fn new_in(cx: &Cx, value: T) -> Self {
        Self {
            id: SignalId::next(cx),
            value,
        }
    }

    pub(crate) fn id(&self) -> SignalId {
        self.id
    }

    pub(crate) fn read(&self) -> &T {
        &self.value
    }
}

impl<T> Signal<T>
where
    T: Clone,
{
    /// The value the signal was declared with.
    ///
    /// The server renders a view once, so this is what the view is rendered
    /// from. It is public because a view reads a signal in two kinds of place: a
    /// runtime expression, which reads it through the surrogate that also
    /// compiles to a browser read, and plain Rust such as a `for` loop's
    /// iterable, which is compiled by whatever compiler is reading the view and
    /// sees the signal itself.
    #[must_use]
    pub fn get(&self) -> T {
        self.value.clone()
    }
}

pub struct SignalDeclaration<'a, T>(&'a Signal<T>);

impl<'a, T> SignalDeclaration<'a, T> {
    #[inline]
    pub fn new(signal: &'a Signal<T>) -> Self {
        Self(signal)
    }
}

impl<T> NodeViewParts for SignalDeclaration<'_, T>
where
    for<'a> &'a T: Surrogated,
    for<'a> <&'a T as Surrogated>::Surrogate: Serialize,
{
    fn into_view_parts(self, cx: &Cx, parts: &mut PartsWriter<'_>) {
        #[derive(Serialize)]
        struct SignalDeclarationPayload<'a, V>
        where
            V: ?Sized,
        {
            t: &'static str,
            id: std::string::String,
            v: &'a V,
        }

        // Inside an island the declaration is the island's to make, and the
        // comment this would write would be a node its client does not expect.
        if crate::in_island(cx) {
            return;
        }

        let value = (&self.0.value).into_surrogate();
        let payload = SignalDeclarationPayload {
            t: "signal",
            id: self.0.id().to_string(),
            v: &value,
        };
        let json = serde_json::to_string(&payload)
            .expect("failed to serialize signal declaration payload");

        parts.push_comment(|comment| {
            // `json` is untrusted application data and must be escaped to prevent XSS attacks.
            comment
                .push_str_unescaped("::topcoat::signal(")
                .push_str(json)
                .push_str_unescaped(")");
        });
    }
}

#[derive(Debug, Clone)]
pub struct ReadSignal<T> {
    id: SignalId,
    value: T,
}

impl<T> ReadSignal<T> {
    pub fn new(signal: &Signal<T>) -> Self
    where
        T: Clone,
    {
        Self {
            id: signal.id,
            value: signal.value.clone(),
        }
    }

    pub fn id(&self) -> SignalId {
        self.id
    }
}

impl<T> Deref for ReadSignal<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<'de, T> Deserialize<'de> for ReadSignal<T>
where
    T: Surrogated,
    T::Surrogate: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct EncodedReadSignal<S> {
            id: SignalId,
            value: S,
        }

        let encoded = EncodedReadSignal::<T::Surrogate>::deserialize(deserializer)?;
        Ok(Self {
            id: encoded.id,
            value: encoded.value.into_real(),
        })
    }
}

pub trait Signals: Sized {
    fn ids(&self) -> impl Iterator<Item = SignalId>;
    fn decode(encoded_signals: EncodedSignals) -> Self;
}

impl Signals for () {
    fn ids(&self) -> impl Iterator<Item = SignalId> {
        empty()
    }

    fn decode(_encoded_signals: EncodedSignals) -> Self {}
}

macro_rules! impl_signals_for_tuple {
    ($($n:tt $t:ident),+) => {
        impl<$($t),+> Signals for ($(ReadSignal<$t>,)+)
        where
            $(
                $t: Surrogated,
                <$t as Surrogated>::Surrogate: DeserializeOwned,
            )+
        {
            fn ids(&self) -> impl Iterator<Item = SignalId> {
                [$(self.$n.id),+].into_iter()
            }

            fn decode(encoded_signals: EncodedSignals) -> Self {
                serde_json::from_str(&encoded_signals.0).unwrap()
            }
        }
    };
}

impl_signals_for_tuple!(0 T0);
impl_signals_for_tuple!(0 T0, 1 T1);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6, 7 T7);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6, 7 T7, 8 T8);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6, 7 T7, 8 T8, 9 T9);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6, 7 T7, 8 T8, 9 T9, 10 T10);
impl_signals_for_tuple!(0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6, 7 T7, 8 T8, 9 T9, 10 T10, 11 T11);

pub struct EncodedSignals(String);

impl EncodedSignals {
    pub fn new(inner: impl Into<String>) -> Self {
        Self(inner.into())
    }
}

#[cfg(test)]
mod tests {
    use topcoat_view::{HtmlContext, PartsWriter, View, ViewParts};

    use super::*;

    #[test]
    fn payload_cannot_terminate_the_comment() {
        // A value carrying `-->`, a quote, and an ampersand: the characters
        // that could break out of the comment or corrupt its JSON payload.
        let signal = Signal::new(String::from("a-->b\"c&d"));

        let cx = Cx::default();
        let mut parts = ViewParts::new();
        SignalDeclaration::new(&signal)
            .into_view_parts(&cx, &mut PartsWriter::new(&mut parts, HtmlContext::Text));
        let html = View::new(parts).render(&cx);

        // The comment context escaped `>`, so the only `-->` left is the
        // marker's own terminator; the payload cannot end the comment early.
        assert_eq!(html.matches("-->").count(), 1);
        assert!(html.ends_with(") -->"));
        assert!(html.contains("--&gt;"));
        // The JSON's own quotes round-trip as entities the client decodes.
        assert!(html.contains("&quot;"));
    }

    #[test]
    fn ids_outside_an_island_stay_random() {
        let cx = Cx::default();
        let first = Signal::new_in(&cx, 0).id();
        let second = Signal::new_in(&cx, 0).id();

        assert_ne!(first, second);
        assert_eq!(first.to_string().len(), 36, "{first}");
    }

    #[test]
    fn ids_inside_an_island_are_numbered_by_declaration_order() {
        let cx = Cx::default();
        let _island = cx.islands().enter(cx.islands().next_instance());

        assert_eq!(Signal::new_in(&cx, 0).id().to_string(), "i0.0");
        assert_eq!(Signal::new_in(&cx, 0).id().to_string(), "i0.1");
    }

    #[test]
    fn each_island_numbers_its_signals_from_zero() {
        let cx = Cx::default();
        {
            let _first = cx.islands().enter(cx.islands().next_instance());
            assert_eq!(Signal::new_in(&cx, 0).id().to_string(), "i0.0");
        }
        let _second = cx.islands().enter(cx.islands().next_instance());
        assert_eq!(Signal::new_in(&cx, 0).id().to_string(), "i1.0");
    }

    #[test]
    fn both_kinds_of_id_round_trip_through_json() {
        let cx = Cx::default();
        for id in [SignalId::new(), {
            let _island = cx.islands().enter(cx.islands().next_instance());
            SignalId::next(&cx)
        }] {
            let json = serde_json::to_string(&id).unwrap();
            assert_eq!(json, format!("\"{id}\""));
            assert_eq!(serde_json::from_str::<SignalId>(&json).unwrap(), id);
        }
    }
}
