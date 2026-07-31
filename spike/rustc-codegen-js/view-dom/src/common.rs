//! What the constructs that are compiled twice have in common.
//!
//! An island and a client component are both written once and lowered by two
//! emitters, so both read their declaration the same way: a plain parameter
//! list, and a body that is a single `view!` invocation the two emitters parse
//! for themselves.

use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::{Expr, FnArg, Ident, Lifetime, Pat, Stmt, Type, TypeReference, Visibility};

/// The cfg that tells a construct which half of itself to compile.
pub(crate) const CLIENT_CFG: &str = "topcoat_client";

/// One declared parameter of a construct that is compiled twice.
pub(crate) struct Prop {
    pub(crate) ident: Ident,
    pub(crate) ty: Type,
}

impl Prop {
    /// Reads one declared parameter.
    pub(crate) fn parse(input: &FnArg) -> syn::Result<Self> {
        let FnArg::Typed(typed) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "a view is rendered by a free function, so it takes no `self`",
            ));
        };
        let Pat::Ident(pat) = &*typed.pat else {
            return Err(syn::Error::new_spanned(
                &typed.pat,
                "a parameter is passed to the client by name, so each one is a plain identifier \
                 rather than a pattern",
            ));
        };
        Ok(Self {
            ident: pat.ident.clone(),
            ty: (*typed.ty).clone(),
        })
    }

    /// The parameter as a function declares it.
    pub(crate) fn parameter(&self) -> TokenStream {
        let Self { ident, ty } = self;
        quote! { #ident: #ty }
    }
}

/// One half of a construct that is compiled twice: its items, each behind the
/// cfg that selects that half.
///
/// Every item carries an attribute of its own, because a cfg applies to the one
/// item that follows it. A single attribute in front of a list would leave
/// everything after the first item in BOTH compilations, which for a client item
/// means a server build that names the client ABI and redefines what the server
/// half already declares.
pub(crate) fn half(client: bool, items: impl IntoIterator<Item = TokenStream>) -> TokenStream {
    let cfg = Ident::new(CLIENT_CFG, Span::call_site());
    let condition = if client {
        quote! { #cfg }
    } else {
        quote! { not(#cfg) }
    };
    let items = items
        .into_iter()
        .map(|item| quote! { #[cfg(#condition)] #item });
    quote! { #(#items)* }
}

/// The name a construct's child content parameter has.
///
/// A view passes child content to a component by declaring it, the way the
/// server's own components do, and the two halves take it as the type each of
/// them renders: a view on the server, a built node on the client.
pub(crate) const CHILD_PROP: &str = "child";

/// The struct both halves of a construct declare its props in.
///
/// A call site names the fields rather than their order, which is what lets an
/// emitter build props without knowing how the construct declared them. A prop
/// that borrows needs the struct to name a lifetime, which no call site has to
/// spell: the field types are what carry it.
pub(crate) struct PropsStruct {
    /// The declaration, including its generic parameter list.
    pub(crate) declaration: TokenStream,
    /// The generic arguments a use of the struct is written with.
    pub(crate) arguments: TokenStream,
    /// The generic parameters a function taking the struct declares.
    pub(crate) parameters: TokenStream,
}

impl PropsStruct {
    /// The props struct of a construct with these props.
    ///
    /// `client_ty` maps each declared type to the type the client half takes it
    /// as.
    pub(crate) fn new(
        vis: &Visibility,
        ident: &Ident,
        props: &[Prop],
        client_ty: impl Fn(&Prop) -> Type,
    ) -> Self {
        let mut borrows = Borrows::default();
        let mut hoisted = Vec::new();
        let fields: Vec<_> = props
            .iter()
            .map(|prop| {
                let name = &prop.ident;
                // An `impl Trait` parameter is already sugar for a generic
                // parameter, and a struct field cannot hold the sugar, so the
                // desugaring is written out. It happens before lifetimes are
                // named, because the bounds move to a parameter list where the
                // struct's own lifetime is not in scope.
                let mut ty = client_ty(prop);
                hoisted.extend(Hoist::new(&prop.ident).hoist(&mut ty));
                let ty = borrows.borrow(&ty);
                quote! { #vis #name: #ty }
            })
            .collect();

        // The struct is declared with the lifetime its props borrow for and a
        // parameter per desugared prop; a use of it leaves the lifetime elided and
        // names the parameters, which inference fills at the call site. A function
        // taking it declares the parameters and no lifetime, for the same reason.
        let lifetime = borrows.lifetime();
        let bounded: Vec<_> = hoisted
            .iter()
            .map(|(name, bounds)| quote! { #name: #bounds })
            .collect();
        let declared = generics(
            lifetime
                .iter()
                .map(|lifetime| quote! { #lifetime })
                .chain(bounded.iter().cloned()),
        );
        let arguments = generics(
            lifetime
                .iter()
                .map(|_| quote! { '_ })
                .chain(hoisted.iter().map(|(name, _)| quote! { #name })),
        );
        let parameters = generics(bounded.into_iter());

        Self {
            declaration: quote! {
                #vis struct #ident #declared {
                    #(#fields,)*
                }
            },
            arguments,
            parameters,
        }
    }
}

/// The generic parameters an `impl Trait` prop desugars to.
struct Hoist {
    prop: Ident,
    taken: usize,
    hoisted: Vec<(Ident, TokenStream)>,
}

impl Hoist {
    fn new(prop: &Ident) -> Self {
        Self {
            prop: prop.clone(),
            taken: 0,
            hoisted: Vec::new(),
        }
    }

    /// Replaces every `impl Trait` in `ty` with a type parameter, returning each
    /// parameter and the bounds it carries.
    fn hoist(mut self, ty: &mut Type) -> Vec<(Ident, TokenStream)> {
        self.visit_type_mut(ty);
        self.hoisted
    }

    /// The name of the next parameter this prop contributes.
    ///
    /// It is named after the prop rather than by position, so an error about a
    /// bound names something the declaration contains, and it carries a prefix
    /// no type of the crate's own would.
    fn name(&mut self) -> Ident {
        let mut name = String::from("__Tc");
        for word in self.prop.to_string().split('_') {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                name.push(first.to_ascii_uppercase());
                name.push_str(chars.as_str());
            }
        }
        self.taken += 1;
        if self.taken > 1 {
            name.push_str(&self.taken.to_string());
        }
        Ident::new(&name, Span::call_site())
    }
}

impl VisitMut for Hoist {
    fn visit_type_mut(&mut self, node: &mut Type) {
        if let Type::ImplTrait(sugar) = node {
            let bounds = sugar.bounds.clone();
            let name = self.name();
            self.hoisted.push((name.clone(), quote! { #bounds }));
            *node = syn::parse_quote!(#name);
            return;
        }
        visit_mut::visit_type_mut(self, node);
    }
}

/// A generic parameter or argument list, empty when there is nothing in it.
fn generics(parts: impl Iterator<Item = TokenStream>) -> TokenStream {
    let parts: Vec<_> = parts.collect();
    if parts.is_empty() {
        return TokenStream::new();
    }
    quote! { <#(#parts),*> }
}

/// The lifetime a props struct needs, if any of its props borrow.
#[derive(Default)]
struct Borrows {
    used: bool,
}

impl Borrows {
    /// `ty` with every reference in it borrowed for the props lifetime.
    fn borrow(&mut self, ty: &Type) -> Type {
        let mut ty = ty.clone();
        self.visit_type_mut(&mut ty);
        ty
    }

    /// The lifetime the struct is declared with, if any of its props borrow.
    fn lifetime(&self) -> Option<Lifetime> {
        self.used.then(|| self.named())
    }

    fn named(&self) -> Lifetime {
        Lifetime::new(&format!("'{PROPS_LIFETIME}"), Span::call_site())
    }
}

impl VisitMut for Borrows {
    fn visit_type_reference_mut(&mut self, node: &mut TypeReference) {
        if node.lifetime.is_none() {
            node.lifetime = Some(self.named());
            self.used = true;
        }
        visit_mut::visit_type_reference_mut(self, node);
    }

    fn visit_type_mut(&mut self, node: &mut Type) {
        // An elided lifetime written out, which a struct field cannot leave
        // elided either.
        if let Type::Path(path) = node {
            for segment in &mut path.path.segments {
                if let syn::PathArguments::AngleBracketed(args) = &mut segment.arguments {
                    for arg in &mut args.args {
                        if let syn::GenericArgument::Lifetime(lifetime) = arg {
                            if lifetime.ident == "_" {
                                *lifetime = self.named();
                                self.used = true;
                            }
                        }
                    }
                }
            }
        }
        visit_mut::visit_type_mut(self, node);
    }
}

/// The lifetime a props struct borrows its props for.
const PROPS_LIFETIME: &str = "__tc_props";

/// The body of the single `view!` invocation a construct's block is.
///
/// Both halves lower the same tokens, so the body is taken as tokens and never
/// as a parsed view: the two emitters parse it for themselves.
pub(crate) fn view_body(block: &syn::Block, what: &str) -> syn::Result<TokenStream> {
    let unsupported = |span: Span| {
        syn::Error::new(
            span,
            format!(
                "{what}'s body is a single `view!` invocation, because the server and the client \
                 lower the same view",
            ),
        )
    };

    // A brace-delimited invocation in tail position parses as a statement
    // macro; a parenthesized or bracketed one parses as an expression.
    let call = match block.stmts.as_slice() {
        [Stmt::Macro(call)] if call.semi_token.is_none() => &call.mac,
        [Stmt::Expr(Expr::Macro(call), None)] => &call.mac,
        _ => return Err(unsupported(block.span())),
    };
    if !call.path.is_ident("view") {
        return Err(unsupported(call.path.span()));
    }
    Ok(call.tokens.clone())
}
