use topcoat_core::context::Cx;
use topcoat_view::{
    Attribute, AttributeKeyViewParts, AttributeValueViewParts, AttributeViewParts, PartsWriter,
    Unescaped,
};

use crate::Expr;

#[derive(Debug, Clone)]
pub struct BindAttribute<K, V> {
    key: K,
    value: Expr<V>,
}

impl<K, V> BindAttribute<K, V> {
    #[inline]
    pub fn new(key: K, value: Expr<V>) -> Self {
        Self { key, value }
    }
}

impl<K, V> AttributeViewParts for BindAttribute<K, V>
where
    K: AttributeKeyViewParts + Clone,
    V: AttributeValueViewParts,
{
    #[inline]
    fn into_view_parts(self, cx: &Cx, parts: &mut PartsWriter<'_>) {
        let Expr { evaluated, js } = self.value;

        // The evaluated attribute is what the server rendered either way; only
        // the source that would re-evaluate it belongs to this runtime.
        Attribute::new(self.key.clone(), evaluated).into_view_parts(cx, parts);
        if crate::in_island(cx) {
            return;
        }
        Attribute::new(
            (Unescaped::new_unchecked("data-topcoat-bind:"), self.key),
            js,
        )
        .into_view_parts(cx, parts);
    }
}
