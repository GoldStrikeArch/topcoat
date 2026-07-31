use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{Expr, Ident, Pat};
use topcoat_core_grammar::paths::{topcoat_error, topcoat_view};

use crate::attributes::Attributes;
use crate::view::Node;
#[cfg(feature = "dom")]
use crate::view::{KeySite, SharedKeyCursor, walk_attributes};

/// AST nodes that can emit themselves into a [`ViewWriter`].
pub(crate) trait WriteView {
    fn write(&self, writer: &mut ViewWriter);
}

/// Builds the `TokenStream` that a `view!` invocation expands to.
///
/// Adjacent literal markup is concatenated into `static_segment` and flushed as
/// a single write whenever a dynamic chunk (expression, control flow) appears.
pub(crate) struct ViewWriter {
    pub(self) chunks: Vec<Chunk>,
    static_segment: String,
    nested: bool,
    auto_increment: u32,
    #[cfg(feature = "dom")]
    keys: KeyState,
}

impl ViewWriter {
    pub fn new() -> Self {
        Self {
            chunks: Vec::new(),
            static_segment: String::new(),
            nested: false,
            auto_increment: 0,
            #[cfg(feature = "dom")]
            keys: KeyState::default(),
        }
    }

    pub fn new_nested() -> Self {
        Self {
            chunks: Vec::new(),
            static_segment: String::new(),
            nested: true,
            auto_increment: 0,
            #[cfg(feature = "dom")]
            keys: KeyState::default(),
        }
    }

    /// Numbers the sites this writer emits against `cursor`.
    ///
    /// Without a cursor a writer emits no hydration sites at all, which is what
    /// every build that does not lower views to the DOM does.
    #[cfg(feature = "dom")]
    pub fn with_key_cursor(cursor: SharedKeyCursor) -> Self {
        Self {
            keys: KeyState {
                cursor: Some(cursor),
                element_depth: 0,
            },
            ..Self::new()
        }
    }

    /// A fresh suffix for a generated identifier, unique within the expansion
    /// this writer builds. Counting is writer-local, so the same view body
    /// always produces the same identifiers.
    pub fn next_auto_increment(&mut self) -> u32 {
        let increment = self.auto_increment;
        self.auto_increment += 1;
        increment
    }

    /// A writer for a control-flow branch spliced into `self`, seeded so
    /// generated identifiers keep counting up from the ones already handed out.
    ///
    /// A branch is built and dropped as a unit, so it is a template of its own:
    /// its outermost elements carry keys even though an element of the
    /// surrounding view is written around them.
    fn body_writer(&self) -> Self {
        Self {
            auto_increment: self.auto_increment,
            #[cfg(feature = "dom")]
            keys: self.keys.in_new_template(),
            ..Self::new()
        }
    }

    /// Builds a self-contained view from `f` and returns its tokens, keeping
    /// the identifier counter shared with `self`.
    pub fn nested(&mut self, f: impl FnOnce(&mut ViewWriter)) -> TokenStream {
        let mut nested = Self {
            auto_increment: self.auto_increment,
            #[cfg(feature = "dom")]
            keys: self.keys.in_new_template(),
            ..Self::new_nested()
        };
        f(&mut nested);
        self.auto_increment = nested.auto_increment;
        nested.into_token_stream()
    }

    /// Takes the keys the opening tag of an element consumes and writes the
    /// element's hydration key when it starts a template.
    ///
    /// Only the outermost element of a template carries a key; everything below
    /// it is found by walking down from it. Call this after the tag name and
    /// before the attributes, which is where the key belongs in the tag.
    #[cfg_attr(not(feature = "dom"), allow(clippy::unused_self))]
    pub fn write_element_key(&mut self, expression_name: bool) {
        #[cfg(feature = "dom")]
        {
            // The key the site takes numbers it for the client emitter; the
            // server numbers its own as it renders, since only a render knows
            // which branches it took and how many rows a loop had.
            if self.keys.element_depth == 0 && self.keys.take(KeySite::TemplateRoot).is_some() {
                self.statement(quote! {
                    __parts.push_hydration_site(#topcoat_view::HydrationSite::TemplateRoot);
                });
            }
            if expression_name {
                self.keys.take(KeySite::ElementName);
            }
        }
        #[cfg(not(feature = "dom"))]
        let _ = expression_name;
    }

    /// Takes the keys the control flow inside `attributes` consumes.
    ///
    /// Attribute-position control flow is a reactive scope like any other, and
    /// the keys are numbered in the plan whether or not this writer emits
    /// anything for them.
    #[cfg_attr(not(feature = "dom"), allow(clippy::unused_self))]
    pub fn take_attribute_keys(&mut self, attributes: &Attributes) {
        #[cfg(feature = "dom")]
        if let Some(cursor) = &mut self.keys.cursor {
            walk_attributes(attributes, cursor);
        }
        #[cfg(not(feature = "dom"))]
        let _ = attributes;
    }

    /// Takes the key a reactive scope consumes: an `if`, `for`, `match`, or a
    /// `$(...)` expression.
    #[cfg_attr(not(feature = "dom"), allow(clippy::unused_self))]
    pub fn take_reactive_scope_key(&mut self) {
        #[cfg(feature = "dom")]
        self.keys.take(KeySite::ReactiveScope);
    }

    /// Takes the key a `signal` declaration consumes.
    #[cfg_attr(not(feature = "dom"), allow(clippy::unused_self))]
    pub fn take_signal_key(&mut self) {
        #[cfg(feature = "dom")]
        self.keys.take(KeySite::Signal);
    }

    /// Writes `children` as the children of the element being built.
    pub fn write_element_children(&mut self, children: &[Node]) {
        #[cfg(feature = "dom")]
        {
            self.keys.element_depth += 1;
        }
        self.write_children(children);
        #[cfg(feature = "dom")]
        {
            self.keys.element_depth -= 1;
        }
    }

    /// Writes `children` as the whole content of a view.
    ///
    /// The children of an element and the nodes of a view are both a list of
    /// siblings sharing one parent, so both mark their dynamic entries the same
    /// way.
    pub fn write_children(&mut self, children: &[Node]) {
        #[cfg(not(feature = "dom"))]
        for child in children {
            child.write(self);
        }

        // A dynamic child is bracketed by markers so the client can find the
        // range it owns, but only when it has siblings: a lone child is the
        // whole content of its parent and needs no delimiting.
        #[cfg(feature = "dom")]
        {
            let mark = self.keys.cursor.is_some()
                && children.iter().filter(|child| child.is_rendered()).count() > 1;
            for child in children {
                let marked = mark && child.is_dynamic();
                if marked {
                    self.write_hydration_marker("ChildStart");
                }
                child.write(self);
                if marked {
                    self.write_hydration_marker("ChildEnd");
                }
            }
        }
    }

    #[cfg(feature = "dom")]
    fn write_hydration_marker(&mut self, site: &str) {
        let site = Ident::new(site, Span::call_site());
        self.statement(quote! {
            __parts.push_hydration_site(#topcoat_view::HydrationSite::#site);
        });
    }

    pub fn flush(&mut self) {
        if !self.static_segment.is_empty() {
            let mut static_segment = String::new();
            std::mem::swap(&mut self.static_segment, &mut static_segment);
            self.chunks.push(Chunk::Static {
                string: static_segment,
            });
        }
    }

    pub fn write_str_unescaped(&mut self, s: &str) {
        self.static_segment.push_str(s);
    }

    /// Appends literal text escaped for a text node position.
    pub fn write_text(&mut self, s: &str) {
        self.write_in_context(topcoat_view::HtmlContext::Text, s);
    }

    /// Appends literal text escaped for a double-quoted attribute value
    /// position.
    pub fn write_attribute_value(&mut self, s: &str) {
        self.write_in_context(topcoat_view::HtmlContext::AttributeValue, s);
    }

    fn write_in_context(&mut self, context: topcoat_view::HtmlContext, s: &str) {
        let mut f = topcoat_view::Formatter::new(&mut self.static_segment);
        context.writer(&mut f).write_str(s);
    }

    pub fn write_expr(&mut self, kind: ExprKind, tokens: TokenStream) {
        self.flush();
        self.chunks.push(Chunk::Expr { kind, tokens });
    }

    pub fn local_binding(&mut self, pat: &Pat, expr: &Expr) {
        self.flush();
        self.chunks.push(Chunk::Local {
            pat: pat.clone(),
            expr: Box::new(expr.clone()),
        });
    }

    pub fn statement(&mut self, tokens: TokenStream) {
        self.flush();
        self.chunks.push(Chunk::Statement { tokens });
    }

    pub fn for_loop(&mut self, pat: &Pat, expr: &Expr, f: impl FnOnce(&mut ViewWriter)) {
        self.flush();
        let mut body = self.body_writer();
        f(&mut body);
        body.flush();
        self.auto_increment = body.auto_increment;
        self.chunks.push(Chunk::For {
            pat: pat.clone(),
            expr: Box::new(expr.clone()),
            body: Box::new(body),
        });
    }

    pub fn if_else(&mut self, expr: &Expr, f: impl FnOnce(&mut ViewWriter, &mut ViewWriter)) {
        self.flush();
        let mut then_branch = self.body_writer();
        let mut else_branch = self.body_writer();
        f(&mut then_branch, &mut else_branch);
        then_branch.flush();
        else_branch.flush();
        self.auto_increment = then_branch.auto_increment.max(else_branch.auto_increment);
        self.chunks.push(Chunk::If {
            expr: expr.clone(),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        });
    }

    pub fn match_expr(&mut self, expr: &Expr, f: impl FnOnce(&mut MatchArmsBuilder)) {
        self.flush();
        let mut builder = MatchArmsBuilder {
            arms: Vec::new(),
            auto_increment: self.auto_increment,
            // Every arm is a branch, so each is a template of its own.
            #[cfg(feature = "dom")]
            keys: self.keys.in_new_template(),
        };
        f(&mut builder);
        self.auto_increment = builder.auto_increment;
        self.chunks.push(Chunk::Match {
            expr: Box::new(expr.clone()),
            arms: builder.arms,
        });
    }

    pub fn into_token_stream(mut self) -> TokenStream {
        self.flush();

        let format_expr = {
            if self.chunks.is_empty() {
                // Optimized path: The view has no content.
                quote! { #topcoat_view::View::empty() }
            } else if self.chunks.len() == 1
                && let Chunk::Static { string } = &self.chunks[0]
            {
                quote! { #topcoat_view::View::unescaped_unchecked(#string) }
            } else {
                fn build_parts(chunks: &[Chunk]) -> TokenStream {
                    let mut output = TokenStream::new();
                    for chunk in chunks {
                        match chunk {
                            Chunk::Static { string } => {
                                // A view carrying hydration sites can never take
                                // the fully static path, so its literal markup
                                // has to render without naming the request
                                // context, which is not in scope everywhere a
                                // static view is written today.
                                #[cfg(feature = "dom")]
                                {
                                    quote! { __static(&mut __parts, #string); }
                                }
                                #[cfg(not(feature = "dom"))]
                                {
                                    let helper = ExprKind::Unescaped.helper();
                                    let tokens = quote! { #string };
                                    quote! { #helper(__cx, &mut __parts, #tokens); }
                                }
                            }
                            Chunk::Expr { kind, tokens } => {
                                let helper = kind.helper();
                                quote! { #helper(__cx, &mut __parts, #tokens); }
                            }
                            Chunk::Local { pat, expr } => {
                                quote! { let #pat = #expr; }
                            }
                            Chunk::Statement { tokens } => {
                                quote! { #tokens }
                            }
                            Chunk::If {
                                expr,
                                then_branch: then,
                                else_branch: r#else,
                            } => {
                                let then_branch = build_parts(&then.chunks);
                                let else_branch = build_parts(&r#else.chunks);
                                let else_branch = (!r#else.chunks.is_empty())
                                    .then(|| quote! { else { #else_branch } });
                                quote! {
                                    if #expr {
                                        #then_branch
                                    }
                                    #else_branch
                                }
                            }
                            Chunk::For { pat, expr, body } => {
                                let body = build_parts(&body.chunks);
                                quote! {
                                    for #pat in #expr {
                                        #body
                                    }
                                }
                            }
                            Chunk::Match { expr, arms } => {
                                let arm_tokens = arms.iter().map(|arm| {
                                    let pat = &arm.pat;
                                    let guard = arm.guard.as_ref().map(|g| quote! { if #g });
                                    let body = build_parts(&arm.body.chunks);
                                    quote! {
                                        #pat #guard => { #body }
                                    }
                                });
                                quote! {
                                    match #expr {
                                        #(#arm_tokens,)*
                                    }
                                }
                            }
                        }
                        .to_tokens(&mut output);
                    }
                    output
                }

                let statements = build_parts(&self.chunks);

                quote! {{
                    use #topcoat_view::internal::*;
                    let mut __parts = #topcoat_view::ViewParts::new();
                    #statements
                    #topcoat_view::View::new(__parts)
                }}
            }
        };

        if self.nested {
            format_expr
        } else {
            quote! { async { ::core::result::Result::<#topcoat_view::View, #topcoat_error::Error>::Ok(#format_expr) }.await }
        }
    }
}

/// What a [`ViewWriter`] needs to number the sites it writes against a key
/// plan.
///
/// Every builder a writer splits into holds a clone: the cursor is shared, so
/// the keys keep coming in plan order, while the element depth is per builder so
/// a body spliced into another view can start templates of its own.
#[cfg(feature = "dom")]
#[derive(Clone, Default)]
struct KeyState {
    cursor: Option<SharedKeyCursor>,
    element_depth: usize,
}

#[cfg(feature = "dom")]
impl KeyState {
    fn take(&mut self, site: KeySite) -> Option<crate::view::Key> {
        self.cursor.as_ref().map(|cursor| cursor.take(site))
    }

    /// The state for a body that renders a view of its own, so the elements in
    /// it are template roots rather than descendants of the elements around it.
    fn in_new_template(&self) -> Self {
        Self {
            cursor: self.cursor.clone(),
            element_depth: 0,
        }
    }
}

/// Identifies which `internal` helper a [`Chunk::Expr`] should be wrapped in
/// when emitted, so the generated code uses the matching `__*` function and
/// the corresponding `*ViewParts` trait.
#[derive(Copy, Clone)]
pub(crate) enum ExprKind {
    // Literal markup is written without the request context where a view can
    // carry hydration sites, so this kind has no emitter there.
    #[cfg_attr(feature = "dom", allow(dead_code))]
    Unescaped,
    Node,
    View,
    ElementName,
    Attribute,
    AttributeUnescaped,
    AttributeKey,
    AttributeValue,
    Attributes,
}

impl ExprKind {
    fn helper(self) -> Ident {
        let name = match self {
            Self::Unescaped => "__unescaped",
            Self::Node => "__node",
            Self::View => "__view",
            Self::ElementName => "__element_name",
            Self::Attribute => "__attribute",
            Self::AttributeUnescaped => "__attribute_unescaped",
            Self::AttributeKey => "__attribute_key",
            Self::AttributeValue => "__attribute_value",
            Self::Attributes => "__attributes",
        };
        Ident::new(name, Span::call_site())
    }
}

enum Chunk {
    Static {
        string: String,
    },
    Expr {
        kind: ExprKind,
        tokens: TokenStream,
    },
    Local {
        pat: Pat,
        expr: Box<Expr>,
    },
    Statement {
        tokens: TokenStream,
    },
    For {
        pat: Pat,
        expr: Box<Expr>,
        body: Box<ViewWriter>,
    },
    If {
        expr: Expr,
        then_branch: Box<ViewWriter>,
        else_branch: Box<ViewWriter>,
    },
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
    },
}

struct MatchArm {
    pat: Pat,
    guard: Option<Expr>,
    body: Box<ViewWriter>,
}

pub(crate) struct MatchArmsBuilder {
    arms: Vec<MatchArm>,
    auto_increment: u32,
    #[cfg(feature = "dom")]
    keys: KeyState,
}

impl MatchArmsBuilder {
    pub fn arm(&mut self, pat: &Pat, guard: Option<&Expr>, f: impl FnOnce(&mut ViewWriter)) {
        let mut body = ViewWriter {
            auto_increment: self.auto_increment,
            #[cfg(feature = "dom")]
            keys: self.keys.clone(),
            ..ViewWriter::new()
        };
        f(&mut body);
        body.flush();
        self.auto_increment = body.auto_increment;
        self.arms.push(MatchArm {
            pat: pat.clone(),
            guard: guard.cloned(),
            body: Box::new(body),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rendered(writer: ViewWriter) -> String {
        writer.into_token_stream().to_string()
    }

    #[test]
    fn empty_top_level_writer_emits_view_empty() {
        let out = rendered(ViewWriter::new());
        assert!(out.contains("async"));
        assert!(out.contains(&quote! { #topcoat_view::View::empty }.to_string()));
    }

    #[test]
    fn empty_nested_writer_omits_async_wrapper() {
        // Nested writers (e.g. component children) are spliced into a parent
        // and must not introduce their own async block.
        let out = rendered(ViewWriter::new_nested());
        assert!(!out.contains("async"));
        assert!(out.contains(&quote! { #topcoat_view::View::empty }.to_string()));
    }

    #[test]
    fn adjacent_literal_text_is_concatenated() {
        let mut writer = ViewWriter::new();
        writer.write_str_unescaped("<div>");
        writer.write_text("hello");
        writer.write_str_unescaped("</div>");
        let out = rendered(writer);
        assert!(out.contains("\"<div>hello</div>\""));
    }

    #[test]
    fn literal_text_is_escaped_for_its_position() {
        let mut writer = ViewWriter::new();
        writer.write_str_unescaped("<p>");
        writer.write_text("a < b & \"c\"");
        writer.write_str_unescaped("</p>");
        let out = rendered(writer);
        assert!(out.contains("a &lt; b &amp; \\\"c\\\""));

        let mut writer = ViewWriter::new();
        writer.write_str_unescaped("<p x=\"");
        writer.write_attribute_value("a < b & \"c\"");
        writer.write_str_unescaped("\">");
        let out = rendered(writer);
        assert!(out.contains("a < b &amp; &quot;c&quot;"));
    }

    #[test]
    fn expression_breaks_static_segment_with_kind_helper() {
        let mut writer = ViewWriter::new();
        writer.write_str_unescaped("<p>");
        writer.write_expr(ExprKind::Node, quote! { value });
        writer.write_str_unescaped("</p>");
        let out = rendered(writer);
        assert!(out.contains("__node (__cx , & mut __parts , value)"));

        // Literal markup renders without the request context only where a view
        // can carry hydration sites, since those keep it off the fully static
        // path even when it has no other dynamic content.
        #[cfg(not(feature = "dom"))]
        {
            assert!(out.contains("__unescaped (__cx , & mut __parts , \"<p>\")"));
            assert!(out.contains("__unescaped (__cx , & mut __parts , \"</p>\")"));
        }
        #[cfg(feature = "dom")]
        {
            assert!(out.contains("__static (& mut __parts , \"<p>\")"));
            assert!(out.contains("__static (& mut __parts , \"</p>\")"));
        }
    }

    #[test]
    fn if_else_renders_both_branches() {
        let mut writer = ViewWriter::new();
        writer.if_else(&syn::parse_quote!(cond), |then_branch, else_branch| {
            then_branch.write_str_unescaped("yes");
            else_branch.write_str_unescaped("no");
        });
        let out = rendered(writer);
        assert!(out.contains("if cond"));
        assert!(out.contains("else"));
        assert!(out.contains("\"yes\""));
        assert!(out.contains("\"no\""));
    }

    #[test]
    fn if_without_else_omits_else_branch() {
        let mut writer = ViewWriter::new();
        writer.if_else(&syn::parse_quote!(cond), |then_branch, _| {
            then_branch.write_str_unescaped("yes");
        });
        let out = rendered(writer);
        assert!(out.contains("if cond"));
        assert!(!out.contains("else"));
    }

    #[test]
    fn for_loop_wraps_body_in_for_in_expr() {
        let mut writer = ViewWriter::new();
        writer.for_loop(&syn::parse_quote!(x), &syn::parse_quote!(xs), |body| {
            body.write_str_unescaped("x");
        });
        let out = rendered(writer);
        assert!(out.contains("for x in xs"));
    }

    #[test]
    fn match_expr_renders_arms_with_optional_guard() {
        let mut writer = ViewWriter::new();
        writer.match_expr(&syn::parse_quote!(v), |arms| {
            arms.arm(&syn::parse_quote!(A), None, |body| {
                body.write_str_unescaped("a");
            });
            arms.arm(
                &syn::parse_quote!(B),
                Some(&syn::parse_quote!(flag)),
                |body| {
                    body.write_str_unescaped("b");
                },
            );
        });
        let out = rendered(writer);
        assert!(out.contains("match v"));
        assert!(out.contains("A =>"));
        assert!(out.contains("B if flag =>"));
    }

    #[test]
    fn local_binding_emits_let_statement() {
        let mut writer = ViewWriter::new();
        writer.local_binding(&syn::parse_quote!(x), &syn::parse_quote!(value));
        writer.write_str_unescaped("ok");
        let out = rendered(writer);
        assert!(out.contains("let x = value"));
    }

    #[test]
    fn expr_kind_selects_matching_helper() {
        for (kind, expected) in [
            (ExprKind::Unescaped, "__unescaped"),
            (ExprKind::Node, "__node"),
            (ExprKind::View, "__view"),
            (ExprKind::ElementName, "__element_name"),
            (ExprKind::Attribute, "__attribute"),
            (ExprKind::AttributeUnescaped, "__attribute_unescaped"),
            (ExprKind::AttributeKey, "__attribute_key"),
            (ExprKind::AttributeValue, "__attribute_value"),
            (ExprKind::Attributes, "__attributes"),
        ] {
            let mut writer = ViewWriter::new();
            writer.write_expr(kind, quote! { v });
            assert!(
                rendered(writer).contains(expected),
                "expected helper `{expected}` for kind `{expected}`",
            );
        }
    }
}
