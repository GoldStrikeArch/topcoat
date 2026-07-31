use quote::ToTokens;
use topcoat_view_grammar::{
    attributes::{AttributeNode, Attributes},
    view::KeySite,
};

use crate::{DomWriter, Fill, HoleKind, WriteDom};

impl WriteDom for Attributes {
    fn write(&self, writer: &mut DomWriter<'_>) {
        for item in &self.items {
            item.write(writer);
        }
    }
}

impl WriteDom for AttributeNode {
    fn write(&self, writer: &mut DomWriter<'_>) {
        match self {
            Self::Attribute(inner) => inner.write(writer),
            Self::EventHandler(inner) => inner.write(writer),
            Self::BindAttribute(inner) => inner.write(writer),
            Self::Spread(inner) => {
                let span = inner.expr.paren.span.join();
                writer.element_hole(
                    HoleKind::Spread,
                    "",
                    inner.expr.expr.to_token_stream(),
                    Fill::Once,
                    span,
                );
            }
            // Control flow keeps its key so the plan and the emitter stay in
            // step for whatever follows.
            Self::If(inner) => {
                writer.key(KeySite::ReactiveScope);
                writer.unsupported(inner.if_token.span, "`if` in an attribute list");
            }
            Self::ForLoop(inner) => {
                writer.key(KeySite::ReactiveScope);
                writer.unsupported(inner.for_token.span, "`for` in an attribute list");
            }
            Self::Match(inner) => {
                writer.key(KeySite::ReactiveScope);
                writer.unsupported(inner.match_token.span, "`match` in an attribute list");
            }
            Self::Local(inner) => {
                writer.unsupported(inner.local.let_token.span, "`let` in an attribute list");
            }
            Self::Block(inner) => {
                writer.unsupported(inner.brace.span.join(), "a block in an attribute list");
            }
            Self::Continue(inner) => {
                writer.unsupported(
                    inner.expr_continue.continue_token.span,
                    "`continue` in an attribute list",
                );
            }
            Self::Break(inner) => {
                writer.unsupported(
                    inner.expr_break.break_token.span,
                    "`break` in an attribute list",
                );
            }
        }
    }
}
