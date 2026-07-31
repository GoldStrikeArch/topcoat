//! The dashboard island: a movers list and a chart, both driven by one stream of
//! server-sent ticks.
//!
//! Like the other islands this file is a module of two crates. What is new here
//! is that the browser half talks to JavaScript that nobody in this repository
//! wrote a binding for by hand: five declared operations, spelled with
//! `#[js_extern]`, which the backend lowers to the JavaScript they name.
//!
//! # What is compiled Rust
//!
//! Everything with a decision in it. The five prices are signals; the ordering
//! of the movers list is Rust; the chart's configuration and the series handed
//! to it on every update are Rust values that cross as JavaScript objects
//! because that is what the value model spells a `#[repr(C)]` struct as. No part
//! of the update payload is assembled in JavaScript.
//!
//! # What rides `#[js_extern]`
//!
//! `new EventSource(url)`, `target.addEventListener(kind, handler)` and
//! `document.querySelector(selector)` are browser globals, declared with no
//! module. `new Chart(target, config)` and `chart.update(series)` are the chart
//! library. That is the whole foreign surface, and none of it is hand written
//! JavaScript.
//!
//! # What the host module still has to do
//!
//! A declared interface cannot express a callback: handing JavaScript a function
//! means handing it a compiled Rust closure, which is a different problem with a
//! different owner. So the function the `EventSource` calls is made in
//! [`island-rt`](../src/island-rt.mjs), and once it is JavaScript it may as well
//! be the thing that parses the tick, which is the no-serializer arrangement the
//! search island already uses. The host also holds the two handles that outlive
//! a call, because this crate is `#![no_std]` with no heap and no place to put
//! one.
//!
//! # Why setup is safe where it is
//!
//! An island's setup must be synchronous: hydration's window is exactly one
//! synchronous call stack, and anything resuming after a suspension point finds
//! it closed and silently rebuilds the DOM instead of adopting the server's.
//! Constructing an `EventSource` and registering a listener are both synchronous
//! calls, so both are made on the first run of the list's effect, inside that
//! window. Messages cannot arrive during it: they are delivered in later tasks,
//! by which time hydration has returned. So no mount hook is needed, and none
//! was added.

/// The symbols the feed carries, in slot order.
///
/// A tick names a slot rather than a symbol, so this is the only table and both
/// halves read it.
pub const SYMBOLS: [&str; 5] = ["ACME", "BOLT", "CRUX", "DYAD", "ECHO"];

/// What each symbol costs before the first tick, in cents.
pub const START_CENTS: [i32; SYMBOLS.len()] = [12_450, 8_075, 23_900, 4_310, 15_620];

/// Where the ticks come from.
#[cfg(topcoat_client)]
const FEED: &str = "/demo/ticks";

/// The canvas the chart is drawn on.
#[cfg(topcoat_client)]
const CANVAS: &str = ".dash-chart";

/// The event the feed names its ticks.
#[cfg(topcoat_client)]
const TICK: &str = "tick";

/// The price a slot opens at, as the signal holding it spells it.
///
/// Written as a function rather than a literal because a signal's initialiser is
/// plain Rust on both halves and both have to reach the same number: the server
/// renders this price and the browser has to agree with the markup it adopts.
fn start(slot: usize) -> f64 {
    START_CENTS[slot] as f64
}

/// One row of the movers list.
#[derive(Clone, Copy)]
struct Mover {
    symbol: &'static str,
    price: f64,
    delta: f64,
}

/// Which way a row last moved, as a class.
fn trend(delta: f64) -> &'static str {
    if delta > 0.0 {
        "mover up"
    } else if delta < 0.0 {
        "mover down"
    } else {
        "mover"
    }
}

/// The rows to show, in rank order.
///
/// The server has seen no ticks, so its answer is the opening prices in slot
/// order with nothing having moved. That is exactly what the browser's first run
/// answers too, which is what makes the markup it adopts the markup it would
/// have built.
#[cfg(not(topcoat_client))]
fn movers(prices: [&::topcoat::runtime::Signal<f64>; SYMBOLS.len()]) -> [Mover; SYMBOLS.len()] {
    let mut rows = [Mover {
        symbol: "",
        price: 0.0,
        delta: 0.0,
    }; SYMBOLS.len()];
    for (slot, row) in rows.iter_mut().enumerate() {
        *row = Mover {
            symbol: SYMBOLS[slot],
            price: prices[slot].get(),
            delta: 0.0,
        };
    }
    rows
}

/// The rows to show, in the order the host last ranked them.
///
/// Reading all five prices is what subscribes the list to the feed: the loop
/// this fills runs inside an effect, and an effect subscribes to what it read.
/// So a tick that writes one price re-runs this, which re-ranks the list and
/// pushes the new series at the chart.
///
/// # Why the loop that reads this is not keyed
///
/// A key makes a row that was rendered before contribute the NODE it contributed
/// before, so reordering moves nodes instead of replacing them. The row built on
/// the new pass is discarded, and the text in it with it. That is right for a row
/// whose text does not change, or whose changing parts are reactive holes of the
/// row's own template; it is wrong here, where every tick changes a price and a
/// move. Keyed, this list reorders correctly and shows the numbers it had on the
/// first render for ever, which is measurably worse than rebuilding five short
/// rows.
#[cfg(topcoat_client)]
fn movers(prices: [::view_abi::Sig<f64>; SYMBOLS.len()]) -> Movers {
    let series = Series {
        acme: prices[0].get(),
        bolt: prices[1].get(),
        crux: prices[2].get(),
        dyad: prices[3].get(),
        echo: prices[4].get(),
    };

    // The first run is the island's one synchronous entry point, and everything
    // here is a synchronous call, so this is where the chart and the feed are
    // set up. See the module docs on why that is the safe place for it.
    //
    // A `#[js_extern]` declaration is an ordinary safe fn: the attribute expands
    // the block to marker functions rather than leaving anything foreign behind.
    // Only what the page lends the island is a real `extern "C"` call.
    if unsafe { dash_claim() } {
        let chart = chart_new(query_selector(CANVAS), CONFIG);
        unsafe { dash_hold(chart) };

        let feed = event_source_new(FEED);
        let sink = unsafe { dash_sink(prices[0], prices[1], prices[2], prices[3], prices[4]) };
        add_listener(feed, TICK, sink);
    }

    chart_update(unsafe { dash_chart() }, series);

    Movers { rank: 0, prices }
}

/// The rows of the current ranking, one at a time.
///
/// An iterator rather than a collection for the reason the search island's is:
/// the ranking is the host's and there is nowhere here to copy it to.
#[cfg(topcoat_client)]
struct Movers {
    rank: usize,
    prices: [::view_abi::Sig<f64>; SYMBOLS.len()],
}

#[cfg(topcoat_client)]
impl Iterator for Movers {
    type Item = Mover;

    fn next(&mut self) -> Option<Mover> {
        if self.rank >= SYMBOLS.len() {
            return None;
        }

        let slot = unsafe { dash_slot(self.rank as f64) } as usize;
        self.rank += 1;
        Some(Mover {
            symbol: SYMBOLS[slot],
            // The price comes from the signal rather than from the host: the
            // signal is where it lives, and reading it here is the subscription.
            price: self.prices[slot].get(),
            delta: unsafe { dash_delta(slot as f64) },
        })
    }
}

/// An opaque JavaScript value: an event source, an element, a chart, a function.
///
/// One machine word behind a `repr(transparent)`, for the two reasons
/// `view_abi::Node` gives: a zero sized value is folded away, and an ordinary one
/// field struct is rebuilt field by field, so a JavaScript object passed through
/// one would come back with its field undefined.
#[cfg(topcoat_client)]
#[repr(transparent)]
#[derive(Clone, Copy)]
struct JsValue {
    #[allow(dead_code)]
    handle: u32,
}

/// What the chart is asked for when it is built.
///
/// A `repr(C)` struct crosses as an object keyed by field name, so this is the
/// configuration object the library receives, built here.
#[cfg(topcoat_client)]
#[repr(C)]
#[derive(Clone, Copy)]
struct ChartConfig {
    kind: &'static str,
    span: f64,
}

/// The chart the dashboard draws.
#[cfg(topcoat_client)]
const CONFIG: ChartConfig = ChartConfig {
    kind: "line",
    span: SYMBOLS.len() as f64,
};

/// One reading of every price, which is what an update sends.
///
/// Named fields rather than a list, because the library is handed an object and
/// a reader of the trace should be able to see which symbol each number is.
#[cfg(topcoat_client)]
#[repr(C)]
#[derive(Clone, Copy)]
struct Series {
    acme: f64,
    bolt: f64,
    crux: f64,
    dyad: f64,
    echo: f64,
}

// The browser's own globals. A declaration that names no module is rooted at a
// global, so these are the operations spelled exactly as a page would spell
// them, with nothing imported.
#[cfg(topcoat_client)]
#[::js_extern_macro::js_extern]
unsafe extern "C" {
    /// `new EventSource(url)`.
    #[js(new = "EventSource")]
    fn event_source_new(url: &str) -> JsValue;

    /// `target.addEventListener(kind, handler)`.
    #[js(method = "addEventListener")]
    fn add_listener(target: JsValue, kind: &str, handler: JsValue);

    /// `document.querySelector(selector)`.
    ///
    /// A call whose path is walked from a global: `document` is the root and
    /// `querySelector` is called as a member of it, so it keeps its receiver.
    #[js(call = "document.querySelector")]
    fn query_selector(selector: &str) -> JsValue;

    /// `chart.update(series)`.
    ///
    /// A method acts on its first argument, and it may only do that when its
    /// declaration names no module, so it belongs in this block rather than
    /// beside the constructor it goes with.
    #[js(method = "update")]
    fn chart_update(chart: JsValue, series: Series);
}

// The chart library. Which module this is, is a build-time decision: the import
// map resolves `topcoat-chart`, and swapping in a real chart library is a change
// to that entry rather than to anything here.
#[cfg(topcoat_client)]
#[::js_extern_macro::js_extern(module = "topcoat-chart")]
unsafe extern "C" {
    /// `new Chart(target, config)`, through the library's named export.
    #[js(new = "Chart")]
    fn chart_new(target: JsValue, config: ChartConfig) -> JsValue;
}

// What the page lends the island, for the two things a declared interface cannot
// say: a JavaScript function, and somewhere to keep a handle.
#[cfg(topcoat_client)]
#[allow(improper_ctypes)]
unsafe extern "C" {
    /// True the first time it is asked and false afterwards.
    ///
    /// The list's effect re-runs on every tick and setting up twice would open a
    /// second connection, so the claim is what makes the first run the only one.
    fn dash_claim() -> bool;

    /// Keeps the chart, which has to outlive the call that made it.
    fn dash_hold(chart: JsValue);

    /// The chart that was kept.
    fn dash_chart() -> JsValue;

    /// The function the feed calls, which writes the price of the slot a tick
    /// names.
    fn dash_sink(
        acme: ::view_abi::Sig<f64>,
        bolt: ::view_abi::Sig<f64>,
        crux: ::view_abi::Sig<f64>,
        dyad: ::view_abi::Sig<f64>,
        echo: ::view_abi::Sig<f64>,
    ) -> JsValue;

    /// Which slot sits at `rank` in the current ranking.
    fn dash_slot(rank: f64) -> f64;

    /// How far the symbol in `slot` last moved.
    fn dash_delta(slot: f64) -> f64;
}

/// A live market: prices that arrive on their own, a list that reorders itself,
/// and a chart that follows.
#[::view_dom_macro::island]
pub async fn dashboard() -> ::topcoat::Result {
    view! {
        <div class="island dashboard">
            signal acme = start(0);
            signal bolt = start(1);
            signal crux = start(2);
            signal dyad = start(3);
            signal echo = start(4);

            <canvas class="dash-chart" width="480" height="160"></canvas>

            // NOT keyed, and that is a measurement rather than an omission. See
            // the note above `movers`: this loop's rows change their text on
            // every tick, and a keyed row keeps the node it had, text included.
            <ul class="movers">
                for row in movers([acme, bolt, crux, dyad, echo]) {
                    <li class=(trend(row.delta))>
                        <span class="mover-symbol">(row.symbol)</span>
                        <span class="mover-price">(row.price)</span>
                        <span class="mover-delta">(row.delta)</span>
                    </li>
                }
            </ul>
        </div>
    }
}
