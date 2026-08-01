//! Delivery for islands: what a page has to carry for the browser to find its
//! islands, and the routes the pieces are served from.
//!
//! Three things reach the page, in the order they have to run:
//!
//! 1. An import map, so the specifiers the loader and the compiled islands import by resolve to
//!    served files. The build writes it, naming one chunk per island.
//! 2. The hydration bootstrap, a classic script that installs the `_$HY` global the runtime reads
//!    and queues the events fired before an island is ready. It has to be in place before any event
//!    can be missed, so it is inline and not deferred.
//! 3. The loader, a module that finds the islands in the document and hydrates each one. Modules
//!    are deferred, so it runs after the document is parsed and after the two above.
//!
//! # One file per island
//!
//! Each island is compiled into a chunk of its own, plus a shared chunk holding
//! what two or more of them both reach. A page downloads the islands it has and
//! not the ones it does not, which is what makes the loader's laziness worth
//! having: a chunk is fetched when its island is about to be hydrated, so an
//! island below the fold costs nothing until it is scrolled to.
//!
//! # The runtime the islands import
//!
//! The compiled code imports its DOM operations from `topcoat-dom`, which the
//! import map points at [`runtime_js`]: the single vendored bundle from
//! `runtime/dom/dist`. It is one file with no imports left in it, so the page
//! fetches one module and gets both the DOM operations and the reactive core
//! they run on, in one graph.

use topcoat::{
    Result,
    context::Cx,
    router::{
        HeaderValue, IntoResponse, Response, error::RouterErrorExt, header, path_param, route,
    },
    view::{Unescaped, component, view},
};

/// The `_$HY` bootstrap, from `contract/fixtures/hydration-script.js` with the
/// event list filled in.
///
/// Extracted from the pinned runtime's own `generateHydrationScript`, so what
/// the page installs is what the runtime expects to find. A captured event is
/// replayed against the island that owns it once that island has hydrated, so
/// the list is the events the page's islands answer. Every name costs a document
/// listener for the length of the pre-hydration window, and a lazily hydrated
/// island makes that window as long as it takes the reader to scroll, which is
/// the reason the list is not the runtime's whole delegated set.
const BOOTSTRAP_OPEN: &str = r#"<script>window._$HY||(e=>{let t=e=>e&&e.hasAttribute&&(e.hasAttribute("data-hk")?e:t(e.host&&e.host.nodeType?e.host:e.parentNode));[""#;

/// The rest of it, from the closing quote of the event list on.
const BOOTSTRAP_CLOSE: &str = r#""].forEach((o=>document.addEventListener(o,(o=>{if(!e.events)return;let s=t(o.composedPath&&o.composedPath()[0]||o.target);s&&!e.completed.has(s)&&e.events.push([s,o])}))))})(_$HY={events:[],completed:new WeakSet,r:{},fe(){}});</script>"#;

/// The bootstrap capturing `events`, which are separated by spaces.
///
/// The names go inside the quotes the template already supplies, which is how
/// the runtime's own generator writes them.
fn bootstrap(events: &str) -> String {
    if events == DEFAULT_EVENTS {
        return BOOTSTRAP.to_owned();
    }
    let list = events.split_whitespace().collect::<Vec<_>>().join("\", \"");
    format!("{BOOTSTRAP_OPEN}{list}{BOOTSTRAP_CLOSE}")
}

/// What a page captures when it says nothing: the one event the counter and the
/// nesting islands answer.
const DEFAULT_EVENTS: &str = "click";

/// The bootstrap a page carries when it says nothing about its events, spelled
/// out rather than built.
///
/// A page declares the events its own islands answer, so the served bytes differ
/// per page and cannot all be one constant. This is the default page's, written
/// as a literal so that a reader outside this crate can have the bytes without
/// running the program. `the_default_bootstrap_is_the_one_the_builder_makes`
/// pins it against [`bootstrap`], so the two cannot drift.
const BOOTSTRAP: &str = concat!(
    r#"<script>window._$HY||(e=>{let t=e=>e&&e.hasAttribute&&(e.hasAttribute("data-hk")?e:t(e.host&&e.host.nodeType?e.host:e.parentNode));[""#,
    "click",
    r#""].forEach((o=>document.addEventListener(o,(o=>{if(!e.events)return;let s=t(o.composedPath&&o.composedPath()[0]||o.target);s&&!e.completed.has(s)&&e.events.push([s,o])}))))})(_$HY={events:[],completed:new WeakSet,r:{},fe(){}});</script>"#,
);

/// The import map the build wrote, resolving every island's chunk and the
/// runtime they all import.
///
/// It is generated rather than written here because the chunks are: an island
/// added to `build.rs` appears in this map without anything on the page
/// changing.
const IMPORT_MAP: &str = include_str!(concat!(env!("OUT_DIR"), "/islands.importmap.json"));

/// The module that hydrates the islands in the document.
///
/// One island at a time, because hydration is a single global cursor: the
/// runtime numbers the nodes it claims against one render id at a time and
/// clears it when the call returns. Laziness makes that a queue rather than a
/// loop, since two islands scrolled into view together would otherwise fetch
/// their chunks and hydrate against the same cursor.
const LOADER: &str = r#"import { hydrate } from "topcoat-dom";

// The prefix an island's chunk is imported by. The build writes the other half
// of the name into the page's import map, one entry per island.
const CHUNK = "topcoat-island/";

// The page's own instructions to the loader. `data-tl-eager` names the islands
// to hydrate without waiting, and `data-tl-dev` is written only when the app is
// running under `topcoat dev`, which is what turns the hydration check below on.
const options = document.querySelector("script[data-tl-loader]");
const eager = new Set((options?.dataset.tlEager ?? "").split(" ").filter(Boolean));
const dev = options?.hasAttribute("data-tl-dev") ?? false;

// Hydration is a single global cursor, so islands take it one at a time.
let queue = Promise.resolve();

/// Queues `mount` to be hydrated after every island already queued.
function claim(mount) {
	queue = queue.then(() => start(mount)).catch(error => console.error(error));
	return queue;
}

/// Fetches an island's chunk and hands its server-rendered markup over to it.
async function start(mount) {
	const name = mount.dataset.ti;
	const chunk = await import(CHUNK + name);
	const entry = chunk[`__island_${name}`];
	if (!entry) {
		console.error(`topcoat: the island \`${name}\` has no entry point`);
		return;
	}
	const seeds = JSON.parse(mount.dataset.ts || "[]");
	const key = mount.dataset.tk;
	// Taken before hydrating, because what the check asks is whether these very
	// nodes are the ones the island ended up running on.
	const served = dev ? [...mount.querySelectorAll("[data-hk]")] : [];
	hydrate(() => entry(...seeds), mount, { renderId: `${key}.` });
	mount.dataset.tlHydrated = "";
	if (dev) reportLoss(name, key, mount, served);
}

/// Warns when an island rebuilt the server's markup instead of taking it over.
///
/// A key the client asks for and the server never wrote is not an error to the
/// runtime: it builds the node itself, the page ends up looking right, and the
/// whole point of hydrating is silently gone. What it cannot hide is that the
/// server's own node was replaced, so that is what this looks for.
function reportLoss(name, key, mount, served) {
	const lost = served.find(node => !mount.contains(node));
	if (!lost) return;
	console.warn(
		`topcoat: the island \`${name}\` rebuilt its markup instead of hydrating it. `
			+ `The server's node for key \`${lost.dataset.hk}\` was replaced, so the client `
			+ `asked for a key the server never wrote under \`${key}.\`.`,
	);
}

const mounts = [...document.querySelectorAll("topcoat-island[data-ti]")];
const waiting = [];
for (const mount of mounts) {
	if (mount.dataset.tl === "eager" || eager.has(mount.dataset.ti)) {
		claim(mount);
	} else {
		waiting.push(mount);
	}
}

// An island is hydrated when it is about to be seen. Without an observer to ask,
// every island is hydrated at once, which is the behaviour laziness improves on
// rather than the one it replaces.
if (waiting.length && typeof IntersectionObserver === "function") {
	const observer = new IntersectionObserver(entries => {
		for (const entry of entries) {
			if (!entry.isIntersecting) continue;
			observer.unobserve(entry.target);
			claim(entry.target);
		}
	});
	for (const mount of waiting) observer.observe(mount);
} else {
	for (const mount of waiting) claim(mount);
}
"#;

/// Everything a page with islands carries, in the order it has to run.
///
/// Put it in the document head, BEFORE any other module script -- including
/// [`topcoat::runtime::script`]. The import map in here is what resolves the
/// bare `topcoat-dom` specifier, and the specification only honors an import
/// map seen before the first module load. Chrome forgives a late map (it
/// accepts them dynamically since 133); Firefox does not, silently drops the
/// map, and every island then dies on "topcoat-dom was a bare specifier, but
/// was not remapped". The loader is a module and so is deferred, which is
/// what lets it find islands that are written after it.
///
/// `events` names the events to capture before the page's islands are ready,
/// separated by spaces: the ones this page's islands answer, since every name
/// costs a document listener for the length of the pre-hydration window.
///
/// `eager` names the islands to hydrate as soon as the loader runs, separated by
/// spaces. Everything else waits until it is scrolled into view.
#[component]
pub async fn script(#[default(DEFAULT_EVENTS)] events: &str, #[default("")] eager: &str) -> Result {
    // Read by the loader, and written only under `topcoat dev`: the check it
    // turns on costs a query per island and reports a mistake nobody makes twice
    // in a build that is already serving.
    let dev = std::env::var("TOPCOAT_DEV_URL").is_ok().then_some("");
    let eager = (!eager.is_empty()).then_some(eager);

    view! {
        <script type="importmap">(IMPORT_MAP)</script>
        (Unescaped::new_unchecked(bootstrap(events)))
        <script
            type="module"
            src=(crate::base::at("/demo/island-loader.js"))
            data-tl-loader=""
            data-tl-eager=(eager)
            data-tl-dev=(dev)
        ></script>
    }
}

/// A build artifact served from a stable URL.
///
/// The islands, the runtime and the maps find each other by URL, so these are
/// plain routes rather than content hashed [`asset!`](topcoat::asset::asset)
/// files. Stable URLs need `Cache-Control: no-store` to stay honest across
/// rebuilds.
struct Module {
    content_type: &'static str,
    body: &'static str,
}

impl Module {
    /// A JavaScript module.
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

impl IntoResponse for Module {
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

/// The runtime the compiled islands import by the bare specifier `topcoat-dom`.
///
/// The committed artifact from `runtime/dom/dist`, served byte for byte. Its
/// export list is asserted against the pinned ABI by `runtime/dom`'s own check,
/// so what a page runs against is what that check covers.
#[route(GET "/demo/topcoat-dom.js")]
async fn runtime_js() -> Result<Module> {
    Ok(Module::script(include_str!(
        "../../runtime/dom/dist/topcoat-dom.js"
    )))
}

/// The functions the islands borrow from the page.
///
/// A compiled island calls an `extern "C"` function as `__rt.<name>(...)`, and
/// the import map resolves `topcoat-island-rt` to this, so an island's chunk
/// imports the names it uses and nothing else.
#[route(GET "/demo/island-rt.js")]
async fn island_rt_js() -> Result<Module> {
    Ok(Module::script(include_str!("island-rt.mjs")))
}

/// The import map the page embeds, served on its own as well.
///
/// The page needs it inline, because an import map has to be in place before the
/// first module resolves. Serving it too is what lets a check outside the
/// browser resolve a chunk the way the page will.
#[route(GET "/demo/chunks.importmap.json")]
async fn chunks_import_map() -> Result<Module> {
    Ok(Module::source_map(IMPORT_MAP))
}

/// The loader that hydrates the document's islands.
#[route(GET "/demo/island-loader.js")]
async fn island_loader_js() -> Result<Module> {
    Ok(Module::script(LOADER))
}

/// The file name of one compiled chunk, or of the source map beside it.
#[path_param]
struct Chunk(str);

/// The compiled islands, one file per island plus what they share.
///
/// One route rather than one per chunk: the chunks are siblings in a directory
/// of their own, so serving that directory is serving the program. The names are
/// still written down, because each file is compiled into this binary and a name
/// that is not here is not a file.
#[route(GET "/demo/chunks/{chunk}")]
async fn chunk_js(cx: &Cx) -> Result<Module> {
    let module = match path_param::<Chunk>(cx) {
        "counter.js" => {
            Module::script(include_str!(concat!(env!("OUT_DIR"), "/chunks/counter.js")))
        }
        "nested.js" => Module::script(include_str!(concat!(env!("OUT_DIR"), "/chunks/nested.js"))),
        "life.js" => Module::script(include_str!(concat!(env!("OUT_DIR"), "/chunks/life.js"))),
        "sand.js" => Module::script(include_str!(concat!(env!("OUT_DIR"), "/chunks/sand.js"))),
        "mines.js" => Module::script(include_str!(concat!(env!("OUT_DIR"), "/chunks/mines.js"))),
        "bench.js" => Module::script(include_str!(concat!(env!("OUT_DIR"), "/chunks/bench.js"))),
        "shared.js" => Module::script(include_str!(concat!(env!("OUT_DIR"), "/chunks/shared.js"))),
        "counter.js.map" => Module::source_map(include_str!(concat!(
            env!("OUT_DIR"),
            "/chunks/counter.js.map"
        ))),
        "nested.js.map" => Module::source_map(include_str!(concat!(
            env!("OUT_DIR"),
            "/chunks/nested.js.map"
        ))),
        "life.js.map" => Module::source_map(include_str!(concat!(
            env!("OUT_DIR"),
            "/chunks/life.js.map"
        ))),
        "sand.js.map" => Module::source_map(include_str!(concat!(
            env!("OUT_DIR"),
            "/chunks/sand.js.map"
        ))),
        "mines.js.map" => Module::source_map(include_str!(concat!(
            env!("OUT_DIR"),
            "/chunks/mines.js.map"
        ))),
        "bench.js.map" => Module::source_map(include_str!(concat!(
            env!("OUT_DIR"),
            "/chunks/bench.js.map"
        ))),
        "shared.js.map" => Module::source_map(include_str!(concat!(
            env!("OUT_DIR"),
            "/chunks/shared.js.map"
        ))),
        _ => None::<Module>.ok_or_not_found()?,
    };
    Ok(module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bootstrap_installs_the_global_the_runtime_reads() {
        let installed = bootstrap("click");
        assert!(installed.starts_with("<script>window._$HY||"));
        assert!(installed.ends_with("</script>"));
        // The names sit inside the quotes the template already supplies.
        assert!(installed.contains(r#"["click"]"#));
    }

    #[test]
    fn the_default_bootstrap_is_the_one_the_builder_makes() {
        // The literal exists so the bytes can be read without running the
        // program. This is what keeps it the same bytes.
        assert_eq!(BOOTSTRAP, bootstrap("click"));
    }

    #[test]
    fn the_bootstrap_captures_the_events_the_page_asked_for() {
        // The order is the page's, because a page declares the list its islands
        // answer and the runtime registers them in the order it is given. The
        // separator is the one the runtime's own generator writes.
        assert!(bootstrap("input click").contains(r#"["input", "click"]"#));
    }

    #[test]
    fn the_import_map_resolves_the_runtime_and_every_island() {
        // `build.rs` writes the map under the same base URL this crate renders
        // pages under, so the expectation carries the prefix too and the test
        // holds in a `TOPCOAT_BASE_URL` build.
        let base = crate::base::BASE_URL;
        assert!(IMPORT_MAP.contains(&format!(r#""topcoat-dom":"{base}/demo/topcoat-dom.js""#)));
        for island in ["counter", "nested", "life", "sand", "mines", "bench"] {
            assert!(
                IMPORT_MAP.contains(&format!(
                    r#""topcoat-island/{island}":"{base}/demo/chunks/{island}.js""#
                )),
                "{island} is missing from the import map"
            );
        }
        // What two islands both reach is a chunk of its own, imported by the
        // chunks that reach it rather than by the page.
        assert!(IMPORT_MAP.contains(&format!(
            r#""topcoat-island/shared":"{base}/demo/chunks/shared.js""#
        )));
    }

    #[test]
    fn the_runtime_is_the_pinned_bundle_and_needs_no_resolver() {
        let runtime = include_str!("../../runtime/dom/dist/topcoat-dom.js");
        // Everything the emitter calls to make and track state, out of one file.
        for name in ["createSignal", "effect", "insert", "template", "hydrate"] {
            assert!(runtime.contains(name), "{name} is missing from the runtime");
        }
        // A surviving bare import would mean the page needs to resolve it.
        assert!(!runtime.contains("from\"solid-js\""));
        assert!(!runtime.contains("from \"solid-js\""));
    }

    #[test]
    fn the_loader_scopes_each_island_to_its_own_keys() {
        // The render id is the prefix every key of this island starts with, and
        // the runtime keeps a key only if it starts with that prefix. The
        // trailing separator is part of it: on `i1` alone, `i10`'s keys would
        // match too. Past the separator the runtime's own encoding takes over,
        // which is what the server writes as well.
        assert!(LOADER.contains("renderId: `${key}.`"));
        assert!(LOADER.contains("hydrate(() => entry(...seeds), mount"));
    }

    #[test]
    fn the_loader_hands_the_entry_point_result_to_the_runtime() {
        // A compiled view evaluates to a template root the entry point returns,
        // so the runtime receives the claimed node directly.
        assert!(LOADER.contains("hydrate(() => entry(...seeds)"));
        assert!(!LOADER.contains("childNodes"));
    }

    #[test]
    fn the_loader_imports_one_chunk_per_island() {
        // The specifier is built from the island's own name, which is what makes
        // a chunk arrive only for an island the page actually has.
        assert!(LOADER.contains(r#"const CHUNK = "topcoat-island/""#));
        assert!(LOADER.contains("await import(CHUNK + name)"));
    }
}
