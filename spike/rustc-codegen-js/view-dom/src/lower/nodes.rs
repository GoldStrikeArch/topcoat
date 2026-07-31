use topcoat_view_grammar::view::Nodes;

use crate::{DomWriter, WriteDom};

impl WriteDom for Nodes {
    fn write(&self, writer: &mut DomWriter<'_>) {
        for node in self {
            node.write(writer);
        }
    }
}
