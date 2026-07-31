use std::collections::BTreeSet;

use proc_macro2::{Span, TokenStream, TokenTree};
use quote::{format_ident, quote, ToTokens};
use topcoat_view_grammar::view::{Key, KeyCursor, KeyPlan, KeySite, View};

use crate::{is_delegated, Fill, Hole, HoleKind, Template, TemplateBuilder, NO_EFFECT_GROUP};

/// AST nodes that can emit themselves into a [`DomWriter`].
pub trait WriteDom {
    fn write(&self, writer: &mut DomWriter<'_>);
}

/// One value a scope contributes to its result.
enum Item {
    /// A template instantiated in this scope, by id.
    Template(usize),
    /// An expression contributed directly, with no template of its own.
    Value(TokenStream),
}

/// A place templates are instantiated in: the view body, or one branch of a
/// control-flow construct.
#[derive(Default)]
struct Scope {
    /// The template being built in this scope, with its id.
    open: Option<(usize, TemplateBuilder)>,
    /// Whether the open template holds loose text rather than an element, and
    /// so ends as soon as anything else appears.
    loose: bool,
    items: Vec<Item>,
    /// The `let` bindings the scope's templates are instantiated after.
    bindings: TokenStream,
}

/// Builds the templates and the client body a `view!` invocation lowers to when
/// it is compiled for the DOM.
///
/// A writer never panics on a construct it cannot lower: it records a spanned
/// error and carries on, so one expansion reports every unsupported node at
/// once.
pub struct DomWriter<'a> {
    cursor: KeyCursor<'a>,
    templates: Vec<Option<Template>>,
    scopes: Vec<Scope>,
    /// How many elements enclose the node being written, counted the way the
    /// key plan counts it so both stay in step.
    element_depth: usize,
    /// The next effect group id to hand out.
    groups: u32,
    /// The names `signal name = init;` has bound so far, which is what
    /// [`DomWriter::reads_a_signal`] recognizes.
    signals: BTreeSet<String>,
    hydratable: bool,
    errors: Vec<syn::Error>,
}

impl<'a> DomWriter<'a> {
    /// A writer reading its keys from `cursor`.
    ///
    /// A hydratable writer brackets dynamic children with `<!$>`/`<!/>` pairs
    /// and marks its templates so the backend claims server-rendered nodes
    /// rather than cloning.
    #[must_use]
    pub fn new(cursor: KeyCursor<'a>, hydratable: bool) -> Self {
        Self {
            cursor,
            templates: Vec::new(),
            scopes: vec![Scope::default()],
            element_depth: 0,
            groups: 0,
            signals: BTreeSet::new(),
            hydratable,
            errors: Vec::new(),
        }
    }

    /// Records that `name` is a signal declared by this view.
    ///
    /// Declarations are lowered before the nodes that read them, so the set is
    /// complete by the time anything asks.
    pub fn declare_signal(&mut self, name: &syn::Ident) {
        self.signals.insert(name.to_string());
    }

    /// Whether `tokens` mention a signal this view declared.
    ///
    /// A signal is an ordinary `Copy` handle bound to an ordinary local, so it
    /// can be read through any path that ends in a `get`: a method call, a
    /// helper function taking the handle, a field of a struct built from it.
    /// Naming it at all is what this asks, because naming it is the only way any
    /// of those can reach it.
    ///
    /// A signal that this view did not declare is invisible here. That is the
    /// whole set of signals a view can name today.
    #[must_use]
    pub fn reads_a_signal(&self, tokens: &impl ToTokens) -> bool {
        fn mentions(tokens: TokenStream, names: &BTreeSet<String>) -> bool {
            tokens.into_iter().any(|tree| match tree {
                TokenTree::Ident(ident) => names.contains(&ident.to_string()),
                TokenTree::Group(group) => mentions(group.stream(), names),
                _ => false,
            })
        }

        !self.signals.is_empty() && mentions(tokens.to_token_stream(), &self.signals)
    }

    /// Consumes the next key for `site`.
    pub fn key(&mut self, site: KeySite) -> Key {
        self.cursor.take(site)
    }

    /// Records that `what` cannot be lowered yet.
    pub fn unsupported(&mut self, span: Span, what: &str) {
        self.errors.push(syn::Error::new(
            span,
            format!("{what} is not yet supported by the dom emitter"),
        ));
    }

    /// Records an error that is not about an unsupported construct.
    pub fn error(&mut self, span: Span, message: &str) {
        self.errors.push(syn::Error::new(span, message));
    }

    /// How many elements enclose the node being written.
    #[must_use]
    pub fn element_depth(&self) -> usize {
        self.element_depth
    }

    /// Descends into an element.
    pub fn enter_element(&mut self) {
        self.element_depth += 1;
    }

    /// Leaves an element.
    pub fn leave_element(&mut self) {
        self.element_depth = self.element_depth.saturating_sub(1);
    }

    /// Returns `true` while a template is being built in the current scope.
    #[must_use]
    pub fn in_template(&self) -> bool {
        self.scopes.last().is_some_and(|scope| scope.open.is_some())
    }

    /// Starts a template for an element that no other element in this scope
    /// encloses.
    pub fn begin_template(&mut self) {
        self.open_template(false);
    }

    /// Finishes the template being built, if there is one.
    pub fn end_template(&mut self) {
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        let Some((id, builder)) = scope.open.take() else {
            return;
        };
        scope.loose = false;
        scope.items.push(Item::Template(id));
        self.templates[id] = Some(builder.finish());
    }

    /// Ends a template that only holds loose text, so the node that follows it
    /// does not land inside it.
    pub fn flush_loose(&mut self) {
        if self.scopes.last().is_some_and(|scope| scope.loose) {
            self.end_template();
        }
    }

    /// The template being built, or `None` after recording that `span` is not
    /// inside one.
    pub fn template(&mut self, span: Span) -> Option<&mut TemplateBuilder> {
        if !self.in_template() {
            self.error(span, "the dom emitter only lowers nodes inside an element");
            return None;
        }
        self.scopes
            .last_mut()?
            .open
            .as_mut()
            .map(|(_, builder)| builder)
    }

    /// Appends literal text. Text outside an element becomes a template of its
    /// own, holding a single text node.
    ///
    /// Empty text writes nothing at all. A template of empty HTML has no node to
    /// clone, so an arm or a branch that renders `""` renders nothing, which is
    /// also what it means.
    pub fn text(&mut self, text: &str, span: Span) {
        if text.is_empty() {
            return;
        }
        if !self.in_template() {
            self.open_template(true);
        }
        if let Some(builder) = self.template(span) {
            builder.text(text);
        }
    }

    /// Records a `let` binding, emitted before the scope's templates are
    /// instantiated.
    pub fn binding(&mut self, tokens: TokenStream) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.extend(tokens);
        }
    }

    /// Records a dynamic child, marked according to the template's mode.
    ///
    /// A child written where no element is open contributes its expression to
    /// the scope's value instead, which is how a control-flow branch that
    /// renders a bare expression or a nested conditional lowers. In the
    /// outermost scope there is no enclosing effect to re-run such a value, so
    /// a reactive one is rejected there.
    pub fn child_hole(&mut self, value: TokenStream, fill: Fill, span: Span) {
        self.flush_loose();
        if !self.in_template() {
            if fill.needs_a_parent() && self.scopes.len() == 1 {
                self.error(
                    span,
                    "a reactive node at the top level of a view has no element to insert it into",
                );
                return;
            }
            if let Some(scope) = self.scopes.last_mut() {
                scope.items.push(Item::Value(value));
            }
            return;
        }
        let Some(builder) = self.template(span) else {
            return;
        };
        let path = builder.child_marker();
        builder.push_hole(Hole::new(path, HoleKind::Child, "", fill, value));
    }

    /// Records a component invocation, whose value the runtime calls once it has
    /// opened a key context for the component.
    ///
    /// A component is a child position like any other, so it takes a marker of
    /// its own; what the kind changes is what the runtime does with the value. It
    /// needs an element to be inserted into, because the context is opened around
    /// the insert.
    pub fn component_hole(&mut self, value: TokenStream, span: Span) {
        self.flush_loose();
        if !self.in_template() {
            self.error(
                span,
                "a component at the top level of a view or a branch has no element to insert it \
                 into",
            );
            return;
        }
        let Some(builder) = self.template(span) else {
            return;
        };
        let path = builder.child_marker();
        builder.push_hole(Hole::new(path, HoleKind::Component, "", Fill::Component, value));
    }

    /// Records a dynamic value written onto the open element.
    pub fn element_hole(
        &mut self,
        kind: HoleKind,
        name: &str,
        value: TokenStream,
        fill: Fill,
        span: Span,
    ) {
        let fresh = self.groups;
        let Some(builder) = self.template(span) else {
            return;
        };
        let path = builder.element_path();
        let group = if fill.reactive() && kind.groups() {
            builder.element_group(fresh)
        } else {
            NO_EFFECT_GROUP
        };
        let hole = Hole::new(path, kind, name, fill, value).in_group(group);
        builder.push_hole(hole);
        if group == fresh {
            self.groups += 1;
        }
    }

    /// Records a handler for the `name` event, delegated where the contract
    /// says so.
    pub fn event_hole(&mut self, name: &str, value: TokenStream, span: Span) {
        let kind = if is_delegated(name) {
            HoleKind::DelegatedEvent
        } else {
            HoleKind::Event
        };
        self.element_hole(kind, name, value, Fill::Handler, span);
        if kind == HoleKind::DelegatedEvent {
            if let Some(builder) = self.template(span) {
                builder.push_event(name);
            }
        }
    }

    /// Lowers `f` in a scope of its own and returns the branch expression it
    /// evaluates to.
    ///
    /// Every branch of a conditional and every row of a loop goes through this,
    /// so they all have the one type `::view_abi::content` erases them to.
    ///
    /// A branch body is built and dropped on its own, so the elements around it
    /// do not enclose what is inside it: the depth restarts at zero and the
    /// branch's outermost element is a template root of its own. That is what
    /// the key plan's own walk does, and the two have to agree site for site.
    pub fn branch(&mut self, f: impl FnOnce(&mut Self)) -> TokenStream {
        self.scopes.push(Scope::default());
        let depth = std::mem::take(&mut self.element_depth);
        f(self);
        self.element_depth = depth;
        let inner = self.close_scope();
        quote! { ::view_abi::content({ #inner }) }
    }

    /// Starts a template in the current scope, `loose` when it was started by
    /// text rather than by an element.
    fn open_template(&mut self, loose: bool) {
        let id = self.templates.len();
        self.templates.push(None);
        let hydratable = self.hydratable;
        if let Some(scope) = self.scopes.last_mut() {
            scope.open = Some((id, TemplateBuilder::new(hydratable)));
            scope.loose = loose;
        }
    }

    /// Pops the innermost scope and returns the statements that instantiate its
    /// templates, followed by the value it evaluates to.
    fn close_scope(&mut self) -> TokenStream {
        self.end_template();
        let Some(scope) = self.scopes.pop() else {
            return quote! { () };
        };

        let bindings = scope.bindings;
        let mut bodies = TokenStream::new();
        let mut values = Vec::with_capacity(scope.items.len());
        for item in scope.items {
            match item {
                Item::Template(id) => {
                    let data = format_ident!("__TC_TEMPLATE_{id}");
                    let root = format_ident!("__tc_root_{id}");
                    if let Some(template) = self.templates[id].as_ref() {
                        bodies.extend(template.body(&data, &root));
                    }
                    values.push(quote! { #root });
                }
                Item::Value(tokens) => values.push(tokens),
            }
        }

        let value = match values.len() {
            0 => quote! { () },
            1 => quote! { #(#values)* },
            _ => quote! { (#(#values),*) },
        };
        quote! { #bindings #bodies #value }
    }

    /// Returns the templates and the client body, or every error recorded along
    /// the way.
    ///
    /// # Errors
    ///
    /// Returns the combined errors for each construct the emitter cannot lower.
    pub fn finish(mut self) -> syn::Result<DomOutput> {
        let body = self.close_scope();

        // A plan the emitter did not walk in step with, or did not walk to the
        // end of, means the two disagree about the view's shape and every
        // ordinal after the disagreement is suspect.
        if self.errors.is_empty() && (self.cursor.desynced() || self.cursor.peek().is_some()) {
            self.error(
                Span::call_site(),
                "the dom emitter and the view's key plan disagree about the view's shape",
            );
        }

        let mut errors = self.errors.into_iter();
        if let Some(mut first) = errors.next() {
            for error in errors {
                first.combine(error);
            }
            return Err(first);
        }

        let templates = self
            .templates
            .into_iter()
            .map(|template| template.expect("every started template is finished"))
            .collect();
        Ok(DomOutput { templates, body })
    }
}

/// What a view lowers to: the templates it needs, and the client body that
/// instantiates them.
pub struct DomOutput {
    templates: Vec<Template>,
    body: TokenStream,
}

impl DomOutput {
    /// Lowers `view` to its templates.
    ///
    /// # Errors
    ///
    /// Returns the combined errors for each construct the emitter cannot lower.
    pub fn build(view: &View, hydratable: bool) -> syn::Result<Self> {
        let plan = KeyPlan::build(&view.nodes);
        let mut writer = DomWriter::new(plan.cursor(), hydratable);
        if let Some(cx) = &view.cx {
            writer.error(
                cx.cx.span(),
                "a leading `cx =>` argument names a request context, which a client view has none of",
            );
        }
        view.nodes.write(&mut writer);
        writer.finish()
    }

    /// The templates, in the order they were started.
    #[must_use]
    pub fn templates(&self) -> &[Template] {
        &self.templates
    }

    /// The block a `view!` invocation expands to: the template statics
    /// followed by the calls that instantiate them and fill their holes.
    #[must_use]
    pub fn expand(&self) -> TokenStream {
        let statics = self
            .templates
            .iter()
            .enumerate()
            .map(|(index, template)| template.data(&format_ident!("__TC_TEMPLATE_{index}")));
        let body = &self.body;
        quote! {{
            #(#statics)*
            #body
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `(path, kind, name, reactive, effect_group)` of one hole.
    type HoleRow = (String, HoleKind, String, bool, u32);

    fn build(source: &str, hydratable: bool) -> DomOutput {
        let view: View = syn::parse_str(source).unwrap();
        match DomOutput::build(&view, hydratable) {
            Ok(output) => output,
            Err(error) => panic!("expected `{source}` to lower: {error}"),
        }
    }

    fn lower(source: &str) -> DomOutput {
        build(source, false)
    }

    fn hydratable(source: &str) -> DomOutput {
        build(source, true)
    }

    fn html(source: &str) -> String {
        lower(source).templates()[0].html().to_owned()
    }

    fn hydratable_html(source: &str) -> String {
        hydratable(source).templates()[0].html().to_owned()
    }

    fn htmls(source: &str) -> Vec<String> {
        lower(source)
            .templates()
            .iter()
            .map(|template| template.html().to_owned())
            .collect()
    }

    fn holes(source: &str) -> Vec<HoleRow> {
        lower(source).templates()[0]
            .holes()
            .iter()
            .map(|hole| {
                (
                    hole.path().to_owned(),
                    hole.kind(),
                    hole.name().to_owned(),
                    hole.reactive(),
                    hole.effect_group(),
                )
            })
            .collect()
    }

    fn paths(source: &str) -> Vec<String> {
        holes(source).into_iter().map(|hole| hole.0).collect()
    }

    fn kinds(source: &str) -> Vec<HoleKind> {
        holes(source).into_iter().map(|hole| hole.1).collect()
    }

    fn expanded(source: &str) -> String {
        lower(source).expand().to_string()
    }

    fn error(source: &str) -> String {
        let view: View = syn::parse_str(source).unwrap();
        match DomOutput::build(&view, false) {
            Ok(_) => panic!("expected `{source}` to be rejected"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_static_element_is_all_template_and_no_holes() {
        let output = lower(r#"<div class="card"><p>"hello"</p></div>"#);
        let template = &output.templates()[0];
        assert_eq!(template.html(), r#"<div class="card"><p>hello</p></div>"#);
        assert!(template.holes().is_empty());
        assert!(template.events().is_empty());
        assert!(!template.hydratable());
    }

    #[test]
    fn adjacent_literals_become_one_text_node() {
        assert_eq!(html(r#"<p>"a" "b" "c"</p>"#), "<p>abc</p>");
    }

    #[test]
    fn literals_are_escaped_for_their_position() {
        // Text only has to escape what can end it, and so does an attribute
        // value: a `"` is fine in text and a `<` is fine in a value.
        assert_eq!(html("<p>\"x < y & z\"</p>"), "<p>x &lt; y &amp; z</p>");
        assert_eq!(
            html("<p title=\"a & \\\"b\\\" < c\"></p>"),
            "<p title=\"a &amp; &quot;b&quot; < c\"></p>",
        );
    }

    #[test]
    fn void_elements_have_no_closing_tag_and_self_closing_ones_do() {
        // A trailing slash is ignored by the HTML parser outside foreign
        // content, so emitting it verbatim would not survive the round trip.
        assert_eq!(html("<br>"), "<br>");
        assert_eq!(html(r#"<img src="a.png">"#), r#"<img src="a.png">"#);
        // `<path>` is SVG only, so a template rooted at one also picks up the
        // synthetic `<svg>` wrapper; the closing tag is the point here.
        assert_eq!(
            html(r#"<path d="M0 0"/>"#),
            r#"<svg><path d="M0 0"></path></svg>"#
        );
        assert_eq!(
            html(r"<div><span/>(x)</div>"),
            "<div><span></span><!></div>"
        );
    }

    #[test]
    fn a_lone_dynamic_child_needs_no_marking() {
        assert_eq!(html("<div>(name)</div>"), "<div></div>");
        assert_eq!(hydratable_html("<div>(name)</div>"), "<div></div>");
        assert_eq!(
            holes("<div>(name)</div>"),
            [(
                String::new(),
                HoleKind::Child,
                String::new(),
                false,
                NO_EFFECT_GROUP,
            )],
        );
    }

    #[test]
    fn a_dynamic_child_beside_text_is_anchored() {
        assert_eq!(html(r#"<div>"hi " (name)</div>"#), "<div>hi <!></div>");
        assert_eq!(paths(r#"<div>"hi " (name)</div>"#), ["cn"]);
    }

    #[test]
    fn two_adjacent_dynamic_children_get_an_anchor_each() {
        assert_eq!(html(r#"<div>"a" (x) (y)</div>"#), "<div>a<!><!></div>");
        assert_eq!(paths(r#"<div>"a" (x) (y)</div>"#), ["cn", "cnn"]);
    }

    #[test]
    fn anchors_shift_the_walks_of_later_siblings() {
        // The anchor for `(x)` becomes a node, so the `<b>` after it is the
        // third child and the hole inside it walks through the shifted path.
        assert_eq!(
            html(r#"<div>"a" (x) <b>(y)</b></div>"#),
            "<div>a<!><b></b></div>",
        );
        assert_eq!(paths(r#"<div>"a" (x) <b>(y)</b></div>"#), ["cn", "cnn"]);
    }

    // The marker cases below are transliterated from the pinned upstream
    // fixtures, whose expected template strings they reproduce exactly apart
    // from the closing tags upstream omits.

    #[test]
    fn a_hydratable_child_after_text_is_bracketed() {
        // `__dom_hydratable_fixtures__/textInterpolation`: `<span>Hello <!$><!/>`.
        assert_eq!(
            hydratable_html(r#"<span>"Hello " (name)</span>"#),
            "<span>Hello <!$><!/></span>",
        );
    }

    #[test]
    fn a_hydratable_child_before_text_is_bracketed() {
        // `<span>{greeting} John</span>` compiles to `<span><!$><!/> John`.
        assert_eq!(
            hydratable_html(r#"<span>(greeting) " John"</span>"#),
            "<span><!$><!/> John</span>",
        );
    }

    #[test]
    fn hydratable_children_never_share_a_marker_pair() {
        // `<span> {greeting}{name} </span>` compiles to
        // `<span> <!$><!/><!$><!/> `, one pair each, unlike the non-hydratable
        // path where adjacent expressions share an anchor upstream.
        assert_eq!(
            hydratable_html(r#"<span>" " (greeting)(name) " "</span>"#),
            "<span> <!$><!/><!$><!/> </span>",
        );
        assert_eq!(
            hydratable_html(r#"<span>" " (greeting) " " (name) " "</span>"#),
            "<span> <!$><!/> <!$><!/> </span>",
        );
    }

    #[test]
    fn a_hydratable_lone_child_gets_no_pair() {
        // `__dom_hydratable_fixtures__/conditionalExpressions`: every
        // `<div>{expr}</div>` compiles to the bare template `<div>`.
        assert_eq!(hydratable_html("<div>(simple)</div>"), "<div></div>");
        assert_eq!(hydratable_html(r"<div>if a { (b) }</div>"), "<div></div>");
    }

    #[test]
    fn a_marker_pair_takes_two_positions_in_the_walk() {
        let output = hydratable(r#"<span>"a" (x) "b" (y)</span>"#);
        assert_eq!(
            output.templates()[0].html(),
            "<span>a<!$><!/>b<!$><!/></span>",
        );
        let paths: Vec<_> = output.templates()[0]
            .holes()
            .iter()
            .map(|hole| hole.path().to_owned())
            .collect();
        // `a` is child 0, the first pair is 1 and 2, `b` is 3, the second pair
        // is 4 and 5.
        assert_eq!(paths, ["cn", "cnnnn"]);
    }

    #[test]
    fn hydratable_templates_say_so_and_carry_no_hydration_key() {
        let output = hydratable(r#"<div class="a">"x"</div>"#);
        assert!(output.templates()[0].hydratable());
        // The server writes `data-hk`; the client finds the node through the
        // hydration registry, so the template string never carries it.
        assert!(!output.templates()[0].html().contains("data-hk"));
        assert!(!lower(r#"<div>"x"</div>"#).templates()[0].hydratable());
        assert!(!html(r#"<div>"a" (x)</div>"#).contains("<!$>"));
    }

    #[test]
    fn a_runtime_expression_child_is_reactive() {
        assert_eq!(
            holes("<div>$(count)</div>"),
            [(
                String::new(),
                HoleKind::Child,
                String::new(),
                true,
                NO_EFFECT_GROUP,
            )],
        );
        // The value closure captures by move: a handle is one `Copy` word, so
        // copying it into the environment is what keeps the local unboxed.
        assert!(expanded("<div>$(count)</div>")
            .contains("effect (& __tc_root_0 , 0u32 , move || count)"));
    }

    #[test]
    fn a_dynamic_attribute_value_keeps_its_static_key() {
        assert_eq!(
            html(r#"<a href=(url) class="link">"go"</a>"#),
            r#"<a class="link">go</a>"#,
        );
        assert_eq!(
            holes(r#"<a href=(url) class="link">"go"</a>"#),
            [(
                String::new(),
                HoleKind::Attribute,
                "href".to_owned(),
                false,
                NO_EFFECT_GROUP,
            )],
        );
    }

    #[test]
    fn dynamism_is_per_attribute_and_per_child() {
        assert_eq!(
            html(r#"<div id="a" title=(t)><span class="b">"s"</span>(child)</div>"#),
            r#"<div id="a"><span class="b">s</span><!></div>"#,
        );
        assert_eq!(
            paths(r#"<div id="a" title=(t)><span class="b">"s"</span>(child)</div>"#),
            ["", "cn"],
        );
    }

    #[test]
    fn a_hole_deep_in_the_tree_walks_from_the_root() {
        assert_eq!(
            html(r#"<div><span>"a"</span><p><b>(x)</b></p></div>"#),
            "<div><span>a</span><p><b></b></p></div>",
        );
        assert_eq!(
            paths(r#"<div><span>"a"</span><p><b>(x)</b></p></div>"#),
            ["cnc"]
        );
    }

    #[test]
    fn holes_are_recorded_in_source_order() {
        assert_eq!(
            kinds(r"<button title=(t) @click=$(go())>(label)</button>"),
            [
                HoleKind::Attribute,
                HoleKind::DelegatedEvent,
                HoleKind::Child,
            ],
        );
    }

    #[test]
    fn delegated_events_are_collected_and_others_are_not() {
        let output = lower(r#"<button @click=$(go()) @focus=$(seen())>"go"</button>"#);
        let template = &output.templates()[0];
        assert_eq!(template.html(), "<button>go</button>");
        assert_eq!(
            template
                .holes()
                .iter()
                .map(|hole| (hole.kind(), hole.name()))
                .collect::<Vec<_>>(),
            [
                (HoleKind::DelegatedEvent, "click"),
                (HoleKind::Event, "focus"),
            ],
        );
        assert_eq!(template.events(), ["click"]);
    }

    #[test]
    fn repeated_delegated_events_are_listed_once_and_sorted() {
        let output =
            lower(r"<div @keyup=$(a())><button @click=$(b())></button><i @click=$(c())></i></div>");
        assert_eq!(output.templates()[0].events(), ["click", "keyup"]);
    }

    #[test]
    fn a_plain_expression_event_handler_is_lowered_like_a_runtime_one() {
        assert_eq!(
            holes(r#"<button @click=(handler)>"go"</button>"#),
            [(
                String::new(),
                HoleKind::DelegatedEvent,
                "click".to_owned(),
                false,
                NO_EFFECT_GROUP,
            )],
        );
    }

    #[test]
    fn a_bind_attribute_is_classified_from_the_contract_tables() {
        assert_eq!(
            kinds(r"<input :value=$(v) :disabled=$(d) :placeholder=$(p)>"),
            [HoleKind::Property, HoleKind::Property, HoleKind::Attribute],
        );
        assert_eq!(
            kinds(r"<div :class=$(c) :style=$(s)></div>"),
            [HoleKind::ClassList, HoleKind::Style],
        );
    }

    #[test]
    fn a_bind_attribute_is_reactive_only_when_written_with_a_dollar() {
        let reactive: Vec<_> = holes(r"<input :value=$(a) :title=(b)>")
            .into_iter()
            .map(|hole| hole.3)
            .collect();
        assert_eq!(reactive, [true, false]);
    }

    #[test]
    fn an_attribute_spread_becomes_one_hole_on_its_element() {
        assert_eq!(
            html(r#"<div (attrs) class="x"></div>"#),
            r#"<div class="x"></div>"#
        );
        assert_eq!(
            holes(r#"<div (attrs) class="x"></div>"#),
            [(
                String::new(),
                HoleKind::Spread,
                String::new(),
                false,
                NO_EFFECT_GROUP,
            )],
        );
    }

    #[test]
    fn reactive_holes_on_one_element_share_an_effect_group() {
        let groups: Vec<_> = holes(r"<div :title=$(a) :id=$(b) @click=$(c)>(d)</div>")
            .into_iter()
            .map(|hole| hole.4)
            .collect();
        // The two binds share group 0; the handler and the child insert own
        // their own computations.
        assert_eq!(groups, [0, 0, NO_EFFECT_GROUP, NO_EFFECT_GROUP]);
    }

    #[test]
    fn each_element_gets_its_own_effect_group() {
        let groups: Vec<_> = holes(r"<div :title=$(a)><span :title=$(b) :id=$(c)></span></div>")
            .into_iter()
            .map(|hole| hole.4)
            .collect();
        assert_eq!(groups, [0, 1, 1]);
    }

    #[test]
    fn a_signal_declaration_binds_its_name_from_its_ordinal() {
        let output = lower("signal count = 0; signal other = 1; <p>(count)</p>");
        assert_eq!(output.templates().len(), 1);
        assert_eq!(output.templates()[0].html(), "<p></p>");

        let expanded = output.expand().to_string();
        assert!(
            expanded.contains("let count = :: view_abi :: signal (0u32 , 0)"),
            "{expanded}",
        );
        assert!(
            expanded.contains("let other = :: view_abi :: signal (1u32 , 1)"),
            "{expanded}",
        );
        // The binding precedes the template it is read from.
        let signal = expanded.find("let count").expect("the signal is bound");
        let root = expanded.find("let __tc_root_0").expect("the root is bound");
        assert!(signal < root, "{expanded}");
    }

    #[test]
    fn a_let_binding_is_emitted_verbatim_before_the_templates() {
        let expanded = expanded(r"<div>(a)</div> let a = 1;");
        assert!(expanded.contains("let a = 1 ;"), "{expanded}");
        assert!(
            expanded.find("let a = 1 ;").unwrap() < expanded.find("let __tc_root_0").unwrap(),
            "{expanded}",
        );
    }

    #[test]
    fn each_root_element_becomes_its_own_template() {
        let output = lower(r#"<div>(a)</div><span>"b"</span>"#);
        assert_eq!(output.templates().len(), 2);
        assert_eq!(output.templates()[0].html(), "<div></div>");
        assert_eq!(output.templates()[1].html(), "<span>b</span>");

        let expanded = output.expand().to_string();
        assert!(expanded.contains("static __TC_TEMPLATE_0 :"), "{expanded}");
        assert!(expanded.contains("static __TC_TEMPLATE_1 :"), "{expanded}");
        assert!(
            expanded.contains("(__tc_root_0 , __tc_root_1)"),
            "{expanded}"
        );
    }

    #[test]
    fn the_client_body_instantiates_and_fills_in_hole_order() {
        let expanded = expanded(r"<a href=(url)>(label)</a>");
        assert!(
            expanded.contains("let __tc_root_0 = :: view_abi :: template (& __TC_TEMPLATE_0)"),
            "{expanded}",
        );
        assert!(
            expanded.contains(":: view_abi :: hole (& __tc_root_0 , 0u32 , url)"),
            "{expanded}",
        );
        assert!(
            expanded.contains(":: view_abi :: hole (& __tc_root_0 , 1u32 , label)"),
            "{expanded}",
        );
    }

    #[test]
    fn the_static_carries_no_link_section() {
        // rustc rejects a custom `link_section` on a static holding references;
        // the backend discovers the payload through the marker call's argument.
        let expanded = expanded(r#"<div>"x"</div>"#);
        assert!(!expanded.contains("link_section"), "{expanded}");
        assert!(
            expanded.contains("static __TC_TEMPLATE_0 : :: view_abi :: TemplateData"),
            "{expanded}",
        );
    }

    #[test]
    fn an_empty_view_lowers_to_nothing() {
        let output = lower("");
        assert!(output.templates().is_empty());
        assert_eq!(output.expand().to_string(), "{ () }");
    }

    #[test]
    fn text_outside_an_element_becomes_a_template_of_its_own() {
        assert_eq!(
            htmls(r#""hello" "there" <p>"x"</p>"#),
            ["hellothere", "<p>x</p>"]
        );
    }

    #[test]
    fn an_expression_outside_an_element_is_the_value_itself() {
        let output = lower("(value)");
        assert!(output.templates().is_empty());
        assert_eq!(output.expand().to_string(), "{ value }");
    }

    // Control flow.

    #[test]
    fn a_conditional_hands_its_test_over_on_its_own() {
        let output = lower(r#"<div>if flag { <b>"y"</b> } else { <i>"n"</i> }</div>"#);
        assert_eq!(
            output
                .templates()
                .iter()
                .map(Template::html)
                .collect::<Vec<_>>(),
            ["<div></div>", "<b>y</b>", "<i>n</i>"],
        );

        // Three closures, the test first: that is what lets the backend hoist it into a memo.
        let expanded = output.expand().to_string();
        assert!(
            expanded.contains(":: view_abi :: cond (|| flag , ||"),
            "{expanded}"
        );
        assert!(expanded.contains("__TC_TEMPLATE_0"), "{expanded}");
        assert!(expanded.contains("__TC_TEMPLATE_1"), "{expanded}");
        // A `cond` is its own accessor, so the hole is written once rather than
        // wrapped in an effect.
        assert!(!expanded.contains(":: view_abi :: effect"), "{expanded}");
    }

    #[test]
    fn a_test_that_binds_stays_one_closure() {
        // `let Some(n) = xs` is an expression only inside its own `if`, so it cannot be
        // handed over as `cond`'s test. Such a chain keeps the shape it always had.
        let expanded = expanded(r#"<div>if let Some(n) = xs { (n) } else { "none" }</div>"#);
        assert!(expanded.contains(":: view_abi :: effect"), "{expanded}");
        assert!(expanded.contains("if let Some (n) = xs"), "{expanded}");
        assert!(!expanded.contains(":: view_abi :: cond"), "{expanded}");
    }

    #[test]
    fn a_missing_else_renders_nothing() {
        let expanded = expanded(r#"<div>if flag { "y" }</div>"#);
        assert!(
            expanded.contains("|| :: view_abi :: content (())"),
            "{expanded}",
        );
    }

    #[test]
    fn an_else_if_chain_nests_without_an_extra_wrapper() {
        let output = lower(r#"<div>if a { "a" } else if b { "b" } else { "c" }</div>"#);
        // One template per branch, plus the element.
        assert_eq!(output.templates().len(), 4);
        // The outer `else` closure's body is the inner `cond`.
        let expanded = output.expand().to_string();
        assert!(
            expanded.contains("|| :: view_abi :: cond (|| b , ||"),
            "{expanded}",
        );
    }

    #[test]
    fn a_conditional_is_one_hole_however_many_branches_it_has() {
        assert_eq!(
            kinds(r#"<div>"x" if a { "a" } else { "b" }</div>"#),
            [HoleKind::Child],
        );
        assert_eq!(paths(r#"<div>"x" if a { "a" } else { "b" }</div>"#), ["cn"]);
    }

    #[test]
    fn a_match_keeps_its_arms_and_guards() {
        let output = lower(
            r#"<div>match state {
                Some(n) if n > 0 => <b>(n)</b>,
                _ => "none",
            }</div>"#,
        );
        let expanded = output.expand().to_string();
        assert!(expanded.contains("match state {"), "{expanded}");
        assert!(expanded.contains("Some (n) if n > 0 =>"), "{expanded}");
        assert!(
            expanded.contains("_ => :: view_abi :: content"),
            "{expanded}"
        );
    }

    #[test]
    fn a_match_arm_block_writes_its_children_into_the_arm() {
        let output = lower(r#"<div>match k { _ => { <b></b><i></i> }, }</div>"#);
        assert_eq!(output.templates().len(), 3);
        // Two roots in one arm are handed over as a pair.
        assert!(output
            .expand()
            .to_string()
            .contains("(__tc_root_1 , __tc_root_2)"),);
    }

    #[test]
    fn a_block_in_node_position_is_not_a_scope() {
        assert_eq!(html(r#"<div>{ "x" (y) }</div>"#), "<div>x<!></div>");
    }

    #[test]
    fn a_for_loop_maps_the_iterable_to_rows() {
        let output = lower(r"<ul>for item in items { <li>(item)</li> }</ul>");
        assert_eq!(
            output
                .templates()
                .iter()
                .map(Template::html)
                .collect::<Vec<_>>(),
            ["<ul></ul>", "<li></li>"],
        );

        // The loop stays Rust: one `push` per row into the list the marker starts.
        let expanded = output.expand().to_string();
        assert!(
            expanded.contains("let __tc_rows = :: view_abi :: list () ; for item in items {"),
            "{expanded}",
        );
        assert!(
            expanded.contains(":: view_abi :: push (& __tc_rows ,"),
            "{expanded}",
        );
        assert!(
            expanded.contains(":: view_abi :: hole (& __tc_root_1 , 0u32 , item)"),
            "{expanded}",
        );
    }

    #[test]
    fn a_loop_over_a_signal_is_filled_through_the_effect_marker() {
        // The accessor `effect` hands `_$insert` is what the runtime subscribes
        // to, so writing the signal renders the list again.
        let expanded = expanded(
            r"signal rows = 0; <ul>for item in rows.get() { <li>(item)</li> }</ul>",
        );
        assert!(
            expanded.contains(":: view_abi :: effect (& __tc_root_0 , 0u32 , move || {"),
            "{expanded}",
        );
        assert!(expanded.contains("for item in rows . get ()"), "{expanded}");
    }

    #[test]
    fn a_loop_over_a_signal_says_so_in_its_hole() {
        assert_eq!(
            holes(r"signal rows = 0; <ul>for item in rows.get() { <li>(item)</li> }</ul>"),
            [(
                String::new(),
                HoleKind::Child,
                String::new(),
                true,
                NO_EFFECT_GROUP,
            )],
        );
    }

    #[test]
    fn a_loop_over_anything_else_is_still_written_once() {
        // A view with signals in it does not make every loop reactive: only one
        // whose iterated expression names a signal.
        let expanded = expanded(
            r"signal count = 0; <ul>(count) for item in items { <li>(item)</li> }</ul>",
        );
        assert!(
            expanded.contains(":: view_abi :: hole (& __tc_root_0 , 1u32 , { let __tc_rows"),
            "{expanded}",
        );
        assert!(
            !expanded.contains(":: view_abi :: effect (& __tc_root_0 , 1u32"),
            "{expanded}",
        );
    }

    #[test]
    fn a_row_that_reads_a_signal_does_not_make_the_list_reactive() {
        // The `$(..)` is a reactive hole of the ROW's template and stays one:
        // re-running the list for it would rebuild every row and fire that hole
        // too. Only the iterated expression decides.
        let expanded =
            expanded(r"signal count = 0; <ul>for item in items { <li>$(count.get())</li> }</ul>");
        assert!(
            expanded.contains(":: view_abi :: hole (& __tc_root_0 , 0u32 , { let __tc_rows"),
            "{expanded}",
        );
        // The row's own hole is the reactive one.
        assert!(
            expanded.contains(":: view_abi :: effect (& __tc_root_1 , 0u32 , move || count . get ())"),
            "{expanded}",
        );
    }

    #[test]
    fn a_keyed_loop_over_a_signal_keeps_both_halves() {
        // The keyed marker is what preserves node identity across the re-runs the
        // effect marker is what causes; a reactive keyed loop needs both.
        let expanded = expanded(
            r"signal rows = 0; <ul>for item in rows.get() key (item.id) { <li>(item)</li> }</ul>",
        );
        assert!(expanded.contains(":: view_abi :: effect (& __tc_root_0"), "{expanded}");
        assert!(
            expanded.contains(":: view_abi :: push_keyed (& __tc_rows , item . id ,"),
            "{expanded}",
        );
    }

    #[test]
    fn a_signal_is_recognized_however_it_is_reached() {
        // `Sig` is `Copy` and reading it can be spelled any number of ways, so
        // naming the signal is the test rather than seeing a `.get()`.
        for source in [
            r"signal rows = 0; <ul>for x in rows.get() { <li>(x)</li> }</ul>",
            r"signal rows = 0; <ul>for x in visible(rows) { <li>(x)</li> }</ul>",
            r"signal rows = 0; <ul>for x in rows.get().iter().take(2) { <li>(x)</li> }</ul>",
        ] {
            assert!(
                expanded(source).contains(":: view_abi :: effect (& __tc_root_0"),
                "`{source}` was not made reactive",
            );
        }
    }

    #[test]
    fn control_flow_nests() {
        let output = lower(
            r"<ul>
                for item in items {
                    <li>if item.done { <b>(item.name)</b> } else { (item.name) }</li>
                }
            </ul>",
        );
        // The `<b>`, the `<li>` and the `<ul>`: the bare expression branch
        // needs no template of its own.
        assert_eq!(output.templates().len(), 3);
        let expanded = output.expand().to_string();
        assert!(expanded.contains("for item in items {"), "{expanded}");
        assert!(expanded.contains("cond (|| item . done"), "{expanded}");
    }

    #[test]
    fn a_binding_inside_a_branch_stays_in_that_branch() {
        let expanded = expanded(r"<div>if a { let n = 1; <b>(n)</b> }</div>");
        let binding = expanded
            .find("let n = 1 ;")
            .expect("the binding is emitted");
        let content = expanded
            .find(":: view_abi :: content")
            .expect("the branch is wrapped");
        assert!(binding > content, "{expanded}");
    }

    #[test]
    fn empty_text_writes_nothing_and_is_not_a_child() {
        // A template of empty HTML has no node to clone, so `""` must write none.
        assert_eq!(html(r#"<div>""</div>"#), "<div></div>");
        assert!(lower(r#"<div>match k { _ => "", }</div>"#)
            .templates()
            .iter()
            .all(|template| !template.html().is_empty()));
        // And it does not take a child position: `(x)` is still the only child, so it needs no
        // anchor.
        assert_eq!(paths(r#"<div>"" (x)</div>"#), [""]);
    }

    #[test]
    fn a_declaration_does_not_count_as_a_child() {
        // The `signal` adds no node, so `(count)` is still the only child and
        // needs no marking.
        assert_eq!(html("<p>signal count = 0; (count)</p>"), "<p></p>");
        assert_eq!(paths("<p>signal count = 0; (count)</p>"), [""]);
    }

    #[test]
    fn two_expansions_of_one_view_are_identical() {
        let source = r#"
            signal count = 0;
            <div :class=$(theme) @click=$(bump())>
                "count: " (count)
                <ul>for item in items { <li>(item)</li> }</ul>
                if count > 0 { <b>"many"</b> } else { "none" }
            </div>
        "#;
        assert_eq!(expanded(source), expanded(source));
        assert_eq!(
            hydratable(source).expand().to_string(),
            hydratable(source).expand().to_string(),
        );
    }

    // Key-plan lockstep.

    #[test]
    fn the_emitter_and_the_key_plan_walk_control_flow_in_step() {
        // Every key site, nested: a signal, template roots, and reactive scopes
        // in an element, a loop, a conditional and a match.
        let source = r#"
            signal count = 0;
            <ul>
                for item in items {
                    <li>
                        if item.done { <i>$(item.name)</i> }
                        else { match item.kind { _ => <b>"?"</b>, } }
                    </li>
                }
            </ul>
            <p>$(count)</p>
        "#;
        let view: View = syn::parse_str(source).unwrap();
        let plan = KeyPlan::build(&view.nodes);
        let mut writer = DomWriter::new(plan.cursor(), true);
        view.nodes.write(&mut writer);
        if let Err(error) = writer.finish() {
            panic!("{error}");
        }
        // Every key the plan numbered was claimed by exactly one site, in the plan's own order:
        // the signal, `<ul>`, the loop, `<li>`, the `if`, `<i>`, `$(item.name)`, the `match`,
        // `<b>`, `<p>`, `$(count)`. Each branch body starts a template of its own, which is why
        // the elements inside them are roots and not children of the element around the branch.
        assert_eq!(
            plan.keys().iter().map(Key::site).collect::<Vec<_>>(),
            [
                KeySite::Signal,
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
            ]
        );
    }

    #[test]
    fn an_emitter_out_of_step_with_its_plan_is_reported() {
        let view: View = syn::parse_str("<div></div>").unwrap();
        let other: View = syn::parse_str("signal a = 0; <div></div>").unwrap();
        let plan = KeyPlan::build(&other.nodes);
        let mut writer = DomWriter::new(plan.cursor(), false);
        view.nodes.write(&mut writer);
        let error = writer.finish().err().expect("the disagreement is reported");
        assert!(error.to_string().contains("key plan disagree"), "{error}");
    }

    // Errors.

    // A literal element name. `<"tag">` was accepted and lowered with nothing
    // reading the text, which `G2-CHECKLIST.md` called the more dangerous of its
    // six findings because it is untested BEHAVIOUR rather than an untested
    // refusal. The decision is to keep lowering it -- a literal tag is as static
    // as an identifier one -- and to read the text. See `lower/element.rs`.

    #[test]
    fn a_literal_element_name_lowers_like_an_identifier() {
        let literal = expanded(r#"<"my-tag"></"my-tag">"#);
        let ident = expanded(r"<my-tag></my-tag>");
        assert!(literal.contains("my-tag"), "{literal}");
        assert_eq!(literal, ident, "the two spellings are the same element");
    }

    #[test]
    fn a_literal_element_name_that_is_not_a_tag_name_is_rejected() {
        // The value reaches the template's html verbatim, so this one would
        // close the element and open a `<script>`. The refusal quotes the text,
        // because the span points at a string literal whose content is the bug.
        let error = error(r#"<"><script>"></"><script>">"#);
        assert!(error.contains("is not an html tag name"), "{error}");
        assert!(error.contains("><script>"), "{error}");
    }

    #[test]
    fn a_literal_element_name_may_not_be_a_void_element() {
        // `ElementName::is_void_element` answers `false` for every literal, so
        // the grammar has already parsed this as a `Normal` element with a
        // closing tag and the emitter would write `<br></br>`. The decision was
        // made before the emitter saw it, so this refuses and says what to
        // write instead.
        let error = error(r#"<"br"></"br">"#);
        assert!(error.contains("is a void element"), "{error}");
        assert!(error.contains("Write `<br>`"), "{error}");
    }

    #[test]
    fn unsupported_nodes_report_a_spanned_error() {
        for (source, what) in [
            (r"<!DOCTYPE html>", "a doctype declaration"),
            (r"<(tag)></(tag)>", "an element with an expression name"),
        ] {
            let error = error(source);
            assert!(
                error.contains(what) && error.contains("not yet supported by the dom emitter"),
                "`{source}` reported `{error}`",
            );
        }
    }

    #[test]
    fn a_keyed_loop_appends_through_the_keyed_marker() {
        let expanded = expanded(r"<ul>for item in items key (item.id) { <li>(item)</li> }</ul>");
        // The key is evaluated inside the loop, where the pattern's bindings are
        // in scope, and it reaches `push_keyed` beside the row it belongs to.
        assert!(
            expanded.contains(":: view_abi :: push_keyed (& __tc_rows , item . id ,"),
            "{expanded}",
        );
        assert!(!expanded.contains(":: view_abi :: push ("), "{expanded}");
    }

    #[test]
    fn an_unkeyed_loop_still_appends_through_the_plain_marker() {
        // The clause is what selects the marker, so a loop without one is
        // untouched by the keyed lowering.
        let expanded = expanded(r"<ul>for item in items { <li>(item)</li> }</ul>");
        assert!(expanded.contains(":: view_abi :: push (& __tc_rows ,"), "{expanded}");
        assert!(!expanded.contains("push_keyed"), "{expanded}");
    }

    #[test]
    fn a_component_at_the_top_level_has_no_element_to_go_in() {
        let error = error(r#"card(title: "t")"#);
        assert!(error.contains("no element to insert it into"), "{error}");
    }

    #[test]
    fn a_components_children_are_not_props_yet() {
        let error = error(r#"<div>card(title: "t", <p></p>)</div>"#);
        assert!(error.contains("child nodes of a component"), "{error}");
    }

    #[test]
    fn a_runtime_expression_is_not_a_component_prop_yet() {
        let error = error(r"<div>card(title: $(name))</div>");
        assert!(error.contains("not a component prop yet"), "{error}");
    }

    #[test]
    fn unsupported_attributes_report_a_spanned_error() {
        for (source, what) in [
            (
                r"<div (name)=(value)></div>",
                "an attribute with an expression name",
            ),
            (
                r"<div :(name)=(value)></div>",
                "a bind attribute with an expression name",
            ),
            (
                r"<div @(name)=(handler)></div>",
                "an event handler with an expression name",
            ),
            (
                r#"<div @click="alert(1)"></div>"#,
                "an event handler written as JavaScript",
            ),
            (
                r#"<div if a { class="x" }></div>"#,
                "`if` in an attribute list",
            ),
            (
                r#"<div for x in xs { (x)="y" }></div>"#,
                "`for` in an attribute list",
            ),
            (
                r#"<div match v { _ => class="x", }></div>"#,
                "`match` in an attribute list",
            ),
            (
                r"<div let a = 1; class=(a)></div>",
                "`let` in an attribute list",
            ),
        ] {
            let error = error(source);
            assert!(
                error.contains(what) && error.contains("not yet supported by the dom emitter"),
                "`{source}` reported `{error}`",
            );
        }
    }

    #[test]
    fn a_reactive_node_with_no_enclosing_element_or_effect_is_rejected() {
        for source in ["$(value)", r#"if flag { "y" }"#] {
            assert!(
                error(source).contains("has no element to insert it into"),
                "`{source}` was accepted",
            );
        }
    }

    #[test]
    fn a_branch_that_is_itself_control_flow_is_the_branch_value() {
        // The inner conditional is the outer branch's value, with no hole of its
        // own: it is a `cond` inside a `cond`'s branch closure, which is how the
        // reference compiler nests its memos too.
        let expanded = expanded(r#"<div>if a { if b { "x" } else { "y" } }</div>"#);
        assert!(
            expanded
                .contains("cond (|| a , || :: view_abi :: content ({ :: view_abi :: cond (|| b ,"),
            "{expanded}"
        );
    }

    #[test]
    fn a_leading_cx_argument_is_rejected() {
        assert!(error("cx => <div></div>").contains("a leading `cx =>` argument"));
    }

    #[test]
    fn every_unsupported_node_in_one_view_is_reported() {
        // One expansion reports every construct it cannot lower, not just the
        // first, so a view is fixed in one pass.
        let view: View =
            syn::parse_str(r"<div (name)=(v) @(other)=(h)><!DOCTYPE html></div>").unwrap();
        let error = DomOutput::build(&view, false)
            .err()
            .expect("it is rejected");
        assert_eq!(error.into_iter().count(), 3);
    }

    #[test]
    fn a_component_fills_a_hole_of_its_own_kind() {
        assert_eq!(
            holes(r#"<div>card(title: "t")</div>"#),
            [(
                "".to_owned(),
                HoleKind::Component,
                String::new(),
                false,
                NO_EFFECT_GROUP
            )],
        );
    }

    #[test]
    fn a_component_takes_a_child_marker_like_any_other_child() {
        // A component is inserted where it is written, so it is anchored the way
        // a dynamic child is: the sole child needs no anchor, a sibling does.
        assert_eq!(paths(r#"<div>card()</div>"#), [""]);
        assert_eq!(paths(r#"<div>"a" card()</div>"#), ["cn"]);
        assert_eq!(
            hydratable_html(r#"<div>"a" card()</div>"#),
            "<div>a<!$><!/></div>",
        );
    }

    #[test]
    fn a_component_call_lowers_to_the_closure_the_runtime_calls() {
        // This is the shape the backend reads: a closure taking nothing, so the
        // component's key context can be opened before its view is built, and
        // props handed over as the struct both halves declare, so the call does
        // not depend on the order the props were declared in.
        let output = lower(r#"<div>my::card(title: (name), count: 3)</div>"#);
        let value = output.templates()[0].holes()[0].value().to_string();
        assert_eq!(
            value,
            "move || my :: card (my :: CardProps { title : (name) , count : 3 , })",
        );
    }

    #[test]
    fn a_component_with_no_props_still_builds_its_props_struct() {
        let output = lower(r#"<div>card()</div>"#);
        assert_eq!(
            output.templates()[0].holes()[0].value().to_string(),
            "move || card (CardProps { })",
        );
    }

    #[test]
    fn a_components_props_are_named_in_the_order_they_are_written() {
        // The struct makes the order immaterial to the callee, but the emitter
        // still evaluates them where they were written.
        let output = lower(r#"<div>card(count: 3, title: (name))</div>"#);
        assert_eq!(
            output.templates()[0].holes()[0].value().to_string(),
            "move || card (CardProps { count : 3 , title : (name) , })",
        );
    }

    #[test]
    fn a_component_hole_is_filled_by_the_component_marker() {
        // `component`, not `hole` and not `effect`. Not `effect`, because a
        // component is created once and whatever reactivity it has is inside the
        // props it was handed. Not `hole`, because that marker does not CALL its
        // value: the monomorphization collector only walks into a closure whose
        // call some body names, so a component reached through `hole` would have
        // its body, and everything it calls, missing from the program.
        let expanded = expanded(r#"<div>card(title: (name))</div>"#);
        assert!(
            expanded.contains(
                ":: view_abi :: component (& __tc_root_0 , 0u32 , move || card (CardProps { title : (name) , }))"
            ),
            "{expanded}",
        );
    }

    #[test]
    fn a_component_inside_a_branch_is_a_hole_of_the_branchs_own_template() {
        let output = lower(r#"<ul>if flag { <li>card(title: (t))</li> }</ul>"#);
        assert_eq!(
            output
                .templates()
                .iter()
                .map(|template| template.holes().iter().map(Hole::kind).collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            [vec![HoleKind::Child], vec![HoleKind::Component]],
        );
    }

    #[test]
    fn the_template_static_serializes_to_the_documented_token_form() {
        // The backend's constant reader is written against exactly this shape.
        let output = lower(r#"<button class="b" @click=$(go())>"hi " (name)</button>"#);
        let data = output.templates()[0]
            .data(&format_ident!("__TC_TEMPLATE_0"))
            .to_string();
        assert_eq!(
            data,
            concat!(
                "static __TC_TEMPLATE_0 : :: view_abi :: TemplateData = :: view_abi :: TemplateData ",
                "{ abi : :: view_abi :: ABI_VERSION , ",
                "html : \"<button class=\\\"b\\\">hi <!></button>\" , ",
                "hydratable : false , ",
                "is_import_node : false , ",
                "is_svg : false , ",
                "holes : & [",
                ":: view_abi :: Hole { path : \"\" , kind : :: view_abi :: HoleKind :: DelegatedEvent , name : \"click\" , reactive : false , effect_group : 4294967295u32 , } , ",
                ":: view_abi :: Hole { path : \"cn\" , kind : :: view_abi :: HoleKind :: Child , name : \"\" , reactive : false , effect_group : 4294967295u32 , }",
                "] , ",
                "events : & [\"click\"] , ",
                "} ;",
            ),
        );
    }
}


/// Grammar coverage: every `Node` and `AttributeNode` variant, classified.
///
/// `contract/G2-CHECKLIST.md` gate item 2 found six grammar variants in neither
/// the "lowered" nor the "refused and asserted" bucket, and named the structural
/// cause rather than only the six: **nothing kept a refusal and its assertion in
/// sync.** The refusal lives in `lower/`, the assertion lived in a hand written
/// table in the test module above, and no mechanism connected them -- so closing
/// the six by adding six rows would have reproduced the seventh.
///
/// This module is the mechanism. It holds three locks, and a variant has to pass
/// all three:
///
/// 1. **A wildcard-free `match` per enum** ([`node_coverage`],
///    [`attribute_coverage`]). A variant added upstream stops this file
///    compiling, at the arm that has to classify it. The `lower/` matches are
///    wildcard free for the same reason, but they answer "what do I emit",
///    which a refusal can satisfy silently; these answer "and what proves it".
/// 2. **A row per variant**, and the row count is asserted against a constant
///    beside each match. Fixing lock 1 without adding a row fails here, which is
///    the step that used to be skipped.
/// 3. **The row's source must actually produce the variant it claims.** The
///    source is parsed and the variant recovered from the AST, so a row cannot
///    be satisfied by a source that exercises something else -- which is how a
///    plausible looking table row can assert nothing at all.
///
/// # The third classification
///
/// Writing this found that two of the six were not gaps. `Node::Continue` and
/// `Node::Break` are refused by the GRAMMAR'S OWN PARSER
/// (`grammar/src/view/node.rs:139-148`, "`continue` is currently not
/// supported"), so no `view!` body can produce one and `view-dom`'s arms for
/// them are unreachable. A refusal nobody can reach is not an untested branch,
/// and calling it one would have overstated the gate.
///
/// So there are three buckets, not two, and [`Coverage::Unreachable`] carries an
/// assertion of its own: the source must fail to PARSE. If the grammar ever
/// starts accepting `continue` in a view body, that assertion fails and points
/// straight at the arm in `lower/node.rs` that then becomes live.
#[cfg(test)]
mod coverage {
    use topcoat_view_grammar::{
        attributes::AttributeNode,
        view::{Element, ElementName, Node, View},
    };

    use crate::DomOutput;

    /// What the dom emitter does with one grammar variant.
    #[derive(Debug, PartialEq, Eq)]
    enum Coverage {
        /// Lowered. Building the source must succeed.
        Lowered,
        /// Refused, carrying this text in a spanned error. Building the source
        /// must fail, and say this.
        Refused(&'static str),
        /// The grammar's parser never produces it, so the emitter's arm cannot
        /// run. Parsing the source must FAIL, with this text.
        Unreachable(&'static str),
    }

    /// The number of `view::Node` variants. Lock 2: this is bumped by hand, and
    /// the compile error from lock 1 is what asks for it.
    const NODE_VARIANTS: usize = 14;

    /// The number of `attributes::AttributeNode` variants. Lock 2, again.
    const ATTRIBUTE_VARIANTS: usize = 11;

    /// The number of `view::ElementName` variants. Lock 2, once more -- and this
    /// is the enum the whole exercise came from. `LitStr` was accepted and
    /// lowered with nothing checking the text and nothing testing it, because
    /// `lower/element.rs` never matched on `ElementName` at all: it asked the
    /// grammar's `string_name()`, which collapses `Ident` and `LitStr` into one
    /// `Option`. A helper that answers for two variants at once is exactly the
    /// shape these locks exist to catch.
    const ELEMENT_NAME_VARIANTS: usize = 3;

    /// `view::Node`, classified. **Wildcard free on purpose** -- see the module
    /// docs.
    fn node_coverage(node: &Node) -> (&'static str, Coverage) {
        match node {
            Node::Text(_) => ("Text", Coverage::Lowered),
            Node::Element(_) => ("Element", Coverage::Lowered),
            Node::Component(_) => ("Component", Coverage::Lowered),
            Node::Expr(_) => ("Expr", Coverage::Lowered),
            Node::RuntimeExpr(_) => ("RuntimeExpr", Coverage::Lowered),
            Node::If(_) => ("If", Coverage::Lowered),
            Node::Local(_) => ("Local", Coverage::Lowered),
            Node::ForLoop(_) => ("ForLoop", Coverage::Lowered),
            Node::Match(_) => ("Match", Coverage::Lowered),
            Node::Block(_) => ("Block", Coverage::Lowered),
            Node::SignalDecaration(_) => ("SignalDecaration", Coverage::Lowered),
            Node::DocumentType(_) => ("DocumentType", Coverage::Refused("a doctype declaration")),
            Node::Continue(_) => (
                "Continue",
                Coverage::Unreachable("`continue` is currently not supported"),
            ),
            Node::Break(_) => (
                "Break",
                Coverage::Unreachable("`break` is currently not supported"),
            ),
        }
    }

    /// `attributes::AttributeNode`, classified. **Wildcard free on purpose.**
    fn attribute_coverage(node: &AttributeNode) -> (&'static str, Coverage) {
        match node {
            AttributeNode::Attribute(_) => ("Attribute", Coverage::Lowered),
            AttributeNode::Spread(_) => ("Spread", Coverage::Lowered),
            AttributeNode::BindAttribute(_) => ("BindAttribute", Coverage::Lowered),
            AttributeNode::EventHandler(_) => ("EventHandler", Coverage::Lowered),
            AttributeNode::If(_) => ("If", Coverage::Refused("`if` in an attribute list")),
            AttributeNode::ForLoop(_) => ("ForLoop", Coverage::Refused("`for` in an attribute list")),
            AttributeNode::Match(_) => ("Match", Coverage::Refused("`match` in an attribute list")),
            AttributeNode::Local(_) => ("Local", Coverage::Refused("`let` in an attribute list")),
            AttributeNode::Block(_) => ("Block", Coverage::Refused("a block in an attribute list")),
            AttributeNode::Continue(_) => {
                ("Continue", Coverage::Refused("`continue` in an attribute list"))
            }
            AttributeNode::Break(_) => {
                ("Break", Coverage::Refused("`break` in an attribute list"))
            }
        }
    }

    /// `view::ElementName`, classified. **Wildcard free on purpose.**
    fn element_name_coverage(name: &ElementName) -> (&'static str, Coverage) {
        match name {
            ElementName::Ident(_) => ("Ident", Coverage::Lowered),
            ElementName::LitStr(_) => ("LitStr", Coverage::Lowered),
            ElementName::Expr(_) => (
                "Expr",
                Coverage::Refused("an element with an expression name"),
            ),
        }
    }

    /// One view body per `ElementName` variant.
    const ELEMENT_NAME_SOURCES: &[(&str, &str)] = &[
        ("Ident", r"<div></div>"),
        ("LitStr", r#"<"my-tag"></"my-tag">"#),
        ("Expr", r"<(tag)></(tag)>"),
    ];

    /// One view body per `Node` variant. Lock 3 checks that each really produces
    /// the variant its row names.
    const NODE_SOURCES: &[(&str, &str)] = &[
        ("Text", r#"<div>"hello"</div>"#),
        ("Element", r"<div></div>"),
        ("Component", r#"<div>card(title: "t")</div>"#),
        ("Expr", r"<div>(value)</div>"),
        ("RuntimeExpr", r"<div>$(value)</div>"),
        ("If", r#"<div>if flag { "y" }</div>"#),
        ("Local", r"<div>let a = 1; (a)</div>"),
        ("ForLoop", r"<div>for x in xs { <li>(x)</li> }</div>"),
        ("Match", r#"<div>match v { _ => "x", }</div>"#),
        ("Block", r#"<div>{ "x" }</div>"#),
        ("SignalDecaration", r"signal count = 0; <div>(count)</div>"),
        ("DocumentType", r"<div><!DOCTYPE html></div>"),
        ("Continue", r"<div>continue;</div>"),
        ("Break", r"<div>break;</div>"),
    ];

    /// One view body per `AttributeNode` variant.
    const ATTRIBUTE_SOURCES: &[(&str, &str)] = &[
        ("Attribute", r#"<div class="x"></div>"#),
        ("Spread", r"<div (attrs)></div>"),
        ("BindAttribute", r"<div :hidden=(flag)></div>"),
        ("EventHandler", r"<div @click=(handler)></div>"),
        ("If", r#"<div if a { class="x" }></div>"#),
        ("ForLoop", r#"<div for x in xs { (x)="y" }></div>"#),
        ("Match", r#"<div match v { _ => class="x", }></div>"#),
        ("Local", r"<div let a = 1; class=(a)></div>"),
        ("Block", r#"<div { class="x" }></div>"#),
        ("Continue", r"<div continue;></div>"),
        ("Break", r"<div break;></div>"),
    ];

    /// The nodes a row's source can put its variant in: the top level of the
    /// view, and the direct children of a top-level element.
    ///
    /// Deliberately shallow. A general walker would need an arm per nesting
    /// construct and would be a fourth thing to keep in step; making the sources
    /// shallow instead costs nothing, because every variant can appear at one of
    /// these two positions.
    fn shallow_nodes(view: &View) -> Vec<&Node> {
        let mut out = Vec::new();
        for node in &view.nodes {
            out.push(node);
            if let Node::Element(element) = node {
                if let Element::Normal { children, .. } = element.as_ref() {
                    out.extend(children.iter());
                }
            }
        }
        out
    }

    /// The attributes of the view's top-level elements.
    fn shallow_attributes(view: &View) -> Vec<&AttributeNode> {
        let mut out = Vec::new();
        for node in &view.nodes {
            if let Node::Element(element) = node {
                out.extend(element.attributes().items.iter());
            }
        }
        out
    }

    /// The three locks, for `view::Node`.
    #[test]
    fn every_node_variant_is_classified_and_proven() {
        assert_eq!(
            NODE_SOURCES.len(),
            NODE_VARIANTS,
            "`view::Node` has {NODE_VARIANTS} variants and this table has {}. If the compiler just \
             asked for a new arm in `node_coverage`, this is the row that goes with it.",
            NODE_SOURCES.len(),
        );

        for (variant, source) in NODE_SOURCES {
            let parsed = syn::parse_str::<View>(source);

            // An unreachable variant is proven by the PARSE failing, which is
            // where the grammar refuses it. Nothing else about it is testable,
            // because no view can produce one.
            if let Some((_, Coverage::Unreachable(message))) = expected(variant) {
                let error = parsed
                    .err()
                    .unwrap_or_else(|| panic!("`{source}` parsed, so `{variant}` is reachable now"))
                    .to_string();
                assert!(
                    error.contains(message),
                    "`{source}` was refused with `{error}`, not `{message}`",
                );
                continue;
            }

            let view = parsed.unwrap_or_else(|e| panic!("`{source}` does not parse: {e}"));
            let found = shallow_nodes(&view)
                .into_iter()
                .map(node_coverage)
                .find(|(name, _)| name == variant)
                .unwrap_or_else(|| panic!("`{source}` contains no `Node::{variant}`"));

            check(source, found);
        }
    }

    /// The three locks, for `attributes::AttributeNode`.
    #[test]
    fn every_attribute_variant_is_classified_and_proven() {
        assert_eq!(
            ATTRIBUTE_SOURCES.len(),
            ATTRIBUTE_VARIANTS,
            "`attributes::AttributeNode` has {ATTRIBUTE_VARIANTS} variants and this table has {}.",
            ATTRIBUTE_SOURCES.len(),
        );

        for (variant, source) in ATTRIBUTE_SOURCES {
            let view = syn::parse_str::<View>(source)
                .unwrap_or_else(|e| panic!("`{source}` does not parse: {e}"));
            let found = shallow_attributes(&view)
                .into_iter()
                .map(attribute_coverage)
                .find(|(name, _)| name == variant)
                .unwrap_or_else(|| panic!("`{source}` contains no `AttributeNode::{variant}`"));

            check(source, found);
        }
    }

    /// The three locks, for `view::ElementName`.
    #[test]
    fn every_element_name_variant_is_classified_and_proven() {
        assert_eq!(
            ELEMENT_NAME_SOURCES.len(),
            ELEMENT_NAME_VARIANTS,
            "`view::ElementName` has {ELEMENT_NAME_VARIANTS} variants and this table has {}.",
            ELEMENT_NAME_SOURCES.len(),
        );

        for (variant, source) in ELEMENT_NAME_SOURCES {
            let view = syn::parse_str::<View>(source)
                .unwrap_or_else(|e| panic!("`{source}` does not parse: {e}"));
            let found = shallow_nodes(&view)
                .into_iter()
                .filter_map(|node| match node {
                    Node::Element(element) => Some(element.name()),
                    _ => None,
                })
                .map(element_name_coverage)
                .find(|(name, _)| name == variant)
                .unwrap_or_else(|| panic!("`{source}` contains no `ElementName::{variant}`"));

            check(source, found);
        }
    }

    /// The classification a row claims, for the one case that is decided before
    /// the source is parsed.
    fn expected(variant: &str) -> Option<(&'static str, Coverage)> {
        match variant {
            "Continue" => Some((
                "Continue",
                Coverage::Unreachable("`continue` is currently not supported"),
            )),
            "Break" => Some((
                "Break",
                Coverage::Unreachable("`break` is currently not supported"),
            )),
            _ => None,
        }
    }

    /// Builds `source` and asserts it does what its classification says.
    fn check(source: &str, (variant, coverage): (&'static str, Coverage)) {
        let view: View = syn::parse_str(source).unwrap();
        let result = DomOutput::build(&view, false);
        match coverage {
            Coverage::Lowered => {
                assert!(
                    result.is_ok(),
                    "`{variant}` is classified as lowered, but `{source}` reported `{}`",
                    result.err().map(|e| e.to_string()).unwrap_or_default(),
                );
            }
            Coverage::Refused(message) => {
                let error = result
                    .err()
                    .unwrap_or_else(|| {
                        panic!("`{variant}` is classified as refused, but `{source}` was accepted")
                    })
                    .to_string();
                assert!(
                    error.contains(message),
                    "`{variant}`: `{source}` reported `{error}`, not `{message}`",
                );
            }
            Coverage::Unreachable(_) => {
                unreachable!("an unreachable variant is never built: see the caller")
            }
        }
    }
}
