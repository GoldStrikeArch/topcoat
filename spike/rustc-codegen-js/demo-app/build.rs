fn main() {
    // Where the pages believe the site root is. Empty in a normal build; a
    // static deployment served under a subpath sets it (see `src/base.rs`),
    // and the import map written here carries the same prefix the pages do.
    println!("cargo:rerun-if-env-changed=TOPCOAT_BASE_URL");
    let base = std::env::var("TOPCOAT_BASE_URL").unwrap_or_default();

    // The three panels: a plain script the page loads with a `<script>` tag.
    jsc_build::BuildConfig::new("client/app.rs")
        .render()
        .unwrap();

    // The islands: an ES module, because the dom-expressions runtime it drives
    // the DOM through is imported rather than global. Its views go through the
    // view macros, so the crate is expanded before the backend sees it, and
    // that expansion is where `topcoat_client` selects the client half of every
    // island.
    //
    // One chunk per island, plus a shared chunk for what more than one of them
    // reaches. A page then downloads the islands it has, and the loader can wait
    // until an island is about to be seen before fetching it at all. The import
    // map is written beside the chunks and served into the page, so an island
    // added here needs no change to the page that carries it.
    jsc_build::BuildConfig::new("client-island/island.rs")
        .output_name("islands.js")
        .shim(false)
        .views(true)
        .cfg("topcoat_client")
        .crate_arg("js-modules=esm")
        .crate_arg("js-shim-module=topcoat-island-rt")
        // Every `__rt.<name>` the emitted chunks reach for is checked against
        // this module's exports before the build succeeds. Without it a helper
        // the module does not supply is a `TypeError` the first time that line
        // runs in a browser, which is how the dashboard found `f2i` and
        // `str_bytes` in wave 5.
        .shim_module_source("src/island-rt.mjs")
        .chunks(
            jsc_build::Chunking::new()
                .island("counter")
                .island("nested")
                .island("search")
                .island("dashboard")
                .island("life")
                .island("sand")
                .island("mines")
                .island("bench")
                .url_base(format!("{base}/demo/chunks/"))
                .import("topcoat-dom", format!("{base}/demo/topcoat-dom.js"))
                .import("topcoat-island-rt", format!("{base}/demo/island-rt.js"))
                // Which module the dashboard's `#[js_extern]` chart declarations
                // reach is decided here and nowhere else. A real chart library
                // swaps in by pointing this entry somewhere else.
                .import("topcoat-chart", format!("{base}/demo/chart-lib.js")),
        )
        .render()
        .unwrap();
}
