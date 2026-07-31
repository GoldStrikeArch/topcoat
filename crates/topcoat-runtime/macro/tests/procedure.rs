//! The wires a procedure serves, at the level a client meets them: an HTTP
//! request against a router the procedure is mounted on.
//!
//! A procedure serves one wire. The surrogate wire is what the browser runtime
//! calls from a runtime expression, and it is asked for with
//! `Content-Type: application/json`. The serde wire is what a client compiled to
//! the browser target calls, it is asked for with
//! [`SERDE_CONTENT_TYPE`](topcoat::runtime::SERDE_CONTENT_TYPE), and its
//! arguments are encoded by their own `Serialize` implementations rather than as
//! surrogates. Which wire a procedure serves is written on the declaration, so
//! these cases are also what pins that a procedure never guesses at a body.
//!
//! Most of the cases are not written here. `fixtures/procedure_wire.json` holds
//! them, distilled from the contract fixture that is the single source for both
//! this side of the wire and the client that produces it. Each vector names the
//! wire and the signature it is about, and the procedures below are one per
//! signature the fixture uses, so a vector nobody serves fails loudly rather than
//! being skipped.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use topcoat::{
    Result,
    router::{Body, Response, Route, Router, to_bytes},
    runtime::{
        ErasedProcedure, ProcedureRoute, RouterBuilderProcedureExt, SERDE_CONTENT_TYPE, procedure,
    },
};

/// A type that is only serde's: nothing implements the shared vocabulary for it,
/// which is what a procedure on the serde wire has to be free of.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Input {
    name: String,
    count: u32,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Output {
    ok: bool,
}

#[procedure(serde)]
async fn serde_two(a: f64, b: String) -> Result<String> {
    Ok(format!("{a}:{b}"))
}

#[procedure(serde)]
async fn serde_one(a: f64) -> Result<f64> {
    Ok(a * 2.0)
}

#[procedure(serde)]
async fn serde_none() -> Result<String> {
    Ok("pong".to_owned())
}

#[procedure(serde)]
async fn serde_input(input: Input) -> Result<Input> {
    Ok(input)
}

#[procedure]
async fn surrogate_two(a: f64, b: String) -> Result<String> {
    Ok(format!("{a}:{b}"))
}

#[procedure]
async fn surrogate_none() -> Result<String> {
    Ok("pong".to_owned())
}

#[procedure(serde)]
async fn returns_scalar() -> Result<f64> {
    Ok(6.0)
}

#[procedure(serde)]
async fn returns_string() -> Result<String> {
    Ok("ok".to_owned())
}

#[procedure(serde)]
async fn returns_unit() -> Result<()> {
    Ok(())
}

#[procedure(serde)]
async fn returns_struct() -> Result<Output> {
    Ok(Output { ok: true })
}

#[procedure(serde)]
async fn refuse() -> Result<f64> {
    Err(topcoat::router::error::not_found().into())
}

/// The path a procedure is mounted at.
fn path(procedure: impl Into<ErasedProcedure>) -> String {
    ProcedureRoute::new(procedure).path().to_string()
}

/// Sends one request to a procedure's own route.
async fn send(
    procedure: impl Into<ErasedProcedure>,
    method: &str,
    path_override: Option<&str>,
    content_type: Option<&str>,
    body: Option<&str>,
) -> Response {
    let procedure = procedure.into();
    let uri = path_override.map_or_else(|| path(procedure.clone()), ToOwned::to_owned);
    let router = Router::builder().procedure(procedure).build();

    let mut request = http::Request::builder().method(method).uri(uri);
    if let Some(content_type) = content_type {
        request = request.header(http::header::CONTENT_TYPE, content_type);
    }
    let body = body.map_or_else(Body::empty, |body| Body::from(body.to_owned()));
    router
        .handle(request.body(body).expect("the request builds"))
        .await
}

/// The status, the content type and the body of a response.
async fn read(response: Response) -> (u16, Option<String>, String) {
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .map(|value| value.to_str().expect("a header of text").to_owned());
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the body is read");
    (
        status,
        content_type,
        String::from_utf8(bytes.to_vec()).expect("utf-8"),
    )
}

/// Posts `body` to a procedure and returns its status and body.
async fn call(
    procedure: impl Into<ErasedProcedure>,
    content_type: Option<&str>,
    body: &str,
) -> (u16, String) {
    let (status, _, body) =
        read(send(procedure, "POST", None, content_type, Some(body)).await).await;
    (status, body)
}

/// The shared vectors, in file order.
fn vectors() -> Vec<Value> {
    let fixture = include_str!("fixtures/procedure_wire.json");
    let fixture: Value = serde_json::from_str(fixture).expect("the fixture is json");
    fixture["vectors"]
        .as_array()
        .expect("vectors is an array")
        .clone()
}

fn text(vector: &Value, key: &str) -> Option<String> {
    vector[key].as_str().map(ToOwned::to_owned)
}

/// The procedure a request vector is served by, named by the wire and the
/// signature it declares.
///
/// A signature nobody serves is a panic rather than a skip: a vector the fixture
/// gains has to be answered here, or it would silently not be tested at all.
fn served_by(vector: &Value) -> ErasedProcedure {
    let wire = text(vector, "wire").expect("a wire");
    let signature = text(vector, "signature").unwrap_or_default();
    match (wire.as_str(), signature.as_str()) {
        ("serde", "async fn f(a: f64, b: String) -> Result<..>") => serde_two.into(),
        ("serde", "async fn f(a: f64) -> Result<..>") => serde_one.into(),
        // A vector about the route rather than about a body names no signature.
        ("serde", "async fn f() -> Result<..>") | ("both", "") => serde_none.into(),
        (
            "serde",
            "async fn f(input: Input) -> Result<..> where Input { name: String, count: u32 }",
        ) => serde_input.into(),
        ("surrogate", "async fn f(a: f64, b: String) -> Result<..>") => surrogate_two.into(),
        ("surrogate", "async fn f() -> Result<..>") => surrogate_none.into(),
        (wire, signature) => panic!("no procedure serves the {wire} wire's `{signature}`"),
    }
}

/// The procedure a response vector is served by, named by what it returns. Each
/// one takes no arguments, so the vector's response is reached with `[]`.
fn returns(vector: &Value) -> ErasedProcedure {
    match text(vector, "returns").expect("a return type").as_str() {
        "Result<f64>" => returns_scalar.into(),
        "Result<String>" => returns_string.into(),
        "Result<()>" => returns_unit.into(),
        "Result<Output> where Output { ok: bool }" => returns_struct.into(),
        returns => panic!("no procedure returns `{returns}`"),
    }
}

#[tokio::test]
async fn every_shared_vector_is_what_the_wire_answers() {
    let vectors = vectors();
    assert_eq!(vectors.len(), 19, "the fixture should carry every vector");

    for vector in &vectors {
        let id = text(vector, "id").expect("an id");
        if text(vector, "direction").as_deref() == Some("response") {
            // A response vector pins what comes back for a return type, so it is
            // reached by calling a procedure that returns one.
            let sent = send(
                returns(vector),
                "POST",
                None,
                Some(SERDE_CONTENT_TYPE),
                Some("[]"),
            )
            .await;
            let (status, content_type, body) = read(sent).await;
            assert_eq!(
                u64::from(status),
                vector["status"].as_u64().expect("a status"),
                "{id}",
            );
            assert_eq!(
                content_type.as_deref(),
                text(vector, "contentType").as_deref(),
                "{id}",
            );
            assert_eq!(Some(body.as_str()), text(vector, "body").as_deref(), "{id}");
            continue;
        }

        let expect = &vector["expect"];
        let sent = send(
            served_by(vector),
            text(vector, "method").as_deref().unwrap_or("POST"),
            text(vector, "path").as_deref(),
            text(vector, "contentType").as_deref(),
            text(vector, "body").as_deref(),
        )
        .await;
        let (status, _, body) = read(sent).await;

        assert_eq!(
            u64::from(status),
            expect["status"].as_u64().expect("a status"),
            "{id}: {body}",
        );
        if let Some(exactly) = expect["bodyExactly"].as_str() {
            assert_eq!(body, exactly, "{id}");
        }
        // The middle of a decode error is serde_json's own wording, so only the
        // ends are pinned: a patch release of it must not fail this.
        if let Some(prefix) = expect["bodyStartsWith"].as_str() {
            assert!(body.starts_with(prefix), "{id}: {body}");
        }
        if let Some(suffix) = expect["bodyEndsWith"].as_str() {
            assert!(body.ends_with(suffix), "{id}: {body}");
        }
    }
}

#[tokio::test]
async fn an_argument_decodes_to_the_value_the_vector_names() {
    // The vectors assert a status, which cannot tell a decoded argument from a
    // discarded one, so what the arguments became is checked here.
    assert_eq!(
        call(serde_two, Some(SERDE_CONTENT_TYPE), r#"[2.5,"xx"]"#).await,
        (200, "\"2.5:xx\"".to_owned()),
    );
    assert_eq!(
        call(serde_one, Some(SERDE_CONTENT_TYPE), "[2.5]").await,
        (200, "5.0".to_owned()),
    );
    assert_eq!(
        call(
            serde_input,
            Some(SERDE_CONTENT_TYPE),
            r#"[{"count":3,"name":"x"}]"#
        )
        .await,
        (200, r#"{"name":"x","count":3}"#.to_owned()),
    );
    assert_eq!(
        call(surrogate_two, Some("application/json"), r#"[2.5,"xx"]"#).await,
        (200, "\"2.5:xx\"".to_owned()),
    );
}

#[tokio::test]
async fn the_surrogate_wire_rejects_the_serde_wires_zero_argument_body() {
    // The fixture pins the other direction, the serde wire rejecting `null`; this
    // is the half of the asymmetry it does not carry.
    assert_eq!(
        call(surrogate_none, Some("application/json"), "[]").await.0,
        400,
    );
}

#[tokio::test]
async fn a_procedure_that_fails_answers_with_its_own_status() {
    // The error a procedure returns is the response, so a status it chose
    // survives the wire rather than becoming a decode failure.
    assert_eq!(call(refuse, Some(SERDE_CONTENT_TYPE), "[]").await.0, 404);
}
