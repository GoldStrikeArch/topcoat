use std::cell::RefCell;
use std::rc::Rc;

use crate::{
    attributes::{AttributeNode, AttributeNodes, Attributes},
    template::{TemplateBlock, TemplateElse, TemplateForLoop, TemplateIf, TemplateMatch},
    view::{Element, Node, Nodes},
};

/// A position in a view that consumes a generated key.
///
/// Every emitter that lowers a view walks the same sites in the same order, so
/// the ordinal a site receives identifies it across emitters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeySite {
    /// An element that starts a template: one that no other element in the same
    /// body encloses. A control-flow branch and a component's children are
    /// bodies of their own, so their outermost elements start templates too.
    TemplateRoot,
    /// A `signal name = expr;` declaration.
    Signal,
    /// A construct whose body can run zero or more times: `if`, `for`, `match`,
    /// or a `$(...)` expression.
    ReactiveScope,
    /// An element whose tag name is an expression and needs a temporary to hold
    /// the evaluated name.
    ElementName,
}

impl KeySite {
    /// The number of distinct sites, i.e. the number of independent ordinal
    /// sequences a plan hands out.
    pub const COUNT: usize = 4;

    const fn index(self) -> usize {
        match self {
            Self::TemplateRoot => 0,
            Self::Signal => 1,
            Self::ReactiveScope => 2,
            Self::ElementName => 3,
        }
    }
}

/// One numbered site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    site: KeySite,
    index: u32,
    ordinal: u32,
}

impl Key {
    /// What kind of site this key belongs to.
    #[must_use]
    pub fn site(&self) -> KeySite {
        self.site
    }

    /// The key's position in the plan, counting every site.
    #[must_use]
    pub fn index(&self) -> u32 {
        self.index
    }

    /// The key's position among the sites of its own kind. This is the number
    /// an emitter puts in a generated name such as `__element_name_0`.
    #[must_use]
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }
}

/// The numbering of every key-consuming site in a view, built from one
/// pre-order traversal.
///
/// A plan is built once and read by each emitter through its own
/// [`KeyCursor`], which keeps the emitters in lockstep without either of them
/// counting for itself.
pub struct KeyPlan {
    keys: Vec<Key>,
}

impl KeyPlan {
    /// Numbers every key-consuming site in `nodes`.
    #[must_use]
    pub fn build(nodes: &Nodes) -> Self {
        let mut plan = PlanBuilder::default();
        walk_nodes(nodes, &mut plan);
        Self { keys: plan.keys }
    }

    /// Every key, in traversal order.
    #[must_use]
    pub fn keys(&self) -> &[Key] {
        &self.keys
    }

    /// The number of numbered sites.
    #[must_use]
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Returns `true` if the view has no key-consuming site.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// A cursor reading this plan from its first key.
    #[must_use]
    pub fn cursor(&self) -> KeyCursor<'_> {
        KeyCursor {
            keys: &self.keys,
            state: CursorState::default(),
        }
    }

    /// A cursor over this plan that every builder an emitter splits into can
    /// hold a clone of.
    ///
    /// Lowering a view rarely happens in one pass: a conditional or a loop body
    /// is usually built by a builder of its own and spliced back in. Each of
    /// those holds a clone of the same cursor, so the keys stay in the plan's
    /// order however the emitter is structured.
    #[must_use]
    pub fn shared_cursor(self: &Rc<Self>) -> SharedKeyCursor {
        SharedKeyCursor {
            plan: Rc::clone(self),
            state: Rc::new(RefCell::new(CursorState::default())),
        }
    }
}

/// Reads a [`KeyPlan`] one site at a time as an emitter walks the view.
pub struct KeyCursor<'a> {
    keys: &'a [Key],
    state: CursorState,
}

impl KeyCursor<'_> {
    /// Consumes the next key, which is expected to be for `site`.
    ///
    /// The returned key is always numbered from the cursor's own count, so a
    /// caller that walks the view in a different order than the plan still gets
    /// unique ordinals; [`desynced`](Self::desynced) then reports the
    /// disagreement instead of failing.
    pub fn take(&mut self, site: KeySite) -> Key {
        self.state.take(self.keys, site)
    }

    /// The site the next [`take`](Self::take) is expected to be for.
    #[must_use]
    pub fn peek(&self) -> Option<KeySite> {
        self.keys.get(self.state.index).map(Key::site)
    }

    /// The number of keys taken so far.
    #[must_use]
    pub fn position(&self) -> usize {
        self.state.index
    }

    /// Returns `true` if a [`take`](Self::take) ever asked for a site the plan
    /// did not have at that position. A cursor that ends up desynced means the
    /// emitter and the plan disagree about the view's shape.
    #[must_use]
    pub fn desynced(&self) -> bool {
        self.state.desynced
    }
}

/// A [`KeyCursor`] several builders read through at once.
///
/// Every clone advances the one shared position, so an emitter can hand a clone
/// to each sub-builder it creates and still consume the plan exactly once, in
/// order. Created with [`KeyPlan::shared_cursor`].
#[derive(Clone)]
pub struct SharedKeyCursor {
    plan: Rc<KeyPlan>,
    state: Rc<RefCell<CursorState>>,
}

impl SharedKeyCursor {
    /// Consumes the next key, which is expected to be for `site`. See
    /// [`KeyCursor::take`].
    #[must_use]
    pub fn take(&self, site: KeySite) -> Key {
        self.state.borrow_mut().take(&self.plan.keys, site)
    }

    /// The site the next [`take`](Self::take) is expected to be for.
    #[must_use]
    pub fn peek(&self) -> Option<KeySite> {
        self.plan.keys.get(self.state.borrow().index).map(Key::site)
    }

    /// The number of keys taken so far.
    #[must_use]
    pub fn position(&self) -> usize {
        self.state.borrow().index
    }

    /// The plan being read.
    #[must_use]
    pub fn plan(&self) -> &KeyPlan {
        &self.plan
    }

    /// Returns `true` if the emitter and the plan disagree about the view's
    /// shape: either a key was taken for a site the plan did not have there, or
    /// the walk ended without consuming every key.
    #[must_use]
    pub fn desynced(&self) -> bool {
        let state = self.state.borrow();
        state.desynced || state.index != self.plan.keys.len()
    }
}

impl KeySink for SharedKeyCursor {
    fn push(&mut self, site: KeySite) {
        let _ = self.take(site);
    }
}

/// The position and per-site counts a cursor keeps.
#[derive(Default)]
struct CursorState {
    index: usize,
    taken: [u32; KeySite::COUNT],
    desynced: bool,
}

impl CursorState {
    fn take(&mut self, keys: &[Key], site: KeySite) -> Key {
        let index = self.index;
        self.index += 1;

        let ordinal = self.taken[site.index()];
        self.taken[site.index()] += 1;

        if keys.get(index).map(Key::site) != Some(site) {
            self.desynced = true;
        }

        Key {
            site,
            index: u32::try_from(index).unwrap_or(u32::MAX),
            ordinal,
        }
    }
}

/// Receives the key-consuming sites of a view, in plan order.
///
/// Implement this to walk a view the way a plan is built without building one,
/// which is how an emitter keeps a cursor in step with sites it does not itself
/// emit anything for.
pub trait KeySink {
    /// Records that a site of kind `site` was reached.
    fn push(&mut self, site: KeySite);
}

/// Feeds every key-consuming site in `nodes` to `sink`, in plan order.
pub fn walk_nodes(nodes: &Nodes, sink: &mut dyn KeySink) {
    nodes.walk(&mut Walker {
        sink,
        element_depth: 0,
    });
}

/// Feeds every key-consuming site in `attributes` to `sink`, in plan order.
///
/// Attributes hold no elements, so this is the part of the walk an emitter that
/// writes attributes through a builder of their own still has to account for.
pub fn walk_attributes(attributes: &Attributes, sink: &mut dyn KeySink) {
    attributes.walk(&mut Walker {
        sink,
        element_depth: 0,
    });
}

/// Collects the keys during the traversal.
#[derive(Default)]
struct PlanBuilder {
    keys: Vec<Key>,
    ordinals: [u32; KeySite::COUNT],
}

impl KeySink for PlanBuilder {
    fn push(&mut self, site: KeySite) {
        let ordinal = self.ordinals[site.index()];
        self.ordinals[site.index()] += 1;
        self.keys.push(Key {
            site,
            index: u32::try_from(self.keys.len()).unwrap_or(u32::MAX),
            ordinal,
        });
    }
}

/// Carries the traversal state a walk needs on top of its sink.
struct Walker<'a> {
    sink: &'a mut dyn KeySink,
    /// How many elements enclose the node being visited. An element is a
    /// template root only at depth zero.
    element_depth: usize,
}

impl Walker<'_> {
    fn push(&mut self, site: KeySite) {
        self.sink.push(site);
    }

    /// Walks `f` as if it started a fresh view, so the elements inside it are
    /// template roots of their own.
    ///
    /// Two things start one: a component's children, which render inside a view
    /// the component builds, and the body of a control-flow branch, which is
    /// built and dropped on its own as the branch is taken or not. In both cases
    /// the elements around the site do not enclose what is inside it.
    fn in_new_template(&mut self, f: impl FnOnce(&mut Self)) {
        let depth = std::mem::take(&mut self.element_depth);
        f(self);
        self.element_depth = depth;
    }
}

/// A node that contributes keys to a [`KeyPlan`].
trait Walk {
    fn walk(&self, builder: &mut Walker<'_>);
}

impl Walk for Nodes {
    fn walk(&self, builder: &mut Walker<'_>) {
        for node in self {
            node.walk(builder);
        }
    }
}

impl Walk for Node {
    fn walk(&self, builder: &mut Walker<'_>) {
        match self {
            Self::Text(_)
            | Self::DocumentType(_)
            | Self::Expr(_)
            | Self::Local(_)
            | Self::Continue(_)
            | Self::Break(_) => {}
            Self::Element(inner) => inner.walk(builder),
            // A component renders a view of its own, so its children are not
            // enclosed by the elements around the invocation.
            Self::Component(inner) => {
                builder.in_new_template(|builder| inner.children.walk(builder));
            }
            Self::RuntimeExpr(_) => builder.push(KeySite::ReactiveScope),
            Self::If(inner) => inner.walk(builder),
            Self::ForLoop(inner) => inner.walk(builder),
            Self::Match(inner) => inner.walk(builder),
            Self::Block(inner) => inner.walk(builder),
            Self::SignalDecaration(_) => builder.push(KeySite::Signal),
        }
    }
}

impl Walk for Element {
    fn walk(&self, builder: &mut Walker<'_>) {
        if builder.element_depth == 0 {
            builder.push(KeySite::TemplateRoot);
        }
        if self.name().expr().is_some() {
            builder.push(KeySite::ElementName);
        }

        builder.element_depth += 1;
        self.attributes().walk(builder);
        for child in self.children() {
            child.walk(builder);
        }
        builder.element_depth -= 1;
    }
}

impl Walk for Attributes {
    fn walk(&self, builder: &mut Walker<'_>) {
        for item in &self.items {
            item.walk(builder);
        }
    }
}

impl Walk for AttributeNodes {
    fn walk(&self, builder: &mut Walker<'_>) {
        for item in self {
            item.walk(builder);
        }
    }
}

impl Walk for AttributeNode {
    fn walk(&self, builder: &mut Walker<'_>) {
        match self {
            Self::Attribute(_)
            | Self::Spread(_)
            | Self::BindAttribute(_)
            | Self::EventHandler(_)
            | Self::Local(_)
            | Self::Continue(_)
            | Self::Break(_) => {}
            Self::If(inner) => inner.walk(builder),
            Self::ForLoop(inner) => inner.walk(builder),
            Self::Match(inner) => inner.walk(builder),
            Self::Block(inner) => inner.walk(builder),
        }
    }
}

impl<T: Walk> Walk for TemplateBlock<T> {
    fn walk(&self, builder: &mut Walker<'_>) {
        self.children.walk(builder);
    }
}

impl<T: Walk> Walk for TemplateIf<T> {
    fn walk(&self, builder: &mut Walker<'_>) {
        builder.push(KeySite::ReactiveScope);
        builder.in_new_template(|builder| self.then_branch.walk(builder));
        if let Some(else_branch) = &self.else_branch {
            else_branch.walk(builder);
        }
    }
}

impl<T: Walk> Walk for TemplateElse<T> {
    fn walk(&self, builder: &mut Walker<'_>) {
        match self {
            // An `else if` is numbered as the scope it is: the nested `if`
            // pushes its own key and opens its own branches, so the chain adds
            // no template of its own.
            Self::ElseIf { template_if, .. } => template_if.walk(builder),
            Self::Else { then_branch, .. } => {
                builder.in_new_template(|builder| then_branch.walk(builder));
            }
        }
    }
}

impl<T: Walk> Walk for TemplateForLoop<T> {
    fn walk(&self, builder: &mut Walker<'_>) {
        builder.push(KeySite::ReactiveScope);
        builder.in_new_template(|builder| self.body.walk(builder));
    }
}

impl<B: Walk> Walk for TemplateMatch<B> {
    fn walk(&self, builder: &mut Walker<'_>) {
        builder.push(KeySite::ReactiveScope);
        for arm in &self.arms {
            builder.in_new_template(|builder| arm.body.walk(builder));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::View;

    fn plan(source: &str) -> KeyPlan {
        let view: View = syn::parse_str(source).unwrap();
        KeyPlan::build(&view.nodes)
    }

    fn sites(source: &str) -> Vec<KeySite> {
        plan(source).keys().iter().map(Key::site).collect()
    }

    /// The `(site, ordinal)` pairs a cursor hands out when driven through the
    /// whole plan in order, the way an emitter walking the view would.
    fn drive(plan: &KeyPlan) -> Vec<(KeySite, u32)> {
        let mut cursor = plan.cursor();
        let taken = plan
            .keys()
            .iter()
            .map(|key| {
                let key = cursor.take(key.site());
                (key.site(), key.ordinal())
            })
            .collect();
        assert!(!cursor.desynced());
        taken
    }

    #[test]
    fn empty_view_has_no_keys() {
        let plan = plan("");
        assert!(plan.is_empty());
        assert_eq!(plan.len(), 0);
    }

    #[test]
    fn only_outermost_elements_start_templates() {
        assert_eq!(
            sites("<div><span><b></b></span></div> <p></p>"),
            [KeySite::TemplateRoot, KeySite::TemplateRoot],
        );
    }

    #[test]
    fn expression_names_are_numbered_per_element() {
        let plan = plan("<(outer)><(inner)></(inner)></(outer)>");
        let names: Vec<_> = plan
            .keys()
            .iter()
            .filter(|key| key.site() == KeySite::ElementName)
            .map(Key::ordinal)
            .collect();
        assert_eq!(names, [0, 1]);
    }

    #[test]
    fn signal_declarations_are_numbered() {
        let plan = plan("signal a = 0; signal b = 1; <div></div>");
        assert_eq!(
            sites("signal a = 0; signal b = 1; <div></div>"),
            [KeySite::Signal, KeySite::Signal, KeySite::TemplateRoot],
        );
        assert_eq!(plan.keys()[1].ordinal(), 1);
        assert_eq!(plan.keys()[2].ordinal(), 0);
    }

    #[test]
    fn nested_control_flow_is_numbered_in_pre_order() {
        let plan = plan(
            r#"
                <ul>
                    for item in items {
                        <li>
                            if item.done { "x" } else { <b>"-"</b> }
                        </li>
                    }
                </ul>
                $(count)
            "#,
        );
        assert_eq!(
            plan.keys().iter().map(Key::site).collect::<Vec<_>>(),
            [
                // `<ul>`.
                KeySite::TemplateRoot,
                // `for`, then the `<li>` its body is a template for.
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                // The `if` inside the row, whose `else` is a template for `<b>`.
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                // `$(count)` outside the element.
                KeySite::ReactiveScope,
            ],
        );
        assert_eq!(
            plan.keys().iter().map(Key::ordinal).collect::<Vec<_>>(),
            [0, 0, 1, 1, 2, 2],
        );
    }

    #[test]
    fn control_flow_branches_restart_template_numbering() {
        // A branch is built and dropped as a unit, so the DOM emitter gives it a
        // template of its own however deep it sits, and the elements in it are
        // that template's roots rather than descendants of the `<div>`.
        assert_eq!(
            sites(r"<div>if cond { <span></span> } else { <b></b> }</div>"),
            [
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                KeySite::TemplateRoot,
            ],
        );
    }

    #[test]
    fn every_branch_body_is_one_template() {
        // One template per body the DOM emitter opens a scope for: the loop's
        // row, each arm of the `match`, and each branch of the `if`. The `else
        // if` opens no scope of its own; the `if` it chains to opens two.
        assert_eq!(
            sites("<div>for x in xs { <li></li> }</div>"),
            [
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot
            ],
        );
        assert_eq!(
            sites("<div>match x { 0 => <b></b>, _ => <i></i>, }</div>"),
            [
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                KeySite::TemplateRoot,
            ],
        );
        assert_eq!(
            sites(r#"<div>if a { <p></p> } else if b { <b></b> } else { "x" }</div>"#),
            [
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
            ],
        );
    }

    #[test]
    fn a_branch_body_encloses_its_own_elements() {
        // Only the outermost element of the branch is a root; the ones it
        // encloses are reached by walking down from it, exactly as in a view
        // that was written without the branch.
        assert_eq!(
            sites("<div>if cond { <span><b></b></span> }</div>"),
            [
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
            ],
        );
    }

    #[test]
    fn else_if_chains_number_each_condition() {
        assert_eq!(
            sites(r#"if a { "a" } else if b { "b" } else { "c" }"#),
            [KeySite::ReactiveScope, KeySite::ReactiveScope],
        );
    }

    #[test]
    fn attribute_position_control_flow_is_numbered() {
        assert_eq!(
            sites(r#"<div class="x" if cond { id="a" } for x in xs { (x)="y" }></div>"#),
            [
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::ReactiveScope,
            ],
        );
    }

    #[test]
    fn a_keyed_loop_is_numbered_like_any_other_loop() {
        // A key names which rendered row a row is; it does not change what a row
        // contains, so a keyed row is the same one template body as an unkeyed
        // one and both emitters walk it the same way.
        let plain = r"<ul>for item in items { <li>(item)</li> }</ul>";
        let keyed = r"<ul>for item in items key (item.id) { <li>(item)</li> }</ul>";
        assert_eq!(
            sites(plain),
            [
                KeySite::TemplateRoot,
                KeySite::ReactiveScope,
                KeySite::TemplateRoot,
            ],
        );
        assert_eq!(sites(plain), sites(keyed));
        assert_eq!(plan(plain).keys(), plan(keyed).keys());
    }

    #[test]
    fn component_children_start_their_own_templates() {
        assert_eq!(
            sites(r#"<div>card(title: "t", <p></p>)</div>"#),
            [KeySite::TemplateRoot, KeySite::TemplateRoot],
        );
    }

    #[test]
    fn ordinals_are_stable_across_builds() {
        let source = r#"
            signal count = 0;
            <(tag) class="x">
                for item in items { <li>(item)</li> }
                if flag { <(inner)></(inner)> }
            </(tag)>
        "#;
        let first = plan(source);
        let second = plan(source);
        assert_eq!(first.keys(), second.keys());
    }

    #[test]
    fn two_cursors_over_one_plan_agree() {
        let plan = plan(
            r"
                signal count = 0;
                <div>
                    for item in items { <(tag)>(item)</(tag)> }
                    match kind { _ => <b></b>, }
                </div>
            ",
        );
        assert_eq!(drive(&plan), drive(&plan));
    }

    #[test]
    fn a_cursor_that_asks_for_the_wrong_site_reports_a_desync() {
        let plan = plan("<div></div>");
        let mut cursor = plan.cursor();
        let key = cursor.take(KeySite::Signal);
        assert_eq!(key.ordinal(), 0);
        assert!(cursor.desynced());
    }

    #[test]
    fn peek_reports_the_next_site() {
        let plan = plan("signal a = 0; <div></div>");
        let mut cursor = plan.cursor();
        assert_eq!(cursor.peek(), Some(KeySite::Signal));
        cursor.take(KeySite::Signal);
        assert_eq!(cursor.peek(), Some(KeySite::TemplateRoot));
        cursor.take(KeySite::TemplateRoot);
        assert_eq!(cursor.peek(), None);
        assert_eq!(cursor.position(), 2);
    }
}
