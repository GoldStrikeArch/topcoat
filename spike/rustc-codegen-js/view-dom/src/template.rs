use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::Ident;

/// Mirrors `view_abi::NO_EFFECT_GROUP`: the group of a hole that no shared
/// effect drives.
pub const NO_EFFECT_GROUP: u32 = u32::MAX;

/// What a hole holds. Mirrors `view_abi::HoleKind` so the emitter can name a
/// kind without depending on the ABI's layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoleKind {
    Child,
    Attribute,
    Property,
    DelegatedEvent,
    Event,
    ClassList,
    Style,
    Spread,
    Component,
}

impl HoleKind {
    /// The name of the matching `view_abi::HoleKind` variant.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Child => "Child",
            Self::Attribute => "Attribute",
            Self::Property => "Property",
            Self::DelegatedEvent => "DelegatedEvent",
            Self::Event => "Event",
            Self::ClassList => "ClassList",
            Self::Style => "Style",
            Self::Spread => "Spread",
            Self::Component => "Component",
        }
    }

    /// Whether reactive holes of this kind on one element share an effect.
    ///
    /// A child insert owns its own computation, and an event handler is written
    /// once, so neither joins the element's group.
    #[must_use]
    pub fn groups(self) -> bool {
        !matches!(self, Self::Child | Self::DelegatedEvent | Self::Event)
    }
}

impl ToTokens for HoleKind {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let name = Ident::new(self.name(), proc_macro2::Span::call_site());
        quote! { ::view_abi::HoleKind::#name }.to_tokens(tokens);
    }
}

/// Which `view-abi` marker fills a hole, and so what the backend does with the
/// value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    /// `hole`: a plain value, written once.
    Once,
    /// `effect`: a closure the backend re-runs inside `_$effect` whenever
    /// something it read changes.
    ///
    /// It is emitted as a `move` closure. The values a view's holes read are the
    /// runtime's own handles, which are one machine word and `Copy`, so a `move`
    /// closure copies them into its environment; a borrowing closure would
    /// instead capture a reference to the local holding one, which the value
    /// model has to box to give it an address. The generated code is smaller and
    /// reads the handle directly.
    Effect,
    /// `handler`: a closure the backend installs as an event listener. It has a
    /// marker of its own because that marker's body calls it, which is what
    /// makes the closure's body reachable for the monomorphization collector. It
    /// is a `move` closure, for the reason [`Fill::Effect`] gives.
    Handler,
    /// `hole`, with a value a control-flow construct built: an accessor the
    /// runtime subscribes to, or the list of rows a loop rendered. The backend
    /// has already put whatever reactivity there is inside the value, so nothing
    /// wraps it; like [`Fill::Effect`] it still needs an element to be inserted
    /// into.
    ControlFlow,
    /// `component`: a closure that builds a client component's props and calls
    /// it. It has a marker of its own for both of the reasons the other two do.
    /// That marker's body calls the closure, which is what makes the component
    /// and everything it calls reachable for the monomorphization collector; and
    /// the backend wraps the call in `_$createComponent`, which is what nests the
    /// component's hydration keys inside its caller's.
    Component,
}

impl Fill {
    /// Whether the backend wraps the value in `_$effect`, which is what
    /// `view_abi::Hole::reactive` records.
    #[must_use]
    pub fn reactive(self) -> bool {
        matches!(self, Self::Effect)
    }

    /// Whether the value needs an element to be inserted into: it either
    /// re-renders or is re-read, so handing it back as a view's own value would
    /// leave nothing subscribed to it.
    #[must_use]
    pub fn needs_a_parent(self) -> bool {
        !matches!(self, Self::Once)
    }

    /// The marker call that fills a hole of this kind.
    fn marker(self) -> TokenStream {
        match self {
            Self::Once | Self::ControlFlow => quote! { ::view_abi::hole },
            Self::Effect => quote! { ::view_abi::effect },
            Self::Handler => quote! { ::view_abi::handler },
            Self::Component => quote! { ::view_abi::component },
        }
    }
}

/// One dynamic position in a template, paired with the Rust expression that
/// fills it. Everything but the expression is serialized into the template's
/// static; the expression goes into the client body.
pub struct Hole {
    path: String,
    kind: HoleKind,
    name: String,
    fill: Fill,
    effect_group: u32,
    value: TokenStream,
}

impl Hole {
    /// A hole at `path`, filled by `value`, that no shared effect drives.
    #[must_use]
    pub fn new(path: String, kind: HoleKind, name: &str, fill: Fill, value: TokenStream) -> Self {
        Self {
            path,
            kind,
            name: name.to_owned(),
            fill,
            effect_group: NO_EFFECT_GROUP,
            value,
        }
    }

    /// The same hole, driven by the effect of group `group`.
    #[must_use]
    pub fn in_group(mut self, group: u32) -> Self {
        self.effect_group = group;
        self
    }

    /// The walk from the cloned template root to the hole's node: `c` for
    /// `firstChild`, `n` for `nextSibling`.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub fn kind(&self) -> HoleKind {
        self.kind
    }

    /// The attribute, property or event name. Empty where the kind takes none.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How the value reaches the runtime.
    #[must_use]
    pub fn fill(&self) -> Fill {
        self.fill
    }

    /// Whether the backend wraps the value in `_$effect`.
    #[must_use]
    pub fn reactive(&self) -> bool {
        self.fill.reactive()
    }

    /// Which effect drives this hole, or [`NO_EFFECT_GROUP`].
    #[must_use]
    pub fn effect_group(&self) -> u32 {
        self.effect_group
    }

    /// The Rust expression filling the hole.
    #[must_use]
    pub fn value(&self) -> &TokenStream {
        &self.value
    }

    /// The `view_abi::Hole` entry for this hole.
    fn data(&self) -> TokenStream {
        let Self {
            path,
            kind,
            name,
            effect_group,
            ..
        } = self;
        let reactive = self.reactive();
        quote! {
            ::view_abi::Hole {
                path: #path,
                kind: #kind,
                name: #name,
                reactive: #reactive,
                effect_group: #effect_group,
            }
        }
    }
}

/// A finished template: the HTML skeleton, its holes in source order, the
/// delegated events it needs, and the flags `_$template` takes after the HTML.
pub struct Template {
    html: String,
    holes: Vec<Hole>,
    events: BTreeSet<String>,
    hydratable: bool,
    is_import_node: bool,
    is_svg: bool,
}

impl Template {
    /// The template's HTML, exactly as it is handed to `innerHTML`.
    #[must_use]
    pub fn html(&self) -> &str {
        &self.html
    }

    /// The holes, in the order they appear in the source.
    #[must_use]
    pub fn holes(&self) -> &[Hole] {
        &self.holes
    }

    /// The delegated events this template needs, deduplicated and sorted.
    #[must_use]
    pub fn events(&self) -> Vec<&str> {
        self.events.iter().map(String::as_str).collect()
    }

    /// Whether the template claims server-rendered nodes instead of cloning.
    #[must_use]
    pub fn hydratable(&self) -> bool {
        self.hydratable
    }

    /// Whether the cloner imports the prototype node instead of cloning it,
    /// which is what upgrades a custom element on insertion.
    #[must_use]
    pub fn is_import_node(&self) -> bool {
        self.is_import_node
    }

    /// Whether the template's root is an SVG-only element, in which case
    /// [`Template::html`] already carries the `<svg>` wrapper the flag expects.
    #[must_use]
    pub fn is_svg(&self) -> bool {
        self.is_svg
    }

    /// The `static` item the backend decodes at codegen time.
    ///
    /// The static deliberately carries no attribute: rustc rejects a custom
    /// `link_section` on a static holding references, and the backend never
    /// needed one — it finds the payload through the marker call's argument
    /// provenance, not by scanning for marked statics.
    #[must_use]
    pub fn data(&self, ident: &Ident) -> TokenStream {
        let html = &self.html;
        let hydratable = self.hydratable;
        let is_import_node = self.is_import_node;
        let is_svg = self.is_svg;
        let holes = self.holes.iter().map(Hole::data);
        let events = self.events.iter();
        quote! {
            static #ident: ::view_abi::TemplateData = ::view_abi::TemplateData {
                abi: ::view_abi::ABI_VERSION,
                html: #html,
                hydratable: #hydratable,
                is_import_node: #is_import_node,
                is_svg: #is_svg,
                holes: &[#(#holes),*],
                events: &[#(#events),*],
            };
        }
    }

    /// The statements that instantiate the template into `root` and fill its
    /// holes, in hole order.
    #[must_use]
    pub fn body(&self, data: &Ident, root: &Ident) -> TokenStream {
        let fills = self.holes.iter().enumerate().map(|(index, hole)| {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            let value = hole.value();
            let marker = hole.fill().marker();
            // Only `effect` takes the value as a closure: it is the one the backend re-runs.
            // It is a `move` closure for the reason [`Fill::Effect`] gives.
            match hole.fill() {
                Fill::Effect => quote! { #marker(&#root, #index, move || #value); },
                _ => quote! { #marker(&#root, #index, #value); },
            }
        });
        quote! {
            let #root = ::view_abi::template(&#data);
            #(#fills)*
        }
    }
}

/// One open element while a template is being built.
struct Frame {
    /// The walk from the template root to this element.
    path: String,
    /// How many DOM child nodes have been emitted into this element.
    children: usize,
    /// How many DOM-producing source children this element has. A lone dynamic
    /// child needs neither an anchor nor a marker pair.
    expected: usize,
    /// Whether the last emitted child is a text node that further literal text
    /// would merge into.
    trailing_text: bool,
    /// The effect the element's reactive holes share, once one has claimed it.
    group: Option<u32>,
}

impl Frame {
    /// The walk to this element's next child.
    fn next_child_path(&self) -> String {
        let mut path = String::with_capacity(self.path.len() + self.children + 1);
        path.push_str(&self.path);
        path.push('c');
        for _ in 0..self.children {
            path.push('n');
        }
        path
    }

    /// Accounts for a node that is about to be appended, returning its path.
    fn append(&mut self) -> String {
        let path = self.next_child_path();
        self.children += 1;
        self.trailing_text = false;
        path
    }
}

/// Accumulates one template's HTML while tracking the walk to every hole.
///
/// The same cursor drives the HTML and the hole paths, so inserting an anchor
/// comment shifts every following sibling walk on its own.
#[derive(Default)]
pub struct TemplateBuilder {
    html: String,
    holes: Vec<Hole>,
    events: BTreeSet<String>,
    frames: Vec<Frame>,
    hydratable: bool,
    /// The tag the template's root element opened with, which is what decides
    /// the `isSVG` flag and the synthetic wrapper.
    root_tag: Option<String>,
    /// Whether any element written so far has a dashed tag name.
    dashed_tag: bool,
}

impl TemplateBuilder {
    /// A builder for a template instantiated by cloning or, when `hydratable`,
    /// by claiming server-rendered nodes.
    #[must_use]
    pub fn new(hydratable: bool) -> Self {
        Self {
            hydratable,
            ..Self::default()
        }
    }

    /// Starts an element and descends into it.
    pub fn open_element(&mut self, name: &str) {
        let path = match self.frames.last_mut() {
            Some(parent) => parent.append(),
            None => String::new(),
        };
        if self.root_tag.is_none() {
            self.root_tag = Some(name.to_owned());
        }
        // A custom element is any element with a dashed name, anywhere in the
        // template: importing the prototype is per template, not per element.
        self.dashed_tag |= name.contains('-');
        self.html.push('<');
        self.html.push_str(name);
        self.frames.push(Frame {
            path,
            children: 0,
            expected: 0,
            trailing_text: false,
            group: None,
        });
    }

    /// Writes a static attribute into the open tag.
    pub fn attribute(&mut self, name: &str, value: &str) {
        self.html.push(' ');
        self.html.push_str(name);
        self.html.push_str("=\"");
        escape_attribute_value(&mut self.html, value);
        self.html.push('"');
    }

    /// Closes the open tag of an element that has `children` DOM-producing
    /// source children.
    pub fn open_children(&mut self, children: usize) {
        self.html.push('>');
        if let Some(frame) = self.frames.last_mut() {
            frame.expected = children;
        }
    }

    /// Closes an element.
    ///
    /// An element written with a trailing slash is closed the same way: outside
    /// foreign content the HTML parser ignores the slash, so a template that
    /// relied on it would not survive its `innerHTML` round trip.
    pub fn close_element(&mut self, name: &str) {
        self.html.push_str("</");
        self.html.push_str(name);
        self.html.push('>');
        self.frames.pop();
    }

    /// Closes a void element, which has no closing tag.
    pub fn close_void_element(&mut self) {
        self.html.push('>');
        self.frames.pop();
    }

    /// Appends literal text, merging it into the preceding text node when one
    /// is already there.
    pub fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(frame) = self.frames.last_mut() {
            if !frame.trailing_text {
                frame.children += 1;
                frame.trailing_text = true;
            }
        }
        escape_text(&mut self.html, text);
    }

    /// Appends the marking for one dynamic child and returns the walk to it.
    ///
    /// A lone child needs no marking and is inserted against its parent. A
    /// hydratable template brackets the child with a `<!$>`/`<!/>` pair, whose
    /// opening marker the walk reaches. Otherwise a bare `<!>` anchor is
    /// appended.
    pub fn child_marker(&mut self) -> String {
        if self.at_sole_child() {
            return self.element_path();
        }
        if self.hydratable {
            let path = self.append_node();
            self.html.push_str("<!$>");
            self.append_node();
            self.html.push_str("<!/>");
            return path;
        }
        let path = self.append_node();
        self.html.push_str("<!>");
        path
    }

    /// Accounts for one node position without writing anything.
    fn append_node(&mut self) -> String {
        match self.frames.last_mut() {
            Some(frame) => frame.append(),
            None => String::new(),
        }
    }

    /// The walk to the element currently being written.
    #[must_use]
    pub fn element_path(&self) -> String {
        self.frames
            .last()
            .map_or_else(String::new, |frame| frame.path.clone())
    }

    /// Returns `true` if the node about to be written is the only child of its
    /// element, which can then be filled with no marking at all.
    #[must_use]
    pub fn at_sole_child(&self) -> bool {
        self.frames
            .last()
            .is_some_and(|frame| frame.expected == 1 && frame.children == 0)
    }

    /// The effect the open element's reactive holes share, claiming `fresh` if
    /// this is the first one.
    pub fn element_group(&mut self, fresh: u32) -> u32 {
        match self.frames.last_mut() {
            Some(frame) => *frame.group.get_or_insert(fresh),
            None => fresh,
        }
    }

    /// Records a hole. Holes are kept in the order they are recorded, which is
    /// source order.
    pub fn push_hole(&mut self, hole: Hole) {
        self.holes.push(hole);
    }

    /// Records that this template needs `name` delegated.
    pub fn push_event(&mut self, name: &str) {
        self.events.insert(name.to_owned());
    }

    /// Finishes the template, deciding the two construction flags.
    ///
    /// A template rooted at an SVG-only element is wrapped in a literal `<svg>`
    /// here rather than left to the backend, because the wrapper and the flag
    /// are one decision: `isSVG` makes the runtime read
    /// `t.content.firstChild.firstChild`, so a flag with no wrapper, or a
    /// wrapper with no flag, silently yields the wrong root node
    /// (`contract/CONTRACT-DOM.md` 2.2). The wrapper is closed, unlike the
    /// trailing tags a template may leave open, because that unwrap counts on it.
    ///
    /// Hole paths are untouched: they stay rooted at the element that was
    /// written, which is the node the runtime hands back.
    #[must_use]
    pub fn finish(self) -> Template {
        let is_svg = self
            .root_tag
            .as_deref()
            .is_some_and(crate::is_svg_only_element);
        let html = match is_svg {
            true => format!("<svg>{}</svg>", self.html),
            false => self.html,
        };
        Template {
            html,
            holes: self.holes,
            events: self.events,
            hydratable: self.hydratable,
            is_import_node: self.dashed_tag,
            is_svg,
        }
    }
}

/// Appends `text` escaped for a text position: only `<` and `&` can end it.
fn escape_text(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '<' => out.push_str("&lt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(character),
        }
    }
}

/// Appends `value` escaped for a double-quoted attribute value: only `"` and
/// `&` can end it.
fn escape_attribute_value(out: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '"' => out.push_str("&quot;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(character),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_escapes_only_text_delimiters() {
        let mut out = String::new();
        escape_text(&mut out, r#"a < b & "c" > d"#);
        assert_eq!(out, r#"a &lt; b &amp; "c" > d"#);
    }

    #[test]
    fn attribute_values_escape_only_value_delimiters() {
        let mut out = String::new();
        escape_attribute_value(&mut out, r#"a < b & "c""#);
        assert_eq!(out, "a < b &amp; &quot;c&quot;");
    }

    #[test]
    fn adjacent_text_stays_one_node() {
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("p");
        builder.open_children(3);
        builder.text("a");
        builder.text("b");
        let path = builder.child_marker();
        builder.close_element("p");
        assert_eq!(builder.finish().html(), "<p>ab<!></p>");
        // The two literals merged, so the anchor is the second child.
        assert_eq!(path, "cn");
    }

    #[test]
    fn an_anchor_shifts_the_walks_that_follow_it() {
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("p");
        builder.open_children(3);
        builder.text("a");
        let first = builder.child_marker();
        let second = builder.child_marker();
        builder.close_element("p");
        assert_eq!(builder.finish().html(), "<p>a<!><!></p>");
        assert_eq!(first, "cn");
        assert_eq!(second, "cnn");
    }

    #[test]
    fn a_marker_pair_takes_two_node_positions() {
        let mut builder = TemplateBuilder::new(true);
        builder.open_element("p");
        builder.open_children(3);
        builder.text("a");
        let first = builder.child_marker();
        let second = builder.child_marker();
        builder.close_element("p");
        assert_eq!(builder.finish().html(), "<p>a<!$><!/><!$><!/></p>");
        assert_eq!(first, "cn");
        assert_eq!(second, "cnnn");
    }

    #[test]
    fn a_lone_child_is_marked_in_neither_mode() {
        for hydratable in [false, true] {
            let mut builder = TemplateBuilder::new(hydratable);
            builder.open_element("p");
            builder.open_children(1);
            let path = builder.child_marker();
            builder.close_element("p");
            assert_eq!(builder.finish().html(), "<p></p>");
            assert_eq!(path, "");
        }
    }

    #[test]
    fn nested_elements_walk_from_the_root() {
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("div");
        builder.open_children(2);
        builder.open_element("span");
        builder.open_children(1);
        let inner = builder.child_marker();
        builder.close_element("span");
        builder.open_element("br");
        builder.close_void_element();
        builder.close_element("div");
        assert_eq!(builder.finish().html(), "<div><span></span><br></div>");
        assert_eq!(inner, "c");
    }

    #[test]
    fn a_plain_template_carries_neither_flag() {
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("div");
        builder.open_children(0);
        builder.close_element("div");
        let template = builder.finish();
        assert_eq!(template.html(), "<div></div>");
        assert!(!template.is_import_node());
        assert!(!template.is_svg());
    }

    #[test]
    fn a_dashed_tag_anywhere_imports_the_prototype() {
        // The root itself.
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("my-element");
        builder.open_children(0);
        builder.close_element("my-element");
        assert!(builder.finish().is_import_node());

        // A descendant, which is the same template-wide decision.
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("div");
        builder.open_children(1);
        builder.open_element("my-widget");
        builder.open_children(0);
        builder.close_element("my-widget");
        builder.close_element("div");
        let template = builder.finish();
        assert!(template.is_import_node());
        assert!(!template.is_svg());
    }

    #[test]
    fn an_svg_root_is_an_ordinary_element() {
        // `<svg>` parses in the right namespace on its own, so no flag and no
        // wrapper. The `<rect>` inside it is not the root.
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("svg");
        builder.open_children(1);
        builder.open_element("rect");
        builder.open_children(0);
        builder.close_element("rect");
        builder.close_element("svg");
        let template = builder.finish();
        assert_eq!(template.html(), "<svg><rect></rect></svg>");
        assert!(!template.is_svg());
    }

    #[test]
    fn an_svg_only_root_is_wrapped_and_flagged_together() {
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("rect");
        builder.attribute("x", "50");
        builder.open_children(0);
        builder.close_element("rect");
        let template = builder.finish();
        assert!(template.is_svg());
        assert_eq!(template.html(), "<svg><rect x=\"50\"></rect></svg>");
    }

    #[test]
    fn the_wrapper_does_not_move_a_hole_path() {
        // The walk is rooted at the element that was written, not at the
        // wrapper, because the wrapper is what the runtime unwraps past.
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("linearGradient");
        builder.open_children(2);
        builder.open_element("stop");
        builder.open_children(0);
        builder.close_element("stop");
        let path = builder.child_marker();
        builder.close_element("linearGradient");
        let template = builder.finish();
        assert!(template.is_svg());
        assert_eq!(
            template.html(),
            "<svg><linearGradient><stop></stop><!></linearGradient></svg>"
        );
        assert_eq!(path, "cn");
    }

    #[test]
    fn one_element_hands_out_one_group() {
        let mut builder = TemplateBuilder::new(false);
        builder.open_element("div");
        assert_eq!(builder.element_group(0), 0);
        assert_eq!(builder.element_group(1), 0);
        builder.open_children(1);
        builder.open_element("span");
        assert_eq!(builder.element_group(1), 1);
        builder.close_element("span");
        builder.close_element("div");
    }

    #[test]
    fn the_variant_names_exist_on_the_abi_enum() {
        // Constructing every variant keeps the mirrored names honest: the
        // emitter writes these paths into the generated statics.
        let _ = [
            view_abi::HoleKind::Child,
            view_abi::HoleKind::Attribute,
            view_abi::HoleKind::Property,
            view_abi::HoleKind::DelegatedEvent,
            view_abi::HoleKind::Event,
            view_abi::HoleKind::ClassList,
            view_abi::HoleKind::Style,
            view_abi::HoleKind::Spread,
            view_abi::HoleKind::Component,
        ];
    }

    #[test]
    fn the_no_group_sentinel_matches_the_abi() {
        assert_eq!(NO_EFFECT_GROUP, view_abi::NO_EFFECT_GROUP);
    }
}
