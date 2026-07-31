use serde::Serialize;
use topcoat_core::context::Cx;
use topcoat_view::{NodeViewParts, PartsWriter, View, ViewPart};
use uuid::Uuid;

use crate::id::Id;
use crate::{SHARD_ROUTE_PREFIX, ShardId};

/// The identity a reactive scope's markers carry.
///
/// Inside an island the id is derived from the island instance and the scope's
/// position in it; everywhere else it is random. See [`SignalId`](crate::SignalId).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReactiveScopeId(Id);

impl ReactiveScopeId {
    /// A fresh id that no other scope shares.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self(Id::Random(Uuid::new_v4()))
    }

    /// The id for the next reactive scope rendered against `cx`.
    #[must_use]
    pub fn next(cx: &Cx) -> Self {
        match cx.islands().current() {
            Some(island) => Self(Id::Island {
                instance: island.instance(),
                ordinal: island.next_reactive_scope(),
            }),
            None => Self::new(),
        }
    }
}

impl Default for ReactiveScopeId {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ReactiveScopeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for ReactiveScopeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

pub struct ReactiveScope {
    shard_id: ShardId,
    exprs: Vec<ViewPart>,
    placeholder: View,
}

impl ReactiveScope {
    #[inline]
    #[must_use]
    pub fn new(shard_id: ShardId, exprs: Vec<ViewPart>, placeholder: View) -> Self {
        Self {
            shard_id,
            exprs,
            placeholder,
        }
    }
}

impl NodeViewParts for ReactiveScope {
    fn into_view_parts(self, cx: &Cx, parts: &mut PartsWriter<'_>) {
        // The id is minted here rather than at construction so that scopes are
        // numbered in the order they reach the document.
        let id = ReactiveScopeId::next(cx);
        let shard_id = self.shard_id.as_str();

        // <!-- ::topcoat::scope::start("<id>", "<path>", ["<js>", ...]) -->
        //
        // Each parameter's JavaScript source is wrapped in a quoted string.
        // The source parts are sealed with the comment context, so any `"`
        // inside the source renders as `&quot;` and the quotes stay
        // unambiguous delimiters on the client.
        parts.push_str_unescaped("<!-- ::topcoat::scope::start(");
        parts.push_str_unescaped(serde_json::to_string(&id).unwrap());
        parts.push_str_unescaped(", ");
        parts.push_str_unescaped(
            serde_json::to_string(&format!("{SHARD_ROUTE_PREFIX}/{shard_id}")).unwrap(),
        );
        parts.push_str_unescaped(", [");
        let last = self.exprs.len().saturating_sub(1);
        for (index, expr) in self.exprs.into_iter().enumerate() {
            parts.push_str_unescaped("\"");
            parts.push_part(expr);
            parts.push_str_unescaped("\"");
            if index != last {
                parts.push_str_unescaped(", ");
            }
        }
        parts.push_str_unescaped("]) -->");
        self.placeholder.into_view_parts(cx, parts);
        parts.push_str_unescaped("<!-- ::topcoat::scope::end(");
        parts.push_str_unescaped(serde_json::to_string(&id).unwrap());
        parts.push_str_unescaped(") -->");
    }
}
