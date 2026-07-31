use quote::quote;
use topcoat_view_grammar::view::{KeySite, SignalDeclaration};

use crate::{DomWriter, WriteDom};

impl WriteDom for SignalDeclaration {
    fn write(&self, writer: &mut DomWriter<'_>) {
        // The ordinal comes from the key plan, so every emitter numbers this
        // declaration the same way.
        let ordinal = writer.key(KeySite::Signal).ordinal();
        let ident = &self.ident;
        let init = &self.expr;
        writer.declare_signal(ident);
        writer.binding(quote! { let #ident = ::view_abi::signal(#ordinal, #init); });
    }
}
