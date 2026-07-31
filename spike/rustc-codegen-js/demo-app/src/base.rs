//! The URL prefix a static deployment serves the app under.

/// The prefix every URL rendered into a page carries.
///
/// Empty in a normal build, so pages come out exactly as the routes are
/// registered. A static snapshot served under a subpath (a GitHub Pages
/// project site lives at `/<repo>/`) is built with `TOPCOAT_BASE_URL=/<repo>`
/// set, and every href, script src, import map entry, and asset URL picks the
/// prefix up. The routes themselves stay unprefixed: the snapshot script
/// fetches them from a local server and lays the files out under the prefix.
///
/// `build.rs` reads the same variable for the import map it writes, so the
/// two halves cannot disagree within one build.
pub const BASE_URL: &str = match option_env!("TOPCOAT_BASE_URL") {
    Some(base) => base,
    None => "",
};

/// Whether the benchmark page is deployed with no harness in front of it.
///
/// The `/bench` page is a js-framework-benchmark entry, and the harness serves
/// the entry's stylesheet from the server root, so the page normally writes
/// that URL out without the prefix. A standalone snapshot (the GitHub Pages
/// site) has no harness serving it: a build with `TOPCOAT_BENCH_STANDALONE`
/// set links a copy of the stylesheet under [`BASE_URL`] instead, and
/// `scripts/snapshot.mjs` lays that copy out.
pub const BENCH_STANDALONE: bool = option_env!("TOPCOAT_BENCH_STANDALONE").is_some();

/// A path under [`BASE_URL`], for rendering into a page.
pub fn at(path: &str) -> String {
    let mut url = String::with_capacity(BASE_URL.len() + path.len());
    url.push_str(BASE_URL);
    url.push_str(path);
    url
}
