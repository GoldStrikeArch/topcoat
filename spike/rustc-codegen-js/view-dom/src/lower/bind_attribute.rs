use quote::ToTokens;
use topcoat_view_grammar::{
    attributes::{AttributeKey, BindAttribute},
    template::TemplateOrRuntimeExpr,
};

use crate::{is_property, DomWriter, Fill, HoleKind, WriteDom};

impl WriteDom for BindAttribute {
    fn write(&self, writer: &mut DomWriter<'_>) {
        let AttributeKey::Ident(key) = &self.key else {
            let span = self
                .key
                .as_expr()
                .map_or_else(|| self.eq.span, |expr| expr.paren.span.join());
            writer.unsupported(span, "a bind attribute with an expression name");
            return;
        };
        let name = key.to_string();

        // A `$(...)` value is re-read whenever what it reads changes; a plain
        // `(...)` value is written once.
        let (value, fill) = match &self.value {
            TemplateOrRuntimeExpr::Template(inner) => (inner.expr.to_token_stream(), Fill::Once),
            TemplateOrRuntimeExpr::Runtime(inner) => (inner.expr.to_token_stream(), Fill::Effect),
        };

        writer.element_hole(bind_kind(&name), &name, value, fill, key.span());
    }
}

/// How a bound name is written onto its element.
///
/// `class` and `style` take a helper that diffs against the previous value, so
/// they are their own kinds. Everything else is a property or an attribute
/// according to the table the pinned runtime uses.
fn bind_kind(name: &str) -> HoleKind {
    match name {
        "class" => HoleKind::ClassList,
        "style" => HoleKind::Style,
        name if is_property(name) => HoleKind::Property,
        _ => HoleKind::Attribute,
    }
}
