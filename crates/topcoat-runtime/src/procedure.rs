use std::{hash::Hash, marker::PhantomData, pin::Pin};

use ref_cast::RefCast;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use topcoat_core::{context::Cx, error::Result};
use topcoat_router::{
    Body, FromRequest, Method, Methods, Path, PathBuf, Response, Route, RouteFuture, RouterBuilder,
    content::Json, content_type, error::bad_request,
};

use crate::Surrogated;

const PROCEDURE_ROUTE_PREFIX: &str = "/_topcoat/procedures";

/// The media type a client sends serde-encoded call arguments with.
///
/// A procedure serves one wire format, and this is the one a client compiled to
/// the browser target uses: the arguments as a plain JSON array, each encoded by
/// its own [`Serialize`] implementation. The browser runtime that calls a
/// procedure from a runtime expression sends `application/json` instead, and its
/// arguments are encoded as surrogates, so the two are told apart by the media
/// type rather than by guessing at the body.
pub const SERDE_CONTENT_TYPE: &str = "application/topcoat+json";

/// Decodes a call's arguments from a serde-encoded request body.
///
/// `A` is the argument tuple: a call with one argument sends `[value]` and
/// decodes into `(T,)`. A call with no arguments sends `[]` and decodes into
/// `[(); 0]`, because an empty tuple is not an empty JSON array to serde: it is
/// `null`.
///
/// # Errors
///
/// Returns a bad-request error if the body was not sent as
/// [`SERDE_CONTENT_TYPE`], or if it does not decode into `A`.
pub async fn serde_args<A>(cx: &Cx, body: Body) -> Result<A>
where
    A: DeserializeOwned,
{
    if !is_serde_content_type(content_type(cx)) {
        return Err(bad_request(format!(
            "expected request with `Content-Type: {SERDE_CONTENT_TYPE}`"
        ))
        .into());
    }

    // The JSON body itself is read through the same extractor every other JSON
    // endpoint uses, so a malformed body is reported the same way, with the path
    // into the value that failed.
    let Json(args) = <Json<A> as FromRequest>::from_request(cx, body).await?;
    Ok(args)
}

/// Whether `content_type` is [`SERDE_CONTENT_TYPE`], ignoring any parameters
/// after it and the case of the media type itself.
fn is_serde_content_type(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return false;
    };
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim_ascii();
    media_type.eq_ignore_ascii_case(SERDE_CONTENT_TYPE)
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcedureId(&'static str);

impl ProcedureId {
    #[must_use]
    pub const fn new(inner: &'static str) -> Self {
        Self(inner)
    }

    #[must_use]
    fn as_str(&self) -> &str {
        self.0
    }
}

pub type ProcedureHandlerFn =
    for<'cx> fn(
        cx: &'cx Cx,
        body: Body,
    ) -> Pin<Box<dyn Future<Output = Result<Response>> + Send + 'cx>>;

#[derive(Debug, Clone)]
pub struct Procedure<A, R> {
    id: ProcedureId,
    handle: ProcedureHandlerFn,
    _phantom: PhantomData<fn(A) -> R>,
}

impl<A, R> Procedure<A, R> {
    #[inline]
    pub const fn new(id: ProcedureId, handle: ProcedureHandlerFn) -> Self {
        Self {
            id,
            handle,
            _phantom: PhantomData,
        }
    }

    #[must_use]
    pub fn id(&self) -> ProcedureId {
        self.id
    }
}

#[derive(Debug, Clone)]
pub struct ErasedProcedure {
    id: ProcedureId,
    handle: ProcedureHandlerFn,
}

impl ErasedProcedure {
    #[must_use]
    pub const fn new<A, R>(procedure: &Procedure<A, R>) -> Self {
        Self {
            id: procedure.id,
            handle: procedure.handle,
        }
    }

    #[must_use]
    pub fn id(&self) -> ProcedureId {
        self.id
    }

    /// Dispatches the procedure call, awaiting its handler future.
    ///
    /// # Errors
    ///
    /// Propagates any error returned by the underlying procedure handler.
    #[inline]
    pub async fn handle(&self, cx: &Cx, body: Body) -> Result<Response> {
        (self.handle)(cx, body).await
    }
}

impl<A, R> From<Procedure<A, R>> for ErasedProcedure {
    fn from(value: Procedure<A, R>) -> Self {
        Self {
            id: value.id,
            handle: value.handle,
        }
    }
}

impl<A, R> From<&Procedure<A, R>> for ErasedProcedure {
    fn from(value: &Procedure<A, R>) -> Self {
        Self::new(value)
    }
}

#[cfg(feature = "discover")]
inventory::collect!(ErasedProcedure);

/// A [`Route`] that handles calls to one server procedure.
#[derive(Debug, Clone)]
pub struct ProcedureRoute {
    path: PathBuf,
    procedure: ErasedProcedure,
}

impl ProcedureRoute {
    /// Builds the route that serves `procedure`.
    pub fn new(procedure: impl Into<ErasedProcedure>) -> Self {
        let procedure = procedure.into();
        Self {
            path: Path::new(&format!(
                "{PROCEDURE_ROUTE_PREFIX}/{}",
                procedure.id().as_str()
            ))
            .to_owned(),
            procedure,
        }
    }
}

impl Route for ProcedureRoute {
    fn methods(&self) -> Methods<'_> {
        Methods::Only(&[Method::POST])
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn handle<'cx>(&'cx self, cx: &'cx Cx, body: Body) -> RouteFuture<'cx> {
        Box::pin(async move { self.procedure.handle(cx, body).await })
    }
}

/// Registers server procedures on a [`RouterBuilder`].
pub trait RouterBuilderProcedureExt {
    /// Mounts a procedure route.
    #[must_use]
    fn procedure(self, procedure: impl Into<ErasedProcedure>) -> Self;

    /// Registers every procedure linked into the binary.
    #[cfg(feature = "discover")]
    #[must_use]
    fn discover_procedures(self) -> Self;
}

impl RouterBuilderProcedureExt for RouterBuilder {
    fn procedure(self, procedure: impl Into<ErasedProcedure>) -> Self {
        self.route(ProcedureRoute::new(procedure))
    }

    #[cfg(feature = "discover")]
    fn discover_procedures(mut self) -> Self {
        for procedure in inventory::iter::<ErasedProcedure>().cloned() {
            self = self.procedure(procedure);
        }
        self
    }
}

#[derive(Debug, RefCast)]
#[repr(transparent)]
pub struct ProcedureSurrogate<A, R>(Procedure<A, R>);

impl<A, R> ProcedureSurrogate<A, R> {
    pub(crate) const fn new(v: Procedure<A, R>) -> Self {
        Self(v)
    }
}

impl<A, R> ProcedureSurrogate<A, R>
where
    A: Surrogated,
    R: Surrogated,
{
    /// Invokes the procedure from the client side.
    ///
    /// # Panics
    ///
    /// Always panics; procedures can only be invoked from the client runtime.
    #[allow(clippy::unused_async)]
    pub async fn call(&self, _args: A::Surrogate) -> R::Surrogate {
        panic!("procedures cannot be executed on the server");
    }
}

crate::impl_surrogate!({A, R} Procedure<A, R>, ProcedureSurrogate<A, R>);
crate::impl_surrogate_ref!({A, R} Procedure<A, R>, ProcedureSurrogate<A, R>);
crate::impl_surrogate_mut!({A, R} Procedure<A, R>, ProcedureSurrogate<A, R>);

impl<A, R> Serialize for ProcedureSurrogate<A, R> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct TaggedProcedure {
            t: &'static str,
            id: ProcedureId,
        }

        TaggedProcedure {
            t: "Procedure",
            id: self.0.id(),
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use http::{Request, header::CONTENT_TYPE};
    use topcoat_core::context::CxTestBuilder;
    use topcoat_router::error::BadRequestError;

    use super::*;

    /// Builds a `Cx` carrying request parts with the given `Content-Type`.
    fn cx_with_content_type(content_type: Option<&str>) -> Cx {
        let mut builder = Request::builder();
        if let Some(content_type) = content_type {
            builder = builder.header(CONTENT_TYPE, content_type);
        }
        let (parts, ()) = builder.body(()).expect("the request builds").into_parts();
        CxTestBuilder::new().request_context(parts).build()
    }

    #[test]
    fn the_serde_media_type_is_recognized_with_parameters_and_in_any_case() {
        assert!(is_serde_content_type(Some(SERDE_CONTENT_TYPE)));
        assert!(is_serde_content_type(Some(
            "application/topcoat+json; charset=utf-8"
        )));
        assert!(is_serde_content_type(Some("Application/Topcoat+JSON")));
        assert!(is_serde_content_type(Some(" application/topcoat+json ")));
    }

    #[test]
    fn the_wire_the_browser_runtime_sends_is_not_the_serde_wire() {
        // The two wires carry different encodings of the same arguments, so
        // telling them apart is the whole point of the media type.
        assert!(!is_serde_content_type(Some("application/json")));
        assert!(!is_serde_content_type(None));
        assert!(!is_serde_content_type(Some("text/plain")));
        assert!(!is_serde_content_type(Some("application/topcoat")));
    }

    #[tokio::test]
    async fn serde_args_decodes_an_argument_array() {
        let cx = cx_with_content_type(Some(SERDE_CONTENT_TYPE));
        let args: (f64, String) = serde_args(&cx, Body::from(r#"[2.5,"x"]"#))
            .await
            .expect("a valid argument array");

        assert_eq!(args, (2.5, "x".to_owned()));
    }

    #[tokio::test]
    async fn serde_args_decodes_no_arguments_from_an_empty_array() {
        let cx = cx_with_content_type(Some(SERDE_CONTENT_TYPE));
        let args: [(); 0] = serde_args(&cx, Body::from("[]"))
            .await
            .expect("an empty argument array");

        assert_eq!(args, []);
    }

    #[tokio::test]
    async fn serde_args_rejects_the_zero_argument_body_of_the_other_wire() {
        // The browser runtime sends `null` for a call with no arguments; the
        // serde wire sends `[]`. The asymmetry is deliberate and unshared, so it
        // is worth pinning that each wire rejects the other's spelling.
        let cx = cx_with_content_type(Some(SERDE_CONTENT_TYPE));
        let error = serde_args::<[(); 0]>(&cx, Body::from("null"))
            .await
            .expect_err("`null` is not an empty argument array");

        assert!(error.downcast_ref::<BadRequestError>().is_some());
    }

    #[tokio::test]
    async fn serde_args_rejects_a_body_sent_as_the_other_wire() {
        for content_type in [None, Some("application/json")] {
            let cx = cx_with_content_type(content_type);
            let error = serde_args::<(f64,)>(&cx, Body::from("[1.0]"))
                .await
                .expect_err("the serde wire is asked for by media type");

            assert!(error.downcast_ref::<BadRequestError>().is_some());
            assert!(
                error.to_string().contains(SERDE_CONTENT_TYPE),
                "the error names the media type it wanted: {error}"
            );
        }
    }

    #[tokio::test]
    async fn serde_args_reports_where_an_argument_failed_to_decode() {
        let cx = cx_with_content_type(Some(SERDE_CONTENT_TYPE));
        let error = serde_args::<(f64, String)>(&cx, Body::from("[2.5,7]"))
            .await
            .expect_err("a number is not a string");

        assert!(error.downcast_ref::<BadRequestError>().is_some());
    }
}
