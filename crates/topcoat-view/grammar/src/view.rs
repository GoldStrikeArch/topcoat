mod component;
mod document_type;
mod element;
mod element_name;
mod element_tag;
mod html_ident;
#[cfg(feature = "dom")]
mod key_plan;
mod node;
mod nodes;
mod signal_declaration;
mod view_writer;

pub use component::*;
pub use document_type::*;
pub use element::*;
pub use element_name::*;
pub use element_tag::*;
pub use html_ident::*;
#[cfg(feature = "dom")]
pub use key_plan::*;
pub use node::*;
pub use nodes::*;
pub use signal_declaration::*;
pub(crate) use view_writer::*;

use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::parse::{Parse, ParseStream};

use topcoat_core_grammar::ParseOption;

use crate::leading_cx::LeadingCx;

/// The parsed body of a `view!` invocation. Lowers to a
/// [`runtime::View`](topcoat_view::View).
pub struct View {
    /// The request context binding supplied by a leading `cx =>` argument.
    ///
    /// Inside a `#[component]`, `#[page]`, `#[layout]`, or `#[shard]`, the
    /// context is available implicitly, so this is [`None`]. Anywhere else
    /// (for example a `#[route]` handler), the caller names it explicitly as
    /// `view! { cx => ... }` and the rest of the view renders against it.
    pub cx: Option<LeadingCx>,
    pub nodes: Nodes,
}

impl Parse for View {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(Self {
            cx: input.call(LeadingCx::parse_option)?,
            nodes: input.parse()?,
        })
    }
}

impl ToTokens for View {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        #[cfg(not(feature = "dom"))]
        let mut writer = ViewWriter::new();

        // The plan numbers every site in the view once; the writer reads it
        // through a cursor rather than counting for itself, so the keys it
        // writes are the same ones the DOM emitter uses for the same sites.
        #[cfg(feature = "dom")]
        let cursor = std::rc::Rc::new(KeyPlan::build(&self.nodes)).shared_cursor();
        #[cfg(feature = "dom")]
        let mut writer = ViewWriter::with_key_cursor(cursor.clone());

        writer.write_children(&self.nodes);

        #[cfg(feature = "dom")]
        debug_assert!(
            !cursor.desynced(),
            "the view writer and the key plan disagree about the shape of this view",
        );

        let view = writer.into_token_stream();

        // When an explicit context is named, bind it to the `__cx` identifier
        // the generated code (component invocations) reads from. Inside a
        // component/page/layout this binding is already in scope, so we emit
        // the view untouched.
        match &self.cx {
            Some(cx) => quote! {
                {
                    #cx
                    #view
                }
            }
            .to_tokens(tokens),
            None => view.to_tokens(tokens),
        }
    }
}

#[cfg(feature = "pretty")]
impl topcoat_core_grammar::pretty::PrettyPrint for View {
    fn pretty_print(&self, printer: &mut topcoat_core_grammar::pretty::Printer<'_>) {
        if let Some(cx) = &self.cx {
            cx.pretty_print(printer);
        }
        self.nodes.pretty_print(printer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::Node;

    fn parse(source: &str) -> View {
        syn::parse_str(source).unwrap()
    }

    #[test]
    fn empty_input_yields_no_nodes() {
        assert!(parse("").nodes.is_empty());
    }

    #[test]
    fn collects_sibling_nodes_in_order() {
        let view = parse(r#""a" "b" "c""#);
        assert_eq!(view.nodes.len(), 3);
        assert!(view.nodes.iter().all(|n| matches!(n, Node::Text(_))));
    }

    #[test]
    fn parses_leading_cx_argument() {
        let view = parse("cx => <div></div>");
        assert_eq!(view.cx.map(|cx| cx.cx.to_string()), Some("cx".to_owned()));
        assert_eq!(view.nodes.len(), 1);
    }

    #[test]
    fn omitted_cx_is_none() {
        assert!(parse("<div></div>").cx.is_none());
    }

    #[test]
    fn component_invocation_is_not_mistaken_for_cx() {
        // A component invocation also starts with an identifier, but it is
        // followed by `(`, not `=>`, so it stays a node.
        let view = parse(r#"greeting(name: "World")"#);
        assert!(view.cx.is_none());
        assert_eq!(view.nodes.len(), 1);
    }

    #[test]
    fn leading_text_nodes_are_not_mistaken_for_cx() {
        // A leading string literal is not an identifier, so it is never consumed
        // as a `cx` argument.
        let view = parse(r#""a" "b""#);
        assert!(view.cx.is_none());
        assert_eq!(view.nodes.len(), 2);
    }

    #[test]
    fn explicit_cx_binds_the_context_identifier() {
        let tokens = parse("cx => <div></div>").to_token_stream().to_string();
        assert!(tokens.contains("let __cx"), "{tokens}");
    }

    #[test]
    fn omitted_cx_emits_no_binding() {
        let tokens = parse("<div></div>").to_token_stream().to_string();
        assert!(!tokens.contains("let __cx"), "{tokens}");
    }

    #[test]
    fn a_loops_key_clause_does_not_change_what_the_server_renders() {
        // A server render walks the iterable once in order, so it has no rendered
        // rows to match against and the key is neither read nor evaluated. The
        // clause is there for a client emitter alone.
        let plain = parse(r"<ul>for item in items { <li>(item)</li> }</ul>")
            .to_token_stream()
            .to_string();
        let keyed = parse(r"<ul>for item in items key (item.id) { <li>(item)</li> }</ul>")
            .to_token_stream()
            .to_string();
        assert_eq!(plain, keyed);
        assert!(!keyed.contains("id"), "{keyed}");
    }

    #[cfg(not(feature = "dom"))]
    #[test]
    fn no_hydration_sites_are_written_without_the_dom_feature() {
        let tokens = parse(r#"<div>"a" (value)</div>"#)
            .to_token_stream()
            .to_string();
        assert!(!tokens.contains("push_hydration_site"), "{tokens}");
    }

    #[cfg(feature = "dom")]
    mod hydration {
        use super::*;

        fn tokens(source: &str) -> String {
            parse(source).to_token_stream().to_string()
        }

        /// The hydration sites the writer emits, in the order it emits them.
        ///
        /// A site carries no ordinal: a template root is numbered as it renders,
        /// because only a render knows which branches it took. What the
        /// expansion pins is which positions carry a site and in what order.
        fn sites(source: &str) -> Vec<String> {
            tokens(source)
                .split("HydrationSite :: ")
                .skip(1)
                .map(|rest| {
                    rest.split(|c: char| !c.is_ascii_alphanumeric())
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect()
        }

        #[test]
        fn only_the_outermost_element_of_a_template_is_keyed() {
            assert_eq!(sites("<div><span><b></b></span></div>"), ["TemplateRoot"]);
        }

        #[test]
        fn every_sibling_template_is_keyed() {
            assert_eq!(
                sites("<p></p><br><img/>"),
                ["TemplateRoot", "TemplateRoot", "TemplateRoot"],
            );
        }

        #[test]
        fn the_key_is_written_before_the_attributes() {
            // The client reads `data-hk` off the element it already found, so
            // the position only has to match the DOM emitter, which puts it
            // first, right after the tag name.
            let tokens = tokens(r#"<div class="a"></div>"#);
            let key = tokens.find("push_hydration_site").unwrap();
            let class = tokens.find("class").unwrap();
            assert!(key < class, "{tokens}");
        }

        #[test]
        fn a_lone_dynamic_child_is_not_marked() {
            assert_eq!(sites("<p>(value)</p>"), ["TemplateRoot"]);
        }

        #[test]
        fn a_dynamic_child_with_siblings_is_marked() {
            assert_eq!(
                sites(r#"<p>"a" (value)</p>"#),
                ["TemplateRoot", "ChildStart", "ChildEnd"],
            );
        }

        #[test]
        fn every_dynamic_child_gets_its_own_marker_pair() {
            // Markers delimit the range one expression owns, so adjacent
            // expressions never share a pair the way a bare anchor is shared.
            let tokens = tokens("<p>(a) (b)</p>");
            assert_eq!(tokens.matches("ChildStart").count(), 2, "{tokens}");
            assert_eq!(tokens.matches("ChildEnd").count(), 2, "{tokens}");
        }

        #[test]
        fn static_children_are_never_marked() {
            assert_eq!(
                sites(r#"<div>"a" <span></span> "b"</div>"#),
                ["TemplateRoot"],
            );
        }

        #[test]
        fn declarations_do_not_count_towards_the_child_count() {
            // The only child that renders is the expression, so it is alone and
            // needs no markers.
            assert_eq!(sites("<p>signal count = 0; (count)</p>"), ["TemplateRoot"]);
        }

        #[test]
        fn control_flow_is_a_dynamic_child() {
            assert_eq!(
                sites(r#"<div>"a" if flag { "b" }</div>"#),
                ["TemplateRoot", "ChildStart", "ChildEnd"],
            );
        }

        #[test]
        fn elements_inside_control_flow_start_templates_of_their_own() {
            // A branch is built as a unit and thrown away with the test that
            // selects it, so the DOM emitter gives it a template of its own and
            // the client looks that template's root up by key. The server has to
            // have written one there.
            assert_eq!(
                sites("<div>if flag { <span></span> } else { <b></b> }</div>"),
                ["TemplateRoot", "TemplateRoot", "TemplateRoot"],
            );
        }

        #[test]
        fn every_arm_of_a_match_is_a_template() {
            assert_eq!(
                sites(r#"<div>match kind { 0 => <b></b>, _ => "x", }</div>"#),
                ["TemplateRoot", "TemplateRoot"],
            );
        }

        #[test]
        fn a_loop_body_is_a_template() {
            assert_eq!(
                sites("<ul>for item in items { <li>(item)</li> }</ul>"),
                ["TemplateRoot", "TemplateRoot"],
            );
        }

        #[test]
        fn an_else_if_chain_adds_no_template_of_its_own() {
            // The chained `if` opens its own branches, so the elements are one
            // template each and the chain itself is none.
            assert_eq!(
                sites(r#"if a { <p></p> } else if b { <b></b> } else { "x" }"#),
                ["TemplateRoot", "TemplateRoot"],
            );
        }

        #[test]
        fn a_branch_at_the_top_level_still_starts_templates() {
            // Nothing encloses the branch either way, so this reads the same
            // before and after a branch became a template of its own.
            assert_eq!(
                sites("if flag { <p></p> } else { <b></b><i></i> }"),
                ["TemplateRoot", "TemplateRoot", "TemplateRoot"],
            );
        }

        #[test]
        fn component_children_start_templates_of_their_own() {
            assert_eq!(
                sites(r#"<div>card(title: "t", <p></p>)</div>"#),
                ["TemplateRoot", "TemplateRoot"],
            );
        }

        /// A view exercising every construct that partitions templates.
        const CONTROL_FLOW: &str = r#"
            <div>
                for item in items { <li>(item)</li> }
                if flag { <span><b></b></span> } else if other { <i></i> } else { "no" }
                match kind { 0 => <p></p>, _ => <hr>, }
            </div>
        "#;

        #[test]
        fn the_writer_writes_a_key_for_every_root_the_plan_numbers() {
            // The plan is what the DOM emitter numbers its templates against, so
            // a root the plan knows about and the server never writes a key for
            // is a node the client looks up and does not find. Counting both
            // sides of the same fixture is the check.
            let planned = KeyPlan::build(&parse(CONTROL_FLOW).nodes)
                .keys()
                .iter()
                .filter(|key| key.site() == KeySite::TemplateRoot)
                .count();
            let written = sites(CONTROL_FLOW)
                .iter()
                .filter(|site| *site == "TemplateRoot")
                .count();

            // `<div>`, the loop's row, the two branches with elements, and the
            // two arms.
            assert_eq!(planned, 6);
            assert_eq!(written, planned);
        }

        #[test]
        fn the_writer_consumes_the_whole_plan() {
            // Every site the plan numbers has to be taken in the plan's order,
            // or the ordinals drift apart from the DOM emitter's. The writer
            // asserts this itself, so reaching the end here is the check.
            let source = r#"
                signal count = 0;
                <(tag) class="x" if flag { id="a" } for k in ks { (k)="v" }>
                    "text"
                    for item in items { <li>(item)</li> }
                    if flag { <(inner)></(inner)> } else { "no" }
                    match kind { _ => <b></b>, }
                    $(count)
                    card(title: "t", <p></p>)
                </(tag)>
                <hr>
            "#;
            let tokens = tokens(source);
            assert!(tokens.contains("push_hydration_site"), "{tokens}");
        }
    }
}
