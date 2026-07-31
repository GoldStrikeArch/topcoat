use syn::spanned::Spanned;
use topcoat_view_grammar::view::Node;

use crate::{
    lower::{component_tokens, for_tokens, if_tokens, match_tokens},
    DomWriter, Fill, WriteDom,
};

impl WriteDom for Node {
    fn write(&self, writer: &mut DomWriter<'_>) {
        match self {
            Self::Text(inner) => writer.text(&inner.value(), inner.span()),
            Self::Element(inner) => inner.write(writer),
            Self::Expr(inner) => inner.write(writer),
            Self::RuntimeExpr(inner) => inner.write(writer),
            Self::SignalDecaration(inner) => inner.write(writer),
            Self::Local(inner) => inner.write(writer),
            // A block is not a scope of its own: it groups siblings and the
            // key plan walks straight through it.
            Self::Block(inner) => inner.children.write(writer),
            Self::If(inner) => {
                let (value, fill) = if_tokens(inner, writer);
                writer.child_hole(value, fill, inner.if_token.span);
            }
            Self::ForLoop(inner) => {
                let (value, fill) = for_tokens(inner, writer);
                writer.child_hole(value, fill, inner.for_token.span);
            }
            Self::Match(inner) => {
                let value = match_tokens(inner, writer);
                writer.child_hole(value, Fill::Effect, inner.match_token.span);
            }
            Self::DocumentType(inner) => {
                writer.unsupported(inner.lt_token.span, "a doctype declaration");
            }
            Self::Component(inner) => {
                if let Some(value) = component_tokens(inner, writer) {
                    writer.component_hole(value, inner.path.span());
                }
            }
            Self::Continue(inner) => {
                writer.unsupported(inner.expr_continue.span(), "`continue` in a view body");
            }
            Self::Break(inner) => {
                writer.unsupported(inner.expr_break.span(), "`break` in a view body");
            }
        }
    }
}
