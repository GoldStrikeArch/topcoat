//! A Topcoat app whose interactive parts are a `#![no_std]` Rust crate
//! compiled to plain JavaScript by a rustc codegen backend.
//!
//! The server renders the page and serves the compiled program; the browser
//! loads it as an ordinary script and calls its exported functions by name.

mod base;
mod dom;
mod search;
mod ticks;

#[path = "../island/bench.rs"]
mod bench_island;

#[path = "../island/counter.rs"]
mod counter_island;

#[path = "../island/dashboard.rs"]
mod dashboard_island;

#[path = "../island/life.rs"]
mod life_island;

#[path = "../island/mines.rs"]
mod mines_island;

#[path = "../island/nested.rs"]
mod nested_island;

#[path = "../island/panel.rs"]
mod panel_component;

#[path = "../island/sand.rs"]
mod sand_island;

#[path = "../island/search.rs"]
mod search_island;

use base::at;
use bench_island::bench;
use counter_island::counter;
use dashboard_island::dashboard;
use life_island::life;
use mines_island::mines;
use nested_island::nested;
use panel_component::panel;
use sand_island::sand;
use search_island::search as search_island_view;
use topcoat::{
    Result,
    asset::{AssetBundle, AssetConfig, RouterBuilderAssetExt, asset},
    context::Cx,
    router::{
        HeaderValue, IntoResponse, Response, Router, RouterBuilderDiscoverExt, header, page, route,
    },
    view::{component, view},
};

/// Columns of the life board. The server renders the cells, the compiled
/// client crate steps them.
const WIDTH: i32 = 40;

/// Rows of the life board.
const HEIGHT: i32 = 25;

#[tokio::main]
async fn main() {
    // Under a base URL the assets render as hosted rather than served: the
    // snapshot script copies `target/assets` into place itself, and a served
    // route would write unprefixed URLs into the pages.
    let assets = AssetBundle::load().unwrap();
    let builder = Router::builder().discover();
    let builder = if base::BASE_URL.is_empty() {
        builder.assets(assets)
    } else {
        builder.assets(AssetConfig::hosted_at(at("/_topcoat/assets"), assets))
    };

    topcoat::start(builder.build()).await.unwrap();
}

/// A build artifact served from a stable URL.
///
/// The compiled program, its source map, and the shim are served as plain
/// routes rather than as [`asset!`] files, because the map is found through
/// the program's relative `sourceMappingURL` comment and a content hashed name
/// would break that link. Stable URLs need `Cache-Control: no-store` to stay
/// honest across rebuilds.
struct Artifact {
    content_type: &'static str,
    body: &'static str,
}

impl Artifact {
    /// A JavaScript file.
    fn script(body: &'static str) -> Self {
        Self {
            content_type: "text/javascript; charset=utf-8",
            body,
        }
    }

    /// A source map, which is JSON.
    fn source_map(body: &'static str) -> Self {
        Self {
            content_type: "application/json; charset=utf-8",
            body,
        }
    }
}

impl IntoResponse for Artifact {
    fn into_response(self, cx: &Cx) -> Result<Response> {
        (
            [
                (
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(self.content_type),
                ),
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
            ],
            self.body,
        )
            .into_response(cx)
    }
}

/// The runtime shim, copied verbatim from the compiler. It defines the `__rt`
/// object every compiled program calls into.
#[route(GET "/demo/shim.js")]
async fn shim_js() -> Result<Artifact> {
    Ok(Artifact::script(include_str!(concat!(
        env!("OUT_DIR"),
        "/shim.js"
    ))))
}

/// The client crate, compiled to JavaScript during `cargo build`.
#[route(GET "/demo/app.js")]
async fn app_js() -> Result<Artifact> {
    Ok(Artifact::script(include_str!(concat!(
        env!("OUT_DIR"),
        "/app.js"
    ))))
}

/// The source map beside it, carrying the Rust sources inline.
#[route(GET "/demo/app.js.map")]
async fn app_js_map() -> Result<Artifact> {
    Ok(Artifact::source_map(include_str!(concat!(
        env!("OUT_DIR"),
        "/app.js.map"
    ))))
}

/// The hand written wiring between the page and the compiled program.
#[route(GET "/demo/glue.js")]
async fn glue_js() -> Result<Artifact> {
    Ok(Artifact::script(include_str!("glue.js")))
}

#[page("/")]
async fn home() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Rust, compiled to JavaScript"</title>
                <link rel="stylesheet" href=(asset!("./demo.css"))>
                topcoat::dev::script()
                topcoat::runtime::script()
            </head>
            <body>
                <header class="intro">
                    <h1>"Rust, compiled to JavaScript"</h1>
                    <p>
                        "Three panels of "
                        <code>"#![no_std]"</code>
                        " Rust compiled to plain "
                        "JavaScript by a rustc codegen backend: no WASM, no bundler. The browser "
                        "loads "
                        <a href=(at("/demo/app.js"))>"/demo/app.js"</a>
                        " as an ordinary "
                        "script and calls exported functions by name."
                    </p>
                    <p>
                        "The same compiler also drives the DOM: "
                        <a href=(at("/island"))>"the counter island"</a>
                        " is server rendered markup that compiled Rust takes over in the browser, "
                        "and "
                        <a href=(at("/island/nested"))>"the nesting island"</a>
                        " does it through two component boundaries, and "
                        <a href=(at("/island/search"))>"the searching island"</a>
                        " asks the server for its contents."
                    </p>
                    <p>
                        "What that compiler is for is on "
                        <a href=(at("/island/showcase"))>"the showcase"</a>
                        ": a board that plays itself, sand "
                        "that falls under the pointer, and a minefield that floods open from a "
                        "click. Three programs, none of which the expression language that came "
                        "before could have said at all."
                    </p>
                </header>

                explainer()

                life_panel()
                fmt_panel()
                panic_panel()

                <script src=(at("/demo/shim.js"))></script>
                <script src=(at("/demo/app.js"))></script>
                <script src=(at("/demo/glue.js"))></script>
            </body>
        </html>
    }
}

/// The island page: markup the server rendered, taken over by a compiled Rust
/// crate that never re-renders it.
#[page("/island")]
async fn island_page() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"An island, compiled from Rust"</title>
                <link rel="stylesheet" href=(asset!("./demo.css"))>
                topcoat::dev::script()
                dom::script(eager: "counter")
                topcoat::runtime::script()
            </head>
            <body>
                <header class="intro">
                    <h1>"An island, compiled from Rust"</h1>
                    <p>
                        "The counter below was rendered by the server and is now driven by "
                        <a href=(at("/demo/chunks/counter.js"))>"/demo/chunks/counter.js"</a>
                        ", the same view compiled a second time by the rustc backend. The browser "
                        "claims the markup that is already there instead of building it again: "
                        "view source and the counter is in the HTML, with a "
                        <code>"data-hk"</code>
                        " on every node the client has to find again."
                    </p>
                    <p>
                        <a href=(at("/island/showcase"))>"the showcase"</a>
                        " | "
                        <a href=(at("/"))>"back to the three panels"</a>
                    </p>
                </header>

                <section class="panel">
                    <h2>"Counter"</h2>
                    counter(start: 5.0)
                    <p class="readout">
                        "The signal, the handlers, and the text interpolation are one "
                        <code>"view!"</code>
                        " body in "
                        <code>"demo-app/island/counter.rs"</code>
                        ". The server compiled it to HTML; the backend compiled it to a "
                        "dom-expressions template. No JavaScript was written for either."
                    </p>
                </section>
            </body>
        </html>
    }
}

/// The nesting island's page: the same story as `/island`, one component
/// boundary deeper.
///
/// The island calls a component that calls another component, so the keys the
/// server writes nest two contexts deep and the browser opens the same two to
/// claim those nodes. Beside it, a component that takes child content, which the
/// server renders aside before entering the component so the content is numbered
/// where it was written rather than where it lands.
#[page("/island/nested")]
async fn nested_island_page() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"A nesting island, compiled from Rust"</title>
                <link rel="stylesheet" href=(asset!("./demo.css"))>
                topcoat::dev::script()
                dom::script()
                topcoat::runtime::script()
            </head>
            <body>
                <header class="intro">
                    <h1>"A nesting island, compiled from Rust"</h1>
                    <p>
                        "The island below calls a card component, and the card calls a badge. Each "
                        "call opens a hydration context of its own, so the "
                        <code>"data-hk"</code>
                        " the server writes on the card is "
                        <code>"10"</code>
                        " and the one on the badge is "
                        <code>"110"</code>
                        ": a slot inside a slot, not a running count. The browser opens the same "
                        "two contexts and finds the nodes that are already there."
                    </p>
                    <p>
                        <a href=(at("/island"))>"the counter island"</a>
                        " | "
                        <a href=(at("/island/search"))>"the searching island"</a>
                        " | "
                        <a href=(at("/island/showcase"))>"the showcase"</a>
                        " | "
                        <a href=(at("/"))>"back to the three panels"</a>
                    </p>
                </header>

                <section class="panel">
                    <h2>"Nesting"</h2>
                    nested(start: 5.0)
                    <p class="readout">
                        "One "
                        <code>"view!"</code>
                        " body and two "
                        <code>"#[component(client)]"</code>
                        " bodies in "
                        <code>"demo-app/island/nested.rs"</code>
                        ", compiled once by the server and once by the backend. Clicking "
                        <code>"+1"</code>
                        " touches the count and nothing inside the components."
                    </p>
                </section>

                panel(
                    title: "Child content",
                    <p class="readout">
                        "This paragraph is child content: it was built where the call is written "
                        "and rendered into a buffer the component's own view writes back out. That "
                        "is why an eager child is numbered before the component it was passed to."
                    </p>
                )
            </body>
        </html>
    }
}

/// The search island's page: an island that talks to the server.
///
/// The island renders empty, because the server has not been asked anything.
/// Typing into the box settles into one request, the reply is rendered as a
/// list, and the next reply replaces it. The island is hydrated lazily, so its
/// code is not fetched until the box is scrolled into view.
#[page("/island/search")]
async fn search_island_page() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"A searching island, compiled from Rust"</title>
                <link rel="stylesheet" href=(asset!("./demo.css"))>
                topcoat::dev::script()
                dom::script(events: "input click")
                topcoat::runtime::script()
            </head>
            <body>
                <header class="intro">
                    <h1>"A searching island, compiled from Rust"</h1>
                    <p>
                        "The box below asks "
                        <code>"POST /demo/search"</code>
                        " once the typing settles, and renders the reply. The reply is JSON the "
                        "browser has already parsed: the island's crate has no heap, so it reads "
                        "the rows out one at a time instead of decoding them into Rust."
                    </p>
                    <p>
                        <a href=(at("/island"))>"the counter island"</a>
                        " | "
                        <a href=(at("/island/nested"))>"the nesting island"</a>
                        " | "
                        <a href=(at("/island/showcase"))>"the showcase"</a>
                        " | "
                        <a href=(at("/"))>"back to the three panels"</a>
                    </p>
                </header>

                <section class="panel">
                    <h2>"Search"</h2>
                    search_island_view()
                    <p class="readout">
                        "One "
                        <code>"view!"</code>
                        " body in "
                        <code>"demo-app/island/search.rs"</code>
                        ". The two functions it calls are declared twice, once per target: asking "
                        "a server for something is the one thing only the browser half can do."
                    </p>
                </section>
            </body>
        </html>
    }
}

/// The dashboard's page: an island fed by the server rather than by the reader.
///
/// The server renders the opening prices, which is what the browser adopts. From
/// then on the page is driven by `GET /demo/ticks`: each tick writes one price
/// signal, the movers list re-ranks itself around it, and the chart is handed a
/// fresh series.
#[page("/island/dashboard")]
async fn dashboard_island_page() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"A streaming island, compiled from Rust"</title>
                <link rel="stylesheet" href=(asset!("./demo.css"))>
                topcoat::dev::script()
                dom::script()
                topcoat::runtime::script()
            </head>
            <body>
                <header class="intro">
                    <h1>"A streaming island, compiled from Rust"</h1>
                    <p>
                        "Prices arrive on their own over "
                        <code>"GET /demo/ticks"</code>
                        ", a server-sent event stream. The browser subscribes with its own "
                        <code>"EventSource"</code>
                        ", which the island declares rather than borrows: "
                        <code>"#[js_extern]"</code>
                        " names the JavaScript operation and the compiler emits it."
                    </p>
                    <p>
                        <a href=(at("/island"))>"the counter island"</a>
                        " | "
                        <a href=(at("/island/nested"))>"the nesting island"</a>
                        " | "
                        <a href=(at("/island/search"))>"the searching island"</a>
                        " | "
                        <a href=(at("/island/showcase"))>"the showcase"</a>
                        " | "
                        <a href=(at("/"))>"back to the three panels"</a>
                    </p>
                </header>

                <section class="panel">
                    <h2>"Movers"</h2>
                    dashboard()
                    <p class="readout">
                        "Five signals, one per symbol. The list reads all five, so a tick that "
                        "writes one of them re-runs it: that is what re-ranks the rows, and the "
                        "keys are the symbols, so a row that moves keeps the node it had. The "
                        "chart is handed a Rust struct, which crosses as a JavaScript object."
                    </p>
                    <p class="readout">
                        "The chart library here is the contract suite's recorder, which draws "
                        "nothing and writes down what it was asked to do. A real one takes its "
                        "place by pointing the "
                        <code>"topcoat-chart"</code>
                        " entry of the import map at it; the island's declarations already name "
                        "the surface a real one has."
                    </p>
                </section>
            </body>
        </html>
    }
}

/// The showcase: three islands that are programs, and the source of one of them.
///
/// The other island pages each make one point about the machinery. This one is
/// about what the machinery is for. A board plays itself, sand falls under the
/// pointer, and a minefield floods open from a click, and none of the three could
/// be written in the expression language that came before: it had no `match`, no
/// `for`, no structs and no recursion, so anything with a rule in it had to be
/// hand written JavaScript beside the Rust.
///
/// Three islands on one page, which no other page here has. They hydrate one at
/// a time, in the order the loader finds them, and their chunks arrive
/// separately: only Life is eager, so the other two are not fetched until they
/// are scrolled to.
#[page("/island/showcase")]
async fn showcase_page() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Three islands that are programs"</title>
                <link rel="stylesheet" href=(asset!("./demo.css"))>
                topcoat::dev::script()
                // The three events these islands answer, and nothing else: every
                // name costs a document listener for as long as the page has an
                // island left to hydrate, and two of the three wait to be seen.
                dom::script(events: "click input contextmenu", eager: "life")
                topcoat::runtime::script()
            </head>
            <body>
                <header class="intro">
                    <h1>"Three islands that are programs"</h1>
                    <p>
                        "Each panel below is one "
                        <code>"view!"</code>
                        " body compiled twice: to HTML by the "
                        "server, and to a dom-expressions template by the rustc backend. What is "
                        "new here is not the hydration, which "
                        <a href=(at("/island"))>"the counter"</a>
                        " already showed. It is that the thing being compiled is a program -- "
                        "Conway's rule as a "
                        <code>"match"</code>
                        ", a physics pass over a board, a "
                        "recursive flood fill -- and that all of it is Rust the browser runs as "
                        "plain JavaScript."
                    </p>
                    <p>
                        "Only Life is hydrated eagerly. Open the Network panel and scroll: "
                        <code>"sand.js"</code>
                        " and "
                        <code>"mines.js"</code>
                        " arrive as they come into "
                        "view, because an island the reader never reaches costs nothing."
                    </p>
                    <p>
                        <a href=(at("/island"))>"the counter island"</a>
                        " | "
                        <a href=(at("/island/nested"))>"the nesting island"</a>
                        " | "
                        <a href=(at("/island/search"))>"the searching island"</a>
                        " | "
                        <a href=(at("/island/dashboard"))>"the streaming island"</a>
                        " | "
                        <a href=(at("/"))>"back to the three panels"</a>
                    </p>
                </header>

                <section class="panel">
                    <h2>"Life"</h2>
                    life()
                    <p class="readout">
                        "The board is 640 bytes the page owns and the island borrows: "
                        <code>"__rt.board"</code>
                        " hands back the "
                        <code>"{ buf, off, len }"</code>
                        " record a "
                        <code>"&mut [u8]"</code>
                        " is, and the compiled step indexes it in place. Two "
                        "subscriptions keep it cheap -- the driver reads the clock and is one "
                        "number of text, the grid reads the generation and rebuilds only when the "
                        "board really moved."
                    </p>
                </section>

                <section class="panel">
                    <h2>"Falling sand (Click to use)"</h2>
                    sand()
                    <p class="readout">
                        "Every pixel here is a "
                        <code>"fillRect"</code>
                        " the compiled Rust asked "
                        "for, through eight "
                        <code>"#[js_extern]"</code>
                        " declarations and no "
                        "drawing loop. A frame paints the cells whose value changed and no others, "
                        "so a settled pile costs nothing; turning a "
                        <code>"clientX"</code>
                        " into a "
                        "cell is Rust too, subtracting the canvas's own rectangle."
                    </p>
                </section>

                <section class="panel">
                    <h2>"Minesweeper"</h2>
                    mines(seed: minefield_seed())
                    <p class="readout">
                        "The first click is always safe, because nothing is placed until it "
                        "happens: the mines are dealt from the seed the server rendered into "
                        <code>"data-ts"</code>
                        ", skipping the clicked cell and its eight "
                        "neighbours. Opening a blank cell runs a recursive flood fill, and a right "
                        "click flags rather than opening because the handler calls "
                        <code>"prevent_default()"</code>
                        "."
                    </p>
                </section>

                <section class="panel">
                    <h2>"The same island, twice"</h2>
                    <p class="lead">
                        "The two panes below are the Life island as written and the chunk the "
                        "backend compiled it to, both read at build time with "
                        <code>"include_str!"</code>
                        ". Nothing here is interactive, which is the "
                        "point: it is a server rendered "
                        <code>"<details>"</code>
                        " with two "
                        "strings in it, and the page carries no code for it at all."
                    </p>
                    source_viewer(
                        title: "island/life.rs, beside the JavaScript it compiles to",
                        rust: include_str!("../island/life.rs"),
                        js: include_str!(concat!(env!("OUT_DIR"), "/chunks/life.js")),
                    )
                </section>
            </body>
        </html>
    }
}

/// The js-framework-benchmark page: the benchmark's own document, with the app
/// in it as one island.
///
/// Nothing on this page is demo-app's. There is no stylesheet of ours, no
/// navigation and no explanation, because the page is a contract: the harness
/// serves `/css/currentStyle.css` from its own root, presses six buttons by id,
/// and reads the rows out of `tbody`. Everything the page carries beyond that is
/// bytes the harness weighs against every other entry.
///
/// The stylesheet is the harness's, served from the server root whatever
/// directory the entry is unpacked into. Every other URL on this page moves
/// with the base the snapshot is built for; this one must not pick up a
/// prefix, and `bench-pack.mjs` asserts as much. The one build that prefixes
/// it is a standalone snapshot, which has no harness to serve it and carries
/// its own copy (see [`base::BENCH_STANDALONE`]).
#[page("/bench")]
async fn bench_page() -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <title>"Topcoat-island-non-keyed"</title>
                <link href=(bench_style()) rel="stylesheet">
                // Eager, because an island that waits to be scrolled to is an
                // island the harness would find unhydrated. The page carries no
                // `topcoat::runtime::script()`: an island page's markup holds no
                // runtime-expression bindings, so that script would be 5.9 KB of
                // provably unreachable code inside the benchmark's measured
                // weight -- the one page in this app where every byte is scored.
                dom::script(events: "click", eager: "bench")
            </head>
            <body>
                <div id="main">bench()</div>
            </body>
        </html>
    }
}

/// The benchmark page's stylesheet URL.
///
/// The harness's own path by default; in a standalone snapshot, the same path
/// under the base, where `scripts/snapshot.mjs` lays a copy out.
fn bench_style() -> String {
    if base::BENCH_STANDALONE {
        at("/css/currentStyle.css")
    } else {
        "/css/currentStyle.css".to_string()
    }
}

/// Where the minefield's mines come from, minted per request.
///
/// A different number is a different field, so this is the wall clock rather
/// than a constant. Whole seconds, because the seed is written into the island's
/// `data-ts` by the server's float printer and read back by the browser's, and an
/// integer is what those two agree on exactly. The server renders an untouched
/// field either way: nothing is placed until the first reveal, so the seed has to
/// mean nothing to the half that cannot use it.
fn minefield_seed() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1.0, |since| since.as_secs() as f64)
}

/// A file the reader can open beside the one compiled from it.
///
/// Two `<pre>` panes inside a collapsed `<details>`, and the whole of it is
/// server rendered: `title`, `rust` and `js` are strings, and the view's default
/// escaping is what makes a `<` in either of them text rather than markup.
#[component]
async fn source_viewer(title: &str, rust: &str, js: &str) -> Result {
    view! {
        <details class="viewer">
            <summary>(title)</summary>

            <div class="viewer-panes">
                <div class="viewer-pane">
                    <h3>"Rust"</h3>
                    <pre><code>(rust)</code></pre>
                </div>

                <div class="viewer-pane">
                    <h3>"JavaScript"</h3>
                    <pre><code>(js)</code></pre>
                </div>
            </div>
        </details>
    }
}

/// The tour of the compiled output, hidden behind a Topcoat signal so the page
/// shows both ways of getting Rust into a browser at once.
#[component]
async fn explainer() -> Result {
    view! {
        <section class="explainer">
            signal explain = false;

            <button class="toggle" @click=$(|_e| explain.toggle())>
                $(if explain.get() {
                    "hide the tour"
                } else {
                    "what am I looking at?"
                })
            </button>

            <p class="caption">
                "This toggle is Topcoat's own expression runtime: a signal, a few hundred bytes "
                "of generated script. The panels below are the other approach, a whole Rust crate "
                "compiled ahead of time. Both coexist on one page and neither knows about the "
                "other."
            </p>

            <div class="tour" :hidden=$(!explain.get())>
                <h2>"What to read in /demo/app.js"</h2>

                <ul>
                    <li>
                        <code>"function life_step"</code>
                        ": slot indexing, "
                        <code>"cur.buf[cur.off + index]"</code>
                        ", and the "
                        <code>"| 0"</code>
                        " and "
                        <code>">>> 0"</code>
                        " masks that keep Rust's integer widths. It "
                        "really is just JavaScript."
                    </li>
                    <li>
                        <code>"living_neighbours"</code>
                        ": the neighbour offsets as a slice "
                        "literal, "
                        <code>"{ buf: [[-1, -1], ...], off: 0, len: 8 }"</code>
                        ", walked by "
                        <code>"core"</code>
                        "'s own slice iterator."
                    </li>
                    <li>
                        "The panic locations in "
                        <code>"panic_demo"</code>
                        ", spelled "
                        <code>
                            "{ filename: \"demo-app/client/fail.rs\", line: ..., col: ... }"
                        </code>
                        ". They are arguments, filled in at the call site: that is what "
                        <code>"#[track_caller]"</code>
                        " compiles to."
                    </li>
                    <li>
                        <code>"app$fmt$impl_0_write_str"</code>
                        ", commented as "
                        <code>"<fmt::Emit as core::fmt::Write>::write_str"</code>
                        ", calling "
                        <code>"__rt.js_emit(piece)"</code>
                        ". That one call is the entire foreign "
                        "function interface of the formatting panel."
                    </li>
                    <li>
                        "The subtree under "
                        <code>"fmt_demo"</code>
                        ": the honest counterweight, because "
                        <code>"core::fmt"</code>
                        " is deep "
                        "and all of it had to be compiled."
                    </li>
                </ul>

                <p>
                    "The exported names are real globals. Open the console and try "
                    <code>
                        "life_population({buf: [true, true, false], off: 0, len: 3})"
                    </code>
                    "."
                </p>
            </div>
        </section>
    }
}

#[component]
async fn life_panel() -> Result {
    view! {
        <section id="life" class="panel">
            <h2>"Life"</h2>

            <p class="lead">
                "The boards are JavaScript arrays. Rust receives them as "
                <code>"&[bool]"</code>
                " and "
                <code>"&mut [bool]"</code>
                ", which the backend represents as "
                <code>"{ buf, off, len }"</code>
                " over the array itself. Nothing is copied and "
                "nothing is serialized: the compiled code indexes the same array the page holds."
            </p>

            <div id="life-grid" class="grid">
                let cells = WIDTH * HEIGHT;

                for index in 0..cells {
                    <div class="cell" data-i=(index)></div>
                }
            </div>

            <div class="controls">
                <button id="life-play" type="button">"pause"</button>
                <button id="life-step" type="button">"step"</button>
                <button id="life-random" type="button">"randomize"</button>
                <button id="life-glider" type="button">"glider"</button>
                <button id="life-clear" type="button">"clear"</button>
            </div>

            <div class="controls">
                <label for="life-rate">"speed"</label>
                <input id="life-rate" type="range" min="1" max="60" value="10">

                <label for="life-density">"density"</label>
                <input id="life-density" type="range" min="5" max="60" value="30">
            </div>

            <p class="readout">
                "generation "
                <span id="life-gen">"0"</span>
                ", population "
                <span id="life-pop">"0"</span>
                ". Click a cell to toggle it."
            </p>
        </section>
    }
}

#[component]
async fn fmt_panel() -> Result {
    view! {
        <section id="fmt" class="panel">
            <h2>"Formatting"</h2>

            <p class="lead">
                "Every keystroke calls compiled "
                <code>"core::fmt"</code>
                ". Width and precision "
                "are runtime values, but fill and alignment are compile time: "
                <code>"core::fmt"</code>
                " bakes them into the format string, so the Rust side is "
                "a matrix of "
                <code>"write!"</code>
                " arms. Floats are missing on purpose: "
                <code>"Display for f64"</code>
                " is not implemented by the backend yet."
            </p>

            <form class="fmt-form">
                <div class="field">
                    <label for="fmt-kind">"value"</label>
                    <select id="fmt-kind">
                        <option value="0">"integer"</option>
                        <option value="1">"text"</option>
                        <option value="2">"char"</option>
                        <option value="3">"custom Display (Temperature)"</option>
                        <option value="4">"radix"</option>
                    </select>
                </div>

                <div class="field">
                    <label for="fmt-number">"number"</label>
                    <input id="fmt-number" type="text" value="42">
                </div>

                <div class="field">
                    <label for="fmt-text">"text"</label>
                    <input id="fmt-text" type="text" value="hello wörld">
                </div>

                <div class="field">
                    <label for="fmt-width">"width"</label>
                    <input id="fmt-width" type="number" min="0" max="40" value="0">
                </div>

                <div class="field">
                    <label for="fmt-precision">"precision"</label>
                    <input id="fmt-precision" type="number" min="0" max="40" value="0">
                    <label class="check">
                        <input id="fmt-precision-on" type="checkbox">
                        "use it"
                    </label>
                </div>

                <div class="field">
                    <label for="fmt-fill">"fill"</label>
                    <select id="fmt-fill">
                        <option value="0">"space"</option>
                        <option value="1">"zero"</option>
                        <option value="2">"star"</option>
                        <option value="3">"dot"</option>
                    </select>
                </div>

                <div class="field">
                    <label for="fmt-align">"align"</label>
                    <select id="fmt-align">
                        <option value="0">"left"</option>
                        <option value="1">"center"</option>
                        <option value="2">"right"</option>
                    </select>
                </div>

                <div class="field">
                    <label for="fmt-radix">"radix"</label>
                    <select id="fmt-radix">
                        <option value="0">"hex"</option>
                        <option value="1">"HEX"</option>
                        <option value="2">"binary"</option>
                        <option value="3">"octal"</option>
                    </select>
                </div>

                <div class="field">
                    <label class="check">
                        <input id="fmt-alternate" type="checkbox">
                        "alternate (#)"
                    </label>
                    <label class="check">
                        <input id="fmt-sign" type="checkbox">
                        "sign (+)"
                    </label>
                </div>
            </form>

            <pre id="fmt-out"></pre>

            <p class="readout">
                <span id="fmt-pieces"></span>
                <code id="fmt-call"></code>
            </p>
        </section>
    }
}

#[component]
async fn panic_panel() -> Result {
    view! {
        <section id="panic" class="panel">
            <h2>"Panics"</h2>

            <p class="lead">
                "The location below is a Rust file, line, and column, read from "
                <code>"PanicInfo"</code>
                " inside the compiled crate's "
                <code>"#[panic_handler]"</code>
                ". The two unwrap choices report different lines because of a single "
                <code>"#[track_caller]"</code>
                " attribute."
            </p>

            <div class="controls">
                <select id="panic-choice">
                    <option value="0">"return normally"</option>
                    <option value="1">"unwrap None (plain fn)"</option>
                    <option value="2">"unwrap None (#[track_caller] fn)"</option>
                    <option value="3">"index out of bounds"</option>
                    <option value="4">"divide by zero"</option>
                    <option value="5">"explicit panic!"</option>
                </select>

                <button id="panic-run" type="button">"run it"</button>
            </div>

            <div id="panic-result">
                <div id="panic-message"></div>
                <div id="panic-location"></div>
            </div>

            <h3>"Stopping in the Rust source"</h3>

            <ol class="recipe">
                <li>"Open DevTools and go to the Sources panel."</li>
                <li>
                    "Find "
                    <code>"demo-app/client/fail.rs"</code>
                    ". The source map embeds the Rust text, so it is really there."
                </li>
                <li>"Turn on \"Pause on caught exceptions\"."</li>
                <li>
                    "Run a panicking choice. The debugger stops on the "
                    <code>"unwrap()"</code>
                    " line in Rust, and the call stack names "
                    <code>"panic_demo"</code>
                    " and "
                    <code>"unwrap_here"</code>
                    ". The compiler's own glue is hidden, because the map lists it in its "
                    <code>"ignoreList"</code>
                    "."
                </li>
            </ol>

            <p class="note">
                "The page keeps working after a panic. State is owned by JavaScript and the "
                "exported functions are pure, so a panic is an exception, not corruption."
            </p>
        </section>
    }
}
