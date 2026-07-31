//! The search procedure: the server half of the search island.
//!
//! Two endpoints answer the same query from the same catalogue, over the same
//! wire:
//!
//! * [`search`], a `#[procedure(serde)]`, which is the surface a Topcoat app uses. Its URL carries
//!   a uuid minted when the macro expanded, so it is stable for the life of a binary.
//! * [`search_endpoint`], a plain route at a URL that is written down. It exists because a
//!   procedure's uuid is minted PER EXPANSION, and an island's file is expanded twice: once by the
//!   server crate and once by the client crate. The two expansions mint different uuids, so a
//!   procedure declared in a shared file cannot address itself from the client half. A compiled
//!   island therefore needs a URL that does not depend on an expansion, which is this one.
//!
//! Both decode their arguments with [`serde_args`], so both speak exactly the
//! wire `contract/fixtures/procedure-wire.json` describes: `POST`, a request
//! `Content-Type` of `application/topcoat+json`, a JSON ARRAY of arguments even
//! for a single one, and a JSON response.

use topcoat::{
    Result,
    context::Cx,
    router::{Body, content::Json, route},
    runtime::{procedure, serde_args},
};

/// What the search answers from.
///
/// A static list rather than a database: what the island exercises is the round
/// trip, and a query that has to reach a server to be answered is enough for
/// that.
const CATALOGUE: &[&str] = &[
    "rust", "ruby", "trust", "crust", "python", "perl", "ocaml", "haskell", "elixir", "erlang",
    "zig", "nim", "go", "swift", "kotlin",
];

/// Every catalogue entry containing `query`, in catalogue order.
///
/// An empty query answers nothing rather than everything: the island calls this
/// as the user types, and a first keystroke that rendered the whole catalogue
/// would be a worse answer than no answer.
fn hits(query: &str) -> Vec<&'static str> {
    if query.is_empty() {
        return Vec::new();
    }
    CATALOGUE
        .iter()
        .filter(|entry| entry.contains(query))
        .copied()
        .collect()
}

/// The catalogue entries matching `query`.
#[procedure(serde)]
pub async fn search(query: String) -> Result<Vec<&'static str>> {
    Ok(hits(&query))
}

/// The same answer at a URL that is written down rather than minted.
#[route(POST "/demo/search")]
async fn search_endpoint(cx: &Cx, body: Body) -> Result<Json<Vec<&'static str>>> {
    let (query,): (String,) = serde_args(cx, body).await?;
    Ok(Json(hits(&query)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_answers_every_entry_containing_it() {
        assert_eq!(hits("ru"), ["rust", "ruby", "trust", "crust"]);
        assert_eq!(hits("st"), ["rust", "trust", "crust"]);
    }

    #[test]
    fn an_empty_query_answers_nothing() {
        // The island asks as the user types, so the first keystroke must not
        // answer with the whole catalogue.
        assert!(hits("").is_empty());
    }

    #[test]
    fn a_query_nothing_matches_answers_nothing() {
        assert!(hits("qqq").is_empty());
    }
}
