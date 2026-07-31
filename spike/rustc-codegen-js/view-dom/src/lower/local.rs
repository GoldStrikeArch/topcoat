use quote::ToTokens;
use topcoat_view_grammar::template::TemplateLocal;

use crate::{DomWriter, WriteDom};

impl WriteDom for TemplateLocal {
    fn write(&self, writer: &mut DomWriter<'_>) {
        // A binding adds no DOM and is never re-evaluated, so it is emitted
        // verbatim ahead of the templates that read it.
        writer.binding(self.local.to_token_stream());
    }
}
