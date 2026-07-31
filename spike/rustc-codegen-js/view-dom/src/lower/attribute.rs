use quote::ToTokens;
use topcoat_view_grammar::attributes::{Attribute, AttributeKey, AttributeValue};

use crate::{DomWriter, Fill, HoleKind, WriteDom};

impl WriteDom for Attribute {
    fn write(&self, writer: &mut DomWriter<'_>) {
        let AttributeKey::Ident(key) = &self.key else {
            let span = self
                .key
                .as_expr()
                .map_or_else(|| self.eq.span, |expr| expr.paren.span.join());
            writer.unsupported(span, "an attribute with an expression name");
            return;
        };
        let name = key.to_string();

        match &self.value {
            AttributeValue::LitStr(value) => {
                if let Some(builder) = writer.template(key.span()) {
                    builder.attribute(&name, &value.value());
                }
            }
            AttributeValue::Expr(value) => {
                writer.element_hole(
                    HoleKind::Attribute,
                    &name,
                    value.expr.to_token_stream(),
                    Fill::Once,
                    value.paren.span.join(),
                );
            }
        }
    }
}
