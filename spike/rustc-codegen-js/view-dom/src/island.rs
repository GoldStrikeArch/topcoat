//! The `#[island]` construct: one view, rendered by the server and taken over
//! by the client.
//!
//! An island is written once and compiled twice. The server compiles it to a
//! `#[component]` that allocates an island instance, renders the view against
//! it so every node the client has to find again carries a hydration key, and
//! wraps the result in the `<topcoat-island>` element the loader looks for. The
//! client compiles the same view body to a template the DOM emitter lowers, and
//! exports it under a name the loader can call.
//!
//! Which half a crate gets is decided by the `topcoat_client` cfg, so both are
//! emitted and the compiler drops the one that does not apply. The two halves
//! never see each other's dependencies: the server half names `topcoat` and the
//! client half names `view_abi` and `view_dom_macro`.
//!
//! # What the client is handed
//!
//! The `<topcoat-island>` element carries everything the loader needs and
//! nothing else:
//!
//! - `data-ti`, the island's name, which is also the exported entry point's name with `__island_`
//!   in front of it.
//! - `data-tk`, the island instance, whose key prefix scopes the hydration keys inside the element.
//! - `data-ts`, the island's arguments as a JSON array, in the order they are declared. The loader
//!   parses them and passes them straight to the entry point, which is what the client body seeds
//!   its signals from.
//!
//! Seeding through the arguments works because a signal in an island is
//! declared with an expression over them, so passing the same arguments to the
//! client body produces the same initial values. A signal the server has since
//! written to needs its current value instead, which is a value per signal
//! ordinal rather than a value per argument.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Attribute, Ident, ItemFn, Visibility};
use topcoat_view_grammar::component::props_ident;

use crate::common::{half, view_body, Prop, PropsStruct};

/// A parsed `#[island] async fn ...`.
pub struct Island {
    attrs: Vec<Attribute>,
    vis: Visibility,
    ident: Ident,
    inputs: Vec<Prop>,
    output: TokenStream,
    body: TokenStream,
}

impl Island {
    /// Parses an `#[island]` attribute and the function it is written on.
    ///
    /// # Errors
    ///
    /// Returns an error if the attribute takes arguments, if the function
    /// takes a receiver or a pattern parameter, or if its body is anything but
    /// a single `view!` invocation.
    pub fn parse(attr: TokenStream, item: TokenStream) -> syn::Result<Self> {
        if !attr.is_empty() {
            return Err(syn::Error::new_spanned(
                attr,
                "`#[island]` takes no arguments",
            ));
        }

        let item: ItemFn = syn::parse2(item)?;
        let inputs = item
            .sig
            .inputs
            .iter()
            .map(Prop::parse)
            .collect::<syn::Result<Vec<_>>>()?;
        let output = match &item.sig.output {
            syn::ReturnType::Default => {
                return Err(syn::Error::new(
                    item.sig.paren_token.span.join(),
                    "an island renders a view, so it returns one: write `-> Result`",
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
            body: view_body(&item.block, "an island")?,
        })
    }

    /// The island's name, which the `data-ti` attribute carries.
    #[must_use]
    pub fn name(&self) -> String {
        self.ident.to_string()
    }

    /// The name the client half exports the island's entry point under.
    #[must_use]
    pub fn entry_point(&self) -> Ident {
        format_ident!("__island_{}", self.ident)
    }

    /// Both halves of the island, each item behind the cfg that selects it.
    #[must_use]
    pub fn expand(&self) -> TokenStream {
        let server = half(false, [self.server()]);
        let client = half(true, self.client());
        quote! {
            #server
            #client
        }
    }

    /// The server half: a component that renders the view as an island
    /// instance and wraps it in the element the loader finds it by.
    fn server(&self) -> TokenStream {
        let Self {
            attrs,
            vis,
            ident,
            inputs,
            output,
            body,
        } = self;

        let name = self.name();
        let parameters = inputs.iter().map(Prop::parameter);
        let seeds = seeds(inputs);

        quote! {
            #(#attrs)*
            #[::topcoat::view::component]
            #vis async fn #ident(#(#parameters),*) #output {
                // The instance is allocated before the body is built and stays
                // current for the whole of it: everything the body numbers
                // against the island reads the guard while it is held.
                let __topcoat_island = __cx.islands().next_instance();
                let __topcoat_guard = __cx.islands().enter(__topcoat_island);
                let __topcoat_body = ::topcoat::view::view! { #body };
                ::core::mem::drop(__topcoat_guard);
                let __topcoat_body = __topcoat_body?;

                // The wrapper is built outside the island, so its own sites
                // write no keys and the element the client hydrates into is not
                // one of the nodes it has to claim.
                ::topcoat::Result::Ok(__topcoat_body.island_element(
                    #name,
                    __topcoat_island,
                    #seeds,
                ))
            }
        }
    }

    /// The client half: the entry point the loader calls, whose body is the
    /// same view lowered to a hydratable template.
    fn client(&self) -> Vec<TokenStream> {
        let Self { inputs, body, .. } = self;

        let entry_point = self.entry_point();
        let parameters = inputs.iter().map(Prop::parameter);
        // The lowering braces every hole expression, and for a bare identifier
        // the braces are redundant. A warning inside generated code is one no
        // author can act on.
        let mut items = vec![quote! {
            #[unsafe(no_mangle)]
            #[allow(unused_braces)]
            pub fn #entry_point(#(#parameters),*) -> ::view_abi::Node {
                ::view_dom_macro::dom_view! { #body }
            }
        }];
        items.extend(self.nested());
        items
    }

    /// What a client view calling this island resolves to, which is the error
    /// saying it cannot.
    ///
    /// An island is an entry point: the loader calls it with the arguments the
    /// server rendered into the wrapper element, and it takes over nodes that
    /// already carry its own key prefix. A view inside another island has no
    /// wrapper to take over, so nesting one is a constraint rather than a feature
    /// that is missing, and the constraint is said here, where the call is: the
    /// island's name resolves client-side to a function whose parameter names it.
    ///
    /// The props struct is declared alongside, because a call site builds one and
    /// an unresolved struct would be the only error the call reported.
    fn nested(&self) -> Vec<TokenStream> {
        let Self { vis, ident, .. } = self;

        let props = props_ident(ident);
        let PropsStruct { declaration, .. } =
            PropsStruct::new(vis, &props, &self.inputs, |prop| prop.ty.clone());
        let constraint = format_ident!("__{}_island", ident);
        // All three items exist to be named in an error, never to be used, so
        // a correct program leaves each of them dead.
        vec![
            quote! {
                #[doc(hidden)]
                #[allow(dead_code)]
                #declaration
            },
            quote! {
                #[doc(hidden)]
                #vis mod #constraint {
                    #[allow(non_camel_case_types)]
                    #[allow(dead_code)]
                    pub struct AnIslandCannotBeNestedInsideAnotherIsland;
                }
            },
            quote! {
                #[doc(hidden)]
                #[allow(dead_code)]
                #vis fn #ident(
                    _: #constraint::AnIslandCannotBeNestedInsideAnotherIsland,
                ) -> ::view_abi::Node {
                    // A diverging loop rather than `unreachable!`, because a
                    // client crate is expanded to a file that is then compiled
                    // again: a panicking macro has already become
                    // `core::panicking::panic_fmt` by then, which the second
                    // compilation rejects as an unstable internal. Nothing can
                    // call this function anyway, since its parameter is the
                    // constraint its name states.
                    loop {}
                }
            },
        ]
    }
}

/// The expression that renders an island's arguments as the JSON array the
/// client is seeded from.
///
/// An island with no arguments needs no serializer at all, which is what keeps
/// a crate that has no use for one from having to depend on it.
fn seeds(inputs: &[Prop]) -> TokenStream {
    if inputs.is_empty() {
        return quote! { "[]" };
    }
    let idents = inputs.iter().map(|input| &input.ident);
    quote! {
        // A tuple serializes as an array, which is the shape the loader spreads
        // over the entry point's parameters.
        ::serde_json::to_string(&(#(&#idents,)*))
            .unwrap_or_else(|_| ::std::string::String::from("[]"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn island(item: TokenStream) -> Island {
        match Island::parse(TokenStream::new(), item) {
            Ok(island) => island,
            Err(error) => panic!("expected the island to parse: {error}"),
        }
    }

    /// The client half's items, as one string: they are separate items so that
    /// each can carry the cfg that selects it.
    fn client(item: TokenStream) -> String {
        island(item)
            .client()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn error(item: TokenStream) -> String {
        match Island::parse(TokenStream::new(), item) {
            Ok(_) => panic!("expected the island to be rejected"),
            Err(error) => error.to_string(),
        }
    }

    fn counter() -> TokenStream {
        quote! {
            async fn counter(start: i32) -> Result {
                view! { <p>(start)</p> }
            }
        }
    }

    #[test]
    fn the_entry_point_is_the_name_the_loader_builds() {
        let island = island(counter());
        assert_eq!(island.name(), "counter");
        assert_eq!(island.entry_point().to_string(), "__island_counter");
    }

    #[test]
    fn both_halves_are_emitted_behind_the_cfg_that_selects_them() {
        let expanded = island(counter()).expand().to_string();
        assert!(
            expanded.contains("cfg (not (topcoat_client))"),
            "{expanded}"
        );
        assert!(expanded.contains("cfg (topcoat_client)"), "{expanded}");
    }

    #[test]
    fn every_item_of_a_half_carries_the_cfg_that_selects_it() {
        // A cfg applies to the ONE item that follows it, so a half of more than
        // one item needs an attribute each. A single attribute in front of the
        // list would compile everything after the first item into both halves,
        // and a server build would then name the client ABI and redefine what the
        // server half already declares.
        let island = island(counter());
        let expanded = island.expand().to_string();
        assert_eq!(
            expanded.matches("# [cfg (topcoat_client)]").count(),
            island.client().len(),
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
    fn a_view_calling_an_island_is_told_that_islands_do_not_nest() {
        let client = client(counter());
        // A call site builds the props struct, so it has to resolve: otherwise
        // the only error reported would be that it does not exist, which says
        // nothing about the constraint.
        assert!(
            client.contains("struct CounterProps { start : i32 , }"),
            "{client}",
        );
        assert!(
            client.contains("pub struct AnIslandCannotBeNestedInsideAnotherIsland ;"),
            "{client}",
        );
        // The island's own name resolves to a function whose parameter type is
        // the constraint, so the error lands on the call.
        assert!(
            client.contains(
                "fn counter (_ : __counter_island :: AnIslandCannotBeNestedInsideAnotherIsland ,) \
                 -> :: view_abi :: Node"
            ),
            "{client}",
        );
        // The entry point is still what the loader calls.
        assert!(
            client.contains("pub fn __island_counter (start : i32)"),
            "{client}",
        );
    }

    #[test]
    fn no_client_item_uses_a_panicking_macro() {
        // A client crate is expanded to a file and that file is compiled again,
        // and a panicking macro has already become `core::panicking::panic_fmt`
        // by then, which the second compilation rejects as an unstable internal.
        // So a client item's body cannot contain one, however unreachable it is.
        let client = client(counter());
        for banned in [
            "unreachable !",
            "panic !",
            "assert !",
            "todo !",
            "panicking",
        ] {
            assert!(!client.contains(banned), "{banned}: {client}");
        }
        // What stands in for one, in the only client item that needs to diverge.
        assert!(client.contains("loop { }"), "{client}");
    }

    #[test]
    fn the_server_half_renders_the_body_as_an_island_instance() {
        let server = island(counter()).server().to_string();
        assert!(server.contains("next_instance"), "{server}");
        assert!(server.contains("enter (__topcoat_island)"), "{server}");
        // The guard is released before the wrapper is built, so the wrapper's
        // own sites are outside the island and write no keys.
        let entered = server
            .find("drop (__topcoat_guard)")
            .expect("the guard is dropped");
        let wrapper = server
            .find("island_element")
            .expect("the body is wrapped for the loader");
        assert!(entered < wrapper, "{server}");
        assert!(server.contains("__topcoat_island ,"), "{server}");
    }

    #[test]
    fn the_client_half_exports_the_entry_point_the_loader_calls() {
        let client = client(counter());
        assert!(client.contains("no_mangle"), "{client}");
        assert!(
            client.contains("pub fn __island_counter (start : i32)"),
            "{client}"
        );
        assert!(client.contains("dom_view !"), "{client}");
    }

    #[test]
    fn arguments_are_seeded_as_a_json_array() {
        let server = island(counter()).server().to_string();
        assert!(server.contains("to_string (& (& start ,))"), "{server}");
    }

    #[test]
    fn an_island_with_no_arguments_needs_no_serializer() {
        let island = island(quote! {
            async fn plain() -> Result {
                view! { <p>"x"</p> }
            }
        });
        let server = island.server().to_string();
        assert!(!server.contains("serde_json"), "{server}");
        assert!(server.contains(r#""[]""#), "{server}");
    }

    #[test]
    fn a_body_that_is_not_a_view_is_rejected() {
        let message = error(quote! {
            async fn counter() -> Result {
                let x = 1;
                view! { <p>(x)</p> }
            }
        });
        assert!(message.contains("single `view!` invocation"), "{message}");
    }

    #[test]
    fn a_pattern_argument_is_rejected() {
        let message = error(quote! {
            async fn counter((a, b): (i32, i32)) -> Result {
                view! { <p>(a)</p> }
            }
        });
        assert!(message.contains("plain identifier"), "{message}");
    }

    #[test]
    fn an_island_that_renders_nothing_is_rejected() {
        let message = error(quote! {
            async fn counter() {
                view! { <p>"x"</p> }
            }
        });
        assert!(message.contains("returns one"), "{message}");
    }

    #[test]
    fn the_attribute_takes_no_arguments() {
        let message = Island::parse(quote! { name = "x" }, counter())
            .err()
            .expect("arguments are rejected")
            .to_string();
        assert!(message.contains("no arguments"), "{message}");
    }
}
