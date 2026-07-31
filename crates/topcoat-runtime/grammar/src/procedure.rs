use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote};
use syn::{
    FnArg, Ident, ItemFn, Pat, PatIdent, PatType, ReturnType, Type, Visibility,
    parse::{Parse, ParseStream},
    spanned::Spanned,
};
use topcoat_core_grammar::paths::{
    topcoat_dom, topcoat_internal, topcoat_inventory, topcoat_router, topcoat_runtime,
};

mod kw {
    syn::custom_keyword!(serde);
}

/// The name of the cfg that a crate compiled for the client is built with.
const CLIENT_CFG: &str = "topcoat_client";

/// Arguments passed to the `#[procedure]` attribute itself.
///
/// `serde` opts the procedure onto the serde wire: its arguments and its result
/// cross as plain JSON encoded by their own `Serialize` and `Deserialize`
/// implementations, and the procedure gains the client-side function that calls
/// it.
pub struct ProcedureAttr {
    serde: Option<kw::serde>,
}

impl ProcedureAttr {
    /// Whether the procedure was declared on the serde wire.
    fn is_serde(&self) -> bool {
        self.serde.is_some()
    }
}

impl Parse for ProcedureAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(Self { serde: None });
        }
        let serde = input.parse::<kw::serde>()?;
        if !input.is_empty() {
            return Err(input.error("`#[procedure]` takes at most one argument, `serde`"));
        }
        Ok(Self { serde: Some(serde) })
    }
}

/// The annotated `async fn` that becomes a procedure. Validates the function
/// signature: procedures must be `async`, must declare a return type, and must
/// not take a `self` receiver.
pub struct ProcedureItem {
    item: ItemFn,
}

impl Parse for ProcedureItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let item: ItemFn = input.parse()?;
        if item.sig.asyncness.is_none() {
            return Err(syn::Error::new(
                item.sig.fn_token.span(),
                "procedures must be async",
            ));
        }
        if let ReturnType::Default = &item.sig.output {
            return Err(syn::Error::new(
                item.sig.fn_token.span(),
                "procedures must have a return type",
            ));
        }
        for arg in &item.sig.inputs {
            if let FnArg::Receiver(r) = arg {
                return Err(syn::Error::new_spanned(
                    r,
                    "procedure functions cannot take a `self` receiver",
                ));
            }
        }
        Ok(Self { item })
    }
}

pub struct Procedure(ProcedureAttr, ProcedureItem);

impl Procedure {
    #[must_use]
    pub fn new(attr: ProcedureAttr, item: ProcedureItem) -> Self {
        Self(attr, item)
    }

    /// Parses a procedure from its attribute and item token streams.
    ///
    /// # Errors
    ///
    /// Returns an error if either token stream fails to parse as a procedure
    /// attribute or function item.
    pub fn parse(attr: TokenStream, item: TokenStream) -> syn::Result<Self> {
        Ok(Self::new(syn::parse2(attr)?, syn::parse2(item)?))
    }
}

impl ToTokens for Procedure {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ident = &self.1.item.sig.ident;

        let vis = &self.1.item.vis;
        let docs = self
            .1
            .item
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("doc"));
        let mut item = self.1.item.clone();
        item.vis = Visibility::Inherited;

        let mut args = Vec::new();
        let mut args_with_cx = Vec::new();
        let mut arg_names = Vec::new();
        let mut arg_index = 0;
        for arg in &item.sig.inputs {
            match arg {
                FnArg::Typed(PatType { pat, .. }) => match &**pat {
                    Pat::Ident(PatIdent { ident, .. }) if ident == "cx" => {
                        args_with_cx.push(ident.clone());
                    }
                    pat => {
                        let arg = format_ident!("arg{arg_index}");
                        // The client-side function is what a caller reads, so it
                        // keeps the name the procedure declared wherever the
                        // parameter has one.
                        arg_names.push(match pat {
                            Pat::Ident(PatIdent { ident, .. }) => ident.clone(),
                            _ => arg.clone(),
                        });
                        args.push(arg.clone());
                        args_with_cx.push(arg);
                        arg_index += 1;
                    }
                },
                FnArg::Receiver(_) => unreachable!("validated by ProcedureItem"),
            }
        }

        let arg_tys = item
            .sig
            .inputs
            .iter()
            .filter_map(|arg| match arg {
                FnArg::Typed(PatType { pat, ty, .. }) => match &**pat {
                    Pat::Ident(PatIdent { ident, .. }) if ident == "cx" => None,
                    _ => Some(&**ty),
                },
                FnArg::Receiver(_) => None,
            })
            .collect::<Vec<_>>();
        let ReturnType::Type(_, declared_output) = &item.sig.output else {
            unreachable!("validated by ProcedureItem")
        };
        let return_ty = quote! { <#declared_output as #topcoat_internal::ResultExt>::T };

        let id = uuid::Uuid::new_v4().to_string();

        // The wire the procedure serves. The surrogate wire decodes what the
        // browser runtime sends from a runtime expression; the serde wire decodes
        // what a client compiled to the browser target sends. A procedure serves
        // one of them, because serving both would put `Surrogated` back on every
        // argument type and that is exactly what the serde wire is for.
        let handler = if self.0.is_serde() {
            // An empty tuple is `null` to serde rather than an empty array, so a
            // call with no arguments decodes the empty array as one.
            let args_ty = if arg_tys.is_empty() {
                quote! { [(); 0] }
            } else {
                quote! { (#(#arg_tys,)*) }
            };
            let args_pat = if args.is_empty() {
                quote! { _ }
            } else {
                quote! { (#(#args,)*) }
            };
            quote! {
                let #args_pat: #args_ty = #topcoat_runtime::serde_args(cx, body).await?;
                let response = #ident(#(#args_with_cx),*).await?;
                #topcoat_router::IntoResponse::into_response(#topcoat_router::content::Json(response), cx)
            }
        } else {
            quote! {
                type Surrogate = <(#(#arg_tys,)*) as #topcoat_runtime::Surrogated>::Surrogate;
                let #topcoat_router::content::Json(args) = <#topcoat_router::content::Json<Surrogate> as #topcoat_router::FromRequest>::from_request(cx, body).await?;
                let (#(#args,)*) = #topcoat_runtime::Surrogate::into_real(args);
                let response = #topcoat_runtime::Surrogated::into_surrogate(#ident(#(#args_with_cx),*).await?);
                #topcoat_router::IntoResponse::into_response(#topcoat_router::content::Json(response), cx)
            }
        };

        let server = quote! {
            #(#docs)*
            #[allow(non_upper_case_globals)]
            #vis const #ident: &#topcoat_runtime::Procedure::<(#(#arg_tys,)*), #return_ty> = &#topcoat_runtime::Procedure::new(
                #topcoat_runtime::ProcedureId::new(#id),
                |cx, body| {
                    #[allow(clippy::unused_async)]
                    #item
                    Box::pin(async {
                        #handler
                    })
                },
            );
        };
        let registration = cfg!(feature = "discover").then(|| {
            quote! { #topcoat_inventory::submit! { #topcoat_runtime::ErasedProcedure::new(#ident) } }
        });

        if !self.0.is_serde() {
            quote! {
                #server
                #registration
            }
            .to_tokens(tokens);
            return;
        }

        // On the serde wire the two halves are the same procedure compiled for
        // one target each: the server holds the body and the route, the client
        // holds the call. Which one a crate gets is the cfg, so the body's
        // server-only code never has to compile for the client at all.
        //
        // The registration's cfg rides inside the option: with `discover` off
        // the interpolation is empty, and a bare attribute left behind would
        // fall onto the client item and strip it under every flag.
        let cfg = Ident::new(CLIENT_CFG, Span::call_site());
        let client = self.client(&id, &arg_names, &arg_tys, declared_output);
        let registration = registration.map(|registration| {
            quote! {
                #[cfg(not(#cfg))]
                #registration
            }
        });
        quote! {
            #[cfg(not(#cfg))]
            #server
            #registration

            #[cfg(#cfg)]
            #client
        }
        .to_tokens(tokens);
    }
}

impl Procedure {
    /// The client-side function that calls the procedure over the serde wire.
    ///
    /// The arguments are serialized as a JSON array, the call is awaited, and the
    /// reply is deserialized into the procedure's own `Ok` type. Whether the
    /// future the call returns is a compiled coroutine or an adapter over a
    /// callback is the client runtime's business, not this expansion's.
    fn client(
        &self,
        id: &str,
        arg_names: &[Ident],
        arg_tys: &[&Type],
        declared_output: &Type,
    ) -> TokenStream {
        let item = &self.1.item;
        let ident = &item.sig.ident;
        let vis = &item.vis;
        let docs = item.attrs.iter().filter(|attr| attr.path().is_ident("doc"));

        // A call with no arguments sends the empty array the server decodes,
        // which no value serializes to: `()` is `null`.
        let args = if arg_names.is_empty() {
            quote! { "[]" }
        } else {
            quote! { &#topcoat_dom::json::to_json(&(#(&#arg_names,)*))? }
        };
        quote! {
            #(#docs)*
            #vis async fn #ident(
                #(#arg_names: #arg_tys,)*
            ) -> #topcoat_dom::Result<<#declared_output as #topcoat_dom::ResultExt>::T> {
                let __topcoat_reply = #topcoat_dom::procedure::call(#id, #args).await?;
                #topcoat_dom::json::from_json(&__topcoat_reply)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_err(source: &str) -> String {
        match syn::parse_str::<ProcedureItem>(source) {
            Ok(_) => panic!("expected parse error for `{source}`"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn accepts_async_fn_with_return_type() {
        syn::parse_str::<ProcedureItem>("async fn double(cx: &Cx, value: f64) -> Result<f64> {}")
            .unwrap();
    }

    #[test]
    fn accepts_a_destructured_argument() {
        // Only argument types reach the generated call, so destructuring
        // patterns stay valid; the re-emitted function unpacks them.
        syn::parse_str::<ProcedureItem>(
            "async fn shift((x, y): (f64, f64)) -> Result<(f64, f64)> {}",
        )
        .unwrap();
    }

    #[test]
    fn rejects_non_async_fn() {
        assert!(parse_err("fn double() -> Result<f64> {}").contains("must be async"));
    }

    #[test]
    fn rejects_missing_return_type() {
        assert!(parse_err("async fn double() {}").contains("must have a return type"));
    }

    #[test]
    fn rejects_self_receiver() {
        let err = parse_err("async fn double(&self) -> Result<f64> {}");
        assert!(err.contains("cannot take a `self` receiver"));
    }

    /// The expansion of a procedure, as one line of tokens.
    fn expand(attr: TokenStream, item: TokenStream) -> String {
        match Procedure::parse(attr, item) {
            Ok(procedure) => procedure.to_token_stream().to_string(),
            Err(error) => panic!("expected the procedure to expand: {error}"),
        }
    }

    fn double() -> TokenStream {
        quote! {
            async fn double(value: f64) -> Result<f64> {
                Ok(value * 2.0)
            }
        }
    }

    /// Every occurrence of the generated procedure id in an expansion.
    fn ids(expanded: &str) -> Vec<&str> {
        expanded
            .split("ProcedureId :: new (\"")
            .skip(1)
            .map(|rest| rest.split('"').next().expect("the literal is closed"))
            .collect()
    }

    #[test]
    fn the_surrogate_wire_is_what_a_procedure_serves_by_default() {
        let expanded = expand(TokenStream::new(), double());
        assert!(
            expanded.contains("as :: topcoat_runtime :: Surrogated"),
            "{expanded}"
        );
        assert!(
            expanded.contains("Surrogate :: into_real (args)"),
            "{expanded}"
        );
        // No wire is chosen per request: the surrogate wire has no client half
        // and so no cfg either.
        assert!(!expanded.contains("topcoat_client"), "{expanded}");
        assert!(!expanded.contains("serde_args"), "{expanded}");
    }

    #[test]
    fn the_serde_wire_decodes_the_arguments_and_serializes_the_result() {
        let expanded = expand(quote! { serde }, double());
        assert!(
            expanded.contains(
                "let (arg0 ,) : (f64 ,) = :: topcoat_runtime :: serde_args (cx , body) . await ?"
            ),
            "{expanded}",
        );
        assert!(
            expanded.contains("let response = double (arg0) . await ?"),
            "{expanded}",
        );
        assert!(expanded.contains("Json (response) , cx)"), "{expanded}");
        // Nothing surrogate is left on this wire: that is what frees the
        // argument types from having to be part of the shared vocabulary.
        assert!(!expanded.contains("Surrogated"), "{expanded}");
    }

    #[test]
    fn a_procedure_with_no_arguments_decodes_an_empty_array_on_the_serde_wire() {
        let expanded = expand(
            quote! { serde },
            quote! {
                async fn ping() -> Result<String> {
                    Ok(String::new())
                }
            },
        );
        assert!(
            expanded.contains(
                "let _ : [() ; 0] = :: topcoat_runtime :: serde_args (cx , body) . await ?"
            ),
            "{expanded}",
        );
        assert!(expanded.contains("call (\""), "{expanded}");
        assert!(expanded.contains(", \"[]\") . await ?"), "{expanded}");
    }

    #[test]
    fn the_serde_wire_compiles_one_half_per_target() {
        let expanded = expand(quote! { serde }, double());
        assert!(
            expanded.contains("# [cfg (not (topcoat_client))]"),
            "{expanded}"
        );
        assert!(expanded.contains("# [cfg (topcoat_client)]"), "{expanded}");
        // The body is the server's alone, so server-only code in it never has to
        // compile for the client.
        assert_eq!(expanded.matches("value * 2.0").count(), 1, "{expanded}");
    }

    #[test]
    fn the_client_half_carries_exactly_one_cfg() {
        // With `discover` off the registration interpolates to nothing, and a
        // bare `#[cfg(not(topcoat_client))]` left standing would fall onto the
        // client item — which then carries both cfgs and is stripped under
        // every flag. Found by compiling the expansion, not by these tests,
        // which is why this one pins the adjacency rather than the counts.
        let expanded = expand(quote! { serde }, double());
        assert!(
            !expanded.contains("# [cfg (not (topcoat_client))] # [cfg (topcoat_client)]"),
            "a bare server cfg fell onto the client item: {expanded}",
        );
    }

    #[test]
    fn both_halves_name_the_same_procedure() {
        let expanded = expand(quote! { serde }, double());
        let ids = ids(&expanded);
        assert_eq!(ids.len(), 1, "{expanded}");
        // The client half calls the id the server registered, so the two cannot
        // drift: they are written from one uuid.
        assert!(
            expanded.contains(&format!("procedure :: call (\"{}\"", ids[0])),
            "{expanded}",
        );
    }

    #[test]
    fn the_client_half_is_an_async_function_over_the_declared_arguments() {
        let expanded = expand(quote! { serde }, double());
        assert!(
            expanded.contains(
                "async fn double (value : f64 ,) -> :: topcoat_dom :: Result << Result < f64 > as :: topcoat_dom :: ResultExt > :: T >"
            ),
            "{expanded}",
        );
        assert!(
            expanded.contains("let __topcoat_reply = :: topcoat_dom :: procedure :: call ("),
            "{expanded}",
        );
        assert!(
            expanded.contains(":: topcoat_dom :: json :: to_json (& (& value ,)) ?"),
            "{expanded}",
        );
        assert!(
            expanded.contains(":: topcoat_dom :: json :: from_json (& __topcoat_reply)"),
            "{expanded}",
        );
    }

    #[test]
    fn a_request_context_is_not_part_of_the_call_the_client_makes() {
        let expanded = expand(
            quote! { serde },
            quote! {
                async fn search(cx: &Cx, query: String) -> Result<String> {
                    Ok(query)
                }
            },
        );
        assert!(
            expanded.contains("async fn search (query : String ,)"),
            "{expanded}",
        );
        assert!(expanded.contains("to_json (& (& query ,))"), "{expanded}");
        // The server still fills it from the request context.
        assert!(expanded.contains("search (cx , arg0)"), "{expanded}");
    }

    #[test]
    fn a_destructured_argument_keeps_its_position_on_the_client() {
        let expanded = expand(
            quote! { serde },
            quote! {
                async fn shift((x, y): (f64, f64)) -> Result<f64> {
                    Ok(x + y)
                }
            },
        );
        // A pattern has no name to borrow, so the client half names it by
        // position; the wire is the array either way.
        assert!(
            expanded.contains("async fn shift (arg0 : (f64 , f64) ,)"),
            "{expanded}",
        );
    }

    #[test]
    fn only_serde_is_accepted_as_an_argument() {
        let error = Procedure::parse(quote! { json }, double())
            .err()
            .expect("an unknown argument is rejected")
            .to_string();
        assert!(error.contains("expected `serde`"), "{error}");

        let error = Procedure::parse(quote! { serde, json }, double())
            .err()
            .expect("a second argument is rejected")
            .to_string();
        assert!(error.contains("at most one argument"), "{error}");
    }
}
