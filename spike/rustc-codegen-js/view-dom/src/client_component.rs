//! The `#[client(client)]` construct: one component, rendered by the server
//! and rendered again by the client.
//!
//! A client component is written once and compiled twice, the way an island is.
//! The server compiles it to the ordinary `#[component]` whose view is wrapped
//! as a component's own, so the hydration keys it writes nest under the ordinal
//! the call took. The client compiles the same view body to a template the DOM
//! emitter lowers, exported as a plain function a client view calls.
//!
//! Which half a crate gets is decided by the `topcoat_client` cfg, and that is
//! the whole of the enforcement: a component that is only declared for the
//! server does not exist under `topcoat_client`, so calling one from a client
//! view fails to resolve and names the component that is missing.
//!
//! # Props
//!
//! Props are eager. A prop is an ordinary Rust value evaluated once where the
//! component is called, so a prop that has to stay reactive is passed as
//! something that carries its own reactivity, such as a signal or a closure. The
//! reference compiler builds props as getters instead, which buys laziness that
//! Rust has no implicit form of: a lazy prop would be a closure either way.
//!
//! Both halves declare their props in the same struct, named after the component
//! the way the server's own props struct is, so exactly one definition of that
//! name exists per target. A call site names the fields rather than their order,
//! which is what lets the emitter build props without knowing how the component
//! declared them.
//!
//! # Child content
//!
//! A component takes child content by declaring a `child` parameter, as the
//! server's own components do. It is a prop like any other, which means it is
//! built where the component is called rather than where the component renders
//! it, and that is what decides its numbering: the content takes the caller's
//! ordinals, and the component's own call takes the next one.
//!
//! The two halves take it as the type each of them has. The server takes a view
//! and renders it into a buffer at the call site before entering the component,
//! handing the component's own view a stand-in that writes the buffer wherever it
//! renders the content. The client takes nodes that are already built. Both sides
//! therefore number the content before the component and neither numbers it
//! inside, which is what the reference runtime's own child content does under an
//! eager argument.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Attribute, Ident, ItemFn, Type, Visibility};
use topcoat_view_grammar::component::props_ident;

use crate::common::{half, view_body, Prop, PropsStruct, CHILD_PROP};

/// A parsed `#[component(client)] async fn ...`.
pub struct ClientComponent {
    attrs: Vec<Attribute>,
    vis: Visibility,
    ident: Ident,
    inputs: Vec<Prop>,
    output: TokenStream,
    body: TokenStream,
}

impl ClientComponent {
    /// Parses a `#[component(client)]` attribute and the function it is written
    /// on.
    ///
    /// # Errors
    ///
    /// Returns an error if the attribute is anything but `client`, if the
    /// function takes a receiver, a pattern parameter or a request context, or
    /// if its body is anything but a single `view!` invocation.
    pub fn parse(attr: TokenStream, item: TokenStream) -> syn::Result<Self> {
        if attr.to_string().trim() != "client" {
            return Err(syn::Error::new_spanned(
                attr,
                "only `#[component(client)]` is compiled for the client; a component without it is \
                 rendered by the server alone",
            ));
        }

        let item: ItemFn = syn::parse2(item)?;
        let inputs = item
            .sig
            .inputs
            .iter()
            .map(Prop::parse)
            .collect::<syn::Result<Vec<_>>>()?;
        for input in &inputs {
            if input.ident == "cx" {
                return Err(syn::Error::new(
                    input.ident.span(),
                    "a `cx` parameter names a request context, which the client half of a \
                     component has none of",
                ));
            }
        }
        let output = match &item.sig.output {
            syn::ReturnType::Default => {
                return Err(syn::Error::new(
                    item.sig.paren_token.span.join(),
                    "a component renders a view, so it returns one: write `-> Result`",
                ));
            }
            syn::ReturnType::Type(arrow, ty) => quote! { #arrow #ty },
        };

        Ok(Self {
            attrs: item.attrs,
            vis: item.vis,
            ident: item.sig.ident,
            inputs,
            output,
            body: view_body(&item.block, "a component")?,
        })
    }

    /// The struct both halves declare the component's props in.
    #[must_use]
    pub fn props(&self) -> Ident {
        props_ident(&self.ident)
    }

    /// Both halves of the component, each item behind the cfg that selects it.
    #[must_use]
    pub fn expand(&self) -> TokenStream {
        let server = half(false, [self.server()]);
        let client = half(true, self.client());
        quote! {
            #server
            #client
        }
    }

    /// Whether the component declared child content.
    fn takes_child(&self) -> bool {
        self.inputs.iter().any(|input| input.ident == CHILD_PROP)
    }

    /// The server half: the ordinary component, rendering its view as a
    /// component's own so its keys nest.
    fn server(&self) -> TokenStream {
        let Self {
            attrs,
            vis,
            ident,
            inputs,
            output,
            body,
        } = self;

        let parameters = inputs.iter().map(Prop::parameter);
        // Child content is an argument, so it is numbered where the call is and
        // not where this view renders it. The view is handed a stand-in and the
        // content itself travels beside it, which is what puts the two sides in
        // the same order.
        let child = Ident::new(CHILD_PROP, Span::call_site());
        let (aside, wrap) = if self.takes_child() {
            (
                quote! {
                    let __topcoat_child = #child;
                    let #child = ::topcoat::view::View::child_content();
                },
                quote! {
                    ::topcoat::view::View::component_with_child(__topcoat_body, __topcoat_child)
                },
            )
        } else {
            (
                TokenStream::new(),
                quote! { ::topcoat::view::View::component(__topcoat_body) },
            )
        };

        quote! {
            #(#attrs)*
            #[::topcoat::view::component]
            #vis async fn #ident(#(#parameters),*) #output {
                #aside
                // The view numbers its keys from zero, so it is wrapped as a
                // component's own: the keys it writes nest under the ordinal the
                // call took, which is the context the client opens for it.
                let __topcoat_body = ::topcoat::view::view! { #body }?;
                ::topcoat::Result::Ok(#wrap)
            }
        }
    }

    /// The client half: the props struct and the function a client view calls,
    /// whose body is the same view lowered to a hydratable template.
    fn client(&self) -> Vec<TokenStream> {
        let Self {
            vis, ident, body, ..
        } = self;

        let props = self.props();
        let PropsStruct {
            declaration,
            arguments,
            parameters,
        } = PropsStruct::new(vis, &props, &self.inputs, Self::client_ty);
        let names = self.inputs.iter().map(|input| &input.ident);
        // A component's client half is dead whenever its callers are all on the
        // server, and where a call happens is not this half's business. The
        // braces allowance matches the island entry point's.
        vec![
            quote! {
                #[allow(dead_code)]
                #declaration
            },
            quote! {
                #[allow(dead_code)]
                #[allow(unused_braces)]
                #vis fn #ident #parameters (props: #props #arguments) -> ::view_abi::Node {
                    let #props { #(#names,)* } = props;
                    ::view_dom_macro::dom_view! { #body }
                }
            },
        ]
    }

    /// The type the client half takes a prop as.
    ///
    /// Child content is the one prop whose type differs between the halves: the
    /// server renders a view, and the client is handed nodes that are already
    /// built.
    fn client_ty(prop: &Prop) -> Type {
        if prop.ident == CHILD_PROP {
            return syn::parse_quote!(::view_abi::Node);
        }
        prop.ty.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(item: TokenStream) -> ClientComponent {
        match ClientComponent::parse(quote! { client }, item) {
            Ok(component) => component,
            Err(error) => panic!("expected the component to parse: {error}"),
        }
    }

    /// The client half's items, as one string: they are separate items so that
    /// each can carry the cfg that selects it.
    fn client(item: TokenStream) -> String {
        component(item)
            .client()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn error(item: TokenStream) -> String {
        match ClientComponent::parse(quote! { client }, item) {
            Ok(_) => panic!("expected the component to be rejected"),
            Err(error) => error.to_string(),
        }
    }

    fn badge() -> TokenStream {
        quote! {
            async fn badge(title: &str, count: i32) -> Result {
                view! { <p>(title)</p> }
            }
        }
    }

    #[test]
    fn the_props_struct_is_the_one_the_server_declares() {
        assert_eq!(component(badge()).props().to_string(), "BadgeProps");
        let renamed = component(quote! {
            async fn user_card() -> Result {
                view! { <p>"x"</p> }
            }
        });
        assert_eq!(renamed.props().to_string(), "UserCardProps");
    }

    #[test]
    fn both_halves_are_emitted_behind_the_cfg_that_selects_them() {
        let expanded = component(badge()).expand().to_string();
        assert!(
            expanded.contains("cfg (not (topcoat_client))"),
            "{expanded}"
        );
        assert!(expanded.contains("cfg (topcoat_client)"), "{expanded}");
    }

    #[test]
    fn every_item_of_a_half_carries_the_cfg_that_selects_it() {
        // A cfg applies to the ONE item that follows it, and the client half is
        // the props struct and the function, so one attribute in front of the two
        // would compile the function into a server build as well, where the
        // client ABI it names does not exist.
        let component = component(badge());
        let expanded = component.expand().to_string();
        assert_eq!(
            expanded.matches("# [cfg (topcoat_client)]").count(),
            component.client().len(),
            "{expanded}",
        );
        assert_eq!(
            expanded.matches("# [cfg (not (topcoat_client))]").count(),
            1,
            "{expanded}",
        );
        for item in expanded.split("# [cfg ").skip(1) {
            assert!(
                !item.contains("view_abi") || item.starts_with("(topcoat_client)]"),
                "an item the server compiles names the client ABI: {item}",
            );
        }
    }

    #[test]
    fn the_server_half_wraps_its_view_as_a_components_own() {
        let server = component(badge()).server().to_string();
        assert!(
            server.contains(":: topcoat :: view :: component]"),
            "{server}"
        );
        assert!(
            server.contains("View :: component (__topcoat_body)"),
            "{server}"
        );
        // The declared parameters are the server component's, unchanged.
        assert!(
            server.contains("async fn badge (title : & str , count : i32)"),
            "{server}",
        );
    }

    #[test]
    fn the_client_half_takes_its_props_as_one_struct() {
        let client = client(badge());
        assert!(
            client.contains(
                "struct BadgeProps < '__tc_props > { title : & '__tc_props str , count : i32 , }"
            ),
            "{client}",
        );
        assert!(
            client.contains("fn badge (props : BadgeProps < '_ >) -> :: view_abi :: Node"),
            "{client}",
        );
        assert!(
            client.contains("let BadgeProps { title , count , } = props ;"),
            "{client}",
        );
        assert!(client.contains("dom_view !"), "{client}");
    }

    #[test]
    fn props_that_do_not_borrow_need_no_lifetime() {
        let client = client(quote! {
            async fn plain(count: i32) -> Result {
                view! { <p>(count)</p> }
            }
        });
        assert!(
            client.contains("struct PlainProps { count : i32 , }"),
            "{client}"
        );
        assert!(
            client.contains("fn plain (props : PlainProps) -> :: view_abi :: Node"),
            "{client}",
        );
    }

    #[test]
    fn an_impl_trait_prop_is_desugared_onto_both_items() {
        // `impl Trait` in a parameter is sugar for a generic parameter, and a
        // struct field cannot hold the sugar, so the desugaring is written out.
        // A call site is unaffected: inference fills the parameter in.
        let client = client(quote! {
            async fn readout(read: impl Fn() -> f64) -> Result {
                view! { <p>(read())</p> }
            }
        });
        assert!(
            client
                .contains("struct ReadoutProps < __TcRead : Fn () -> f64 > { read : __TcRead , }"),
            "{client}",
        );
        assert!(
            client.contains(
                "fn readout < __TcRead : Fn () -> f64 > (props : ReadoutProps < __TcRead >) \
                 -> :: view_abi :: Node"
            ),
            "{client}",
        );
    }

    #[test]
    fn a_desugared_prop_and_a_borrowing_one_share_one_parameter_list() {
        let client = client(quote! {
            async fn readout(label: &str, read: impl Fn() -> f64) -> Result {
                view! { <p>(label)(read())</p> }
            }
        });
        // The lifetime comes first and the function leaves it elided, so the two
        // lists cannot nest inside one another.
        assert!(
            client.contains(
                "struct ReadoutProps < '__tc_props , __TcRead : Fn () -> f64 > \
                 { label : & '__tc_props str , read : __TcRead , }"
            ),
            "{client}",
        );
        assert!(
            client.contains(
                "fn readout < __TcRead : Fn () -> f64 > (props : ReadoutProps < '_ , __TcRead >)"
            ),
            "{client}",
        );
    }

    #[test]
    fn every_impl_trait_prop_gets_a_parameter_of_its_own() {
        let client = client(quote! {
            async fn pair(read: impl Fn() -> f64, write: impl Fn(f64)) -> Result {
                view! { <p>(read())</p> }
            }
        });
        assert!(
            client.contains(
                "struct PairProps < __TcRead : Fn () -> f64 , __TcWrite : Fn (f64) > \
                 { read : __TcRead , write : __TcWrite , }"
            ),
            "{client}",
        );
    }

    #[test]
    fn the_server_half_keeps_the_sugar_it_was_written_with() {
        // Only the struct cannot hold `impl Trait`; the server's own component is
        // an ordinary function and takes the parameter as declared.
        let server = component(quote! {
            async fn readout(read: impl Fn() -> f64) -> Result {
                view! { <p>(read())</p> }
            }
        })
        .server()
        .to_string();
        assert!(
            server.contains("async fn readout (read : impl Fn () -> f64)"),
            "{server}",
        );
    }

    #[test]
    fn an_elided_lifetime_inside_a_prop_type_is_written_out() {
        let client = client(quote! {
            async fn listing(rows: ::std::slice::Iter<'_, i32>) -> Result {
                view! { <p>"x"</p> }
            }
        });
        assert!(client.contains("< '__tc_props >"), "{client}");
        assert!(client.contains("Iter < '__tc_props , i32 >"), "{client}");
    }

    fn card() -> TokenStream {
        quote! {
            async fn card(title: &str, child: View) -> Result {
                view! { <div>(title)(child)</div> }
            }
        }
    }

    #[test]
    fn the_server_half_renders_child_content_before_it_enters_the_client() {
        let server = component(card()).server().to_string();
        // The content is kept aside and the view is handed a stand-in, so the
        // content is numbered where the call is.
        assert!(server.contains("let __topcoat_child = child ;"), "{server}");
        assert!(
            server.contains("let child = :: topcoat :: view :: View :: child_content () ;"),
            "{server}",
        );
        assert!(
            server.contains("View :: component_with_child (__topcoat_body , __topcoat_child)"),
            "{server}",
        );
        // The declared parameter is still the component's own, so a call site
        // passes child nodes exactly as it does to a server component.
        assert!(
            server.contains("async fn card (title : & str , child : View)"),
            "{server}",
        );
    }

    #[test]
    fn a_component_without_child_content_is_wrapped_as_before() {
        let server = component(badge()).server().to_string();
        assert!(
            server.contains("View :: component (__topcoat_body)"),
            "{server}"
        );
        assert!(!server.contains("__topcoat_child"), "{server}");
    }

    #[test]
    fn the_client_half_takes_child_content_as_nodes() {
        let client = client(card());
        // The one prop whose type differs between the halves: the client is
        // handed content that is already built.
        assert!(
            client.contains(
                "struct CardProps < '__tc_props > { title : & '__tc_props str , child : :: view_abi :: Node , }"
            ),
            "{client}",
        );
        assert!(
            client.contains("let CardProps { title , child , } = props ;"),
            "{client}",
        );
    }

    #[test]
    fn a_component_without_the_client_argument_is_server_only() {
        let message = ClientComponent::parse(TokenStream::new(), badge())
            .err()
            .expect("an empty attribute is rejected")
            .to_string();
        assert!(
            message.contains("rendered by the server alone"),
            "{message}"
        );
    }

    #[test]
    fn a_request_context_parameter_is_rejected() {
        let message = error(quote! {
            async fn badge(cx: &Cx) -> Result {
                view! { <p>"x"</p> }
            }
        });
        assert!(message.contains("request context"), "{message}");
    }

    #[test]
    fn a_pattern_parameter_is_rejected() {
        let message = error(quote! {
            async fn badge((a, b): (i32, i32)) -> Result {
                view! { <p>(a)</p> }
            }
        });
        assert!(message.contains("plain identifier"), "{message}");
    }

    #[test]
    fn a_body_that_is_not_a_view_is_rejected() {
        let message = error(quote! {
            async fn badge() -> Result {
                let x = 1;
                view! { <p>(x)</p> }
            }
        });
        assert!(message.contains("single `view!` invocation"), "{message}");
    }

    #[test]
    fn a_component_that_renders_nothing_is_rejected() {
        let message = error(quote! {
            async fn badge() {
                view! { <p>"x"</p> }
            }
        });
        assert!(message.contains("returns one"), "{message}");
    }
}
