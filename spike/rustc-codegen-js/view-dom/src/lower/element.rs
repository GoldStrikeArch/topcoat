use topcoat_view_grammar::view::{Element, ElementName, KeySite, Node};

use crate::{DomWriter, WriteDom};

impl WriteDom for Element {
    fn write(&self, writer: &mut DomWriter<'_>) {
        writer.flush_loose();
        if writer.element_depth() == 0 {
            writer.key(KeySite::TemplateRoot);
        }

        // An element that no other element in this scope encloses starts a
        // template.
        let root = !writer.in_template();
        if root {
            writer.begin_template();
        }

        let span = self.name().span();
        let Some(name) = tag_name(self.name(), writer) else {
            if root {
                writer.end_template();
            }
            return;
        };

        if let Some(builder) = writer.template(span) {
            builder.open_element(&name);
        }
        writer.enter_element();
        self.attributes().write(writer);

        match self {
            Self::Normal { children, .. } => {
                let count = children.iter().filter(|child| in_the_dom(child)).count();
                if let Some(builder) = writer.template(span) {
                    builder.open_children(count);
                }
                for child in children {
                    child.write(writer);
                }
                writer.flush_loose();
                if let Some(builder) = writer.template(span) {
                    builder.close_element(&name);
                }
            }
            // An element written with a trailing slash is emitted with a
            // closing tag. Outside foreign content the HTML parser ignores the
            // slash, so a template that relied on it would come back from its
            // `innerHTML` round trip with the following siblings nested inside.
            Self::SelfClosing { .. } => {
                if let Some(builder) = writer.template(span) {
                    builder.open_children(0);
                    builder.close_element(&name);
                }
            }
            Self::Void { .. } => {
                if let Some(builder) = writer.template(span) {
                    builder.close_void_element();
                }
            }
        }

        writer.leave_element();
        if root {
            writer.end_template();
        }
    }
}

/// The tag this element opens, or `None` when the name was refused.
///
/// The match is **wildcard free on purpose**: `ElementName` gains a variant and
/// this stops compiling, which is the only thing that keeps a new spelling of a
/// tag from silently taking whichever arm happens to be nearest. The grammar's
/// own `string_name()` collapses `Ident` and `LitStr` into one `Option<String>`,
/// which is exactly how the literal form came to be accepted here with nothing
/// checking it and nothing testing it.
///
/// # Why a literal-string tag is lowered rather than refused
///
/// `<"my-tag">` names a tag statically, the same way `<my-tag>` does, so it has a
/// correct lowering and refusing it would make the dom emitter reject a view the
/// grammar accepts for a form it can emit. What was wrong was never the form; it
/// was that the literal's text went into the template unread. Two things are
/// therefore checked here, and neither can be checked for an `Ident` because an
/// identifier cannot spell a way to fail them.
///
/// 1. **It must be a tag name.** The value reaches the template's HTML verbatim,
///    so a literal holding `><script>` would close the element and open another
///    one. That is markup injection from a string the author wrote, and the
///    refusal names the text rather than the position.
/// 2. **It must not name a void element.** `ElementName::is_void_element` answers
///    `false` for every literal, so the grammar has already parsed `<"br">` as a
///    `Normal` element with children and a closing tag, and this emitter would
///    write `<br></br>`. The decision was made before the emitter saw it, so the
///    honest answer is to refuse and say to write `<br>`.
fn tag_name(name: &ElementName, writer: &mut DomWriter<'_>) -> Option<String> {
    let span = name.span();
    match name {
        ElementName::Ident(ident) => Some(ident.to_string()),
        ElementName::LitStr(literal) => {
            let value = literal.value();
            if !is_tag_name(&value) {
                writer.error(
                    span,
                    &format!(
                        "`{value}` is not an html tag name, and a literal element name is written \
                         into the template as it stands"
                    ),
                );
                return None;
            }
            if VOID_ELEMENTS.contains(&value.to_ascii_lowercase().as_str()) {
                writer.error(
                    span,
                    &format!(
                        "`{value}` is a void element, which a literal element name cannot spell: \
                         the grammar has already given this one a closing tag. Write `<{value}>`"
                    ),
                );
                return None;
            }
            Some(value)
        }
        ElementName::Expr(_) => {
            writer.key(KeySite::ElementName);
            writer.unsupported(span, "an element with an expression name");
            None
        }
    }
}

/// Whether `value` is a name an HTML parser reads back as one element.
///
/// Deliberately narrower than the specification's tag-name production. What has
/// to hold is that the text survives an `innerHTML` round trip as the tag it was
/// written as, and the set below is what every emitter in the corpus stays
/// inside: an ASCII letter, then letters, digits, `-`, `_`, `.` and `:`.
fn is_tag_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// The elements an HTML parser closes for itself, so a closing tag for one is
/// not a tag at all.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Whether a node puts a node into the DOM, and so counts toward its parent's
/// child count. Only a parent with more than one such child marks its dynamic
/// children.
///
/// Empty text is nothing: `""` writes no node, so it is not a child either. The
/// writer skips it for the same reason, and the two have to agree or an anchor
/// lands in the wrong child position.
///
/// Wildcard free, and this one is the reason the pattern is worth insisting on:
/// the answer for an unknown variant used to be `true`, so a node added upstream
/// would have been counted as a rendered child, and a wrong child count moves
/// every anchor after it. That is a silent mis-render rather than a compile
/// error, and it would have been found in a browser.
fn in_the_dom(node: &Node) -> bool {
    match node {
        Node::Local(_) | Node::SignalDecaration(_) | Node::Continue(_) | Node::Break(_) => false,
        Node::Text(text) => !text.value().is_empty(),
        Node::DocumentType(_)
        | Node::Element(_)
        | Node::Component(_)
        | Node::Expr(_)
        | Node::RuntimeExpr(_)
        | Node::If(_)
        | Node::ForLoop(_)
        | Node::Match(_)
        | Node::Block(_) => true,
    }
}
