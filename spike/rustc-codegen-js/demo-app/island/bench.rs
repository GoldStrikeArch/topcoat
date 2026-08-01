//! The benchmark island: the whole of the js-framework-benchmark app as one
//! island, so that the harness measures the idiomatic path rather than a
//! hand-written one.
//!
//! Like every island this file is a module of two crates. The server crate reads
//! it as ordinary Topcoat and renders the opening page as HTML with the hydration
//! keys in it; the client crate reads it with `topcoat_client` set and the rustc
//! backend compiles the same view to JavaScript. What is new here is not a
//! technique but a MEASUREMENT: this is the framework's own way of writing a list
//! of rows, put in front of a benchmark that has published numbers for everybody
//! else's.
//!
//! # One signal, and what that costs
//!
//! There is exactly one signal, `version`, and it holds a number nobody reads for
//! its value. Every operation writes the rows into the host's store and then bumps
//! it, and the table's loop reads it, so a bump is what rebuilds the table. That
//! is the honest shape of a reactive `for` here: the loop's iterated expression is
//! what the runtime subscribes to, so ANY write rebuilds EVERY row -- a thousand
//! clones and a couple of thousand mutations for a single selection.
//!
//! A framework that beats this does one of two things: it keys the rows (so a
//! reorder moves nodes), or it puts the changing part of a row behind a hole of
//! the row's own template (so a change patches one text node). Both are available
//! here and neither is used, because what this entry is for is the number the
//! straightforward version gets. The recycling version lives outside demo-app, as
//! the benchmark's second entry.
//!
//! # Where the rows live
//!
//! In the host, as a list of `{ id, label }` objects reached through
//! [`island-rt`](../src/island-rt.mjs). The client crate is `#![no_std]` with no
//! heap and no mutable statics, so a list that survives from one click to the next
//! is not a thing it can declare, and a label is a string that has to be built
//! from three words, which needs the heap it does not have. So the store is
//! JavaScript and every DECISION is Rust: which words a label is made of, which
//! rows a bang lands on, which two rows swap, when the swap is legal at all.
//!
//! # Why the rows are read one at a time
//!
//! [`Rows`] is an iterator over the store rather than a collection: the rows are
//! the host's and there is nowhere in this crate to copy them to. It hands the
//! loop a [`RowView`] per row, which is four borrowed values and no allocation.
//!
//! # Why the handlers are per row
//!
//! The other grid-shaped islands here (Life, Minesweeper) use one handler on the
//! container and `value=(index)` on each cell, which is cheaper. The benchmark's
//! row contract fixes the markup: the label is an `<a>` and so is the delete
//! control, and an `<a>` has no `value` for `event.target.value` to read. So this
//! island uses the other shape that `examples/dom-tests/23_for_row_handlers.rs`
//! pins: a closure per row, capturing the row's index. Two per row, rebuilt on
//! every render, which is part of what is being measured.
//!
//! The index is bound with a `let` inside the loop rather than read as
//! `row.index` inside the handler. A `$( ... )` value is compiled by the server's
//! expression language as well as by the client compiler, and that language
//! captures a free identifier WHOLE: `row.index` would ask it to serialize a
//! `RowView`, which is not one of its types. An `f64` is.

/// The adjectives a label may start with.
///
/// The three tables and the way a word is picked from them are the benchmark's,
/// copied exactly: a different table or a different draw would be a different
/// amount of text in the DOM, which is a thing the benchmark measures.
#[cfg(topcoat_client)]
const ADJECTIVES: [&str; 25] = [
    "pretty",
    "large",
    "big",
    "small",
    "tall",
    "short",
    "long",
    "handsome",
    "plain",
    "quaint",
    "clean",
    "elegant",
    "easy",
    "angry",
    "crazy",
    "helpful",
    "mushy",
    "odd",
    "unsightly",
    "adorable",
    "important",
    "inexpensive",
    "cheap",
    "expensive",
    "fancy",
];

/// The colours. Eleven entries with "brown" twice, which is the benchmark's own
/// table and not a mistake being carried over: the duplicate is what makes brown
/// twice as likely, and every published entry has it.
#[cfg(topcoat_client)]
const COLOURS: [&str; 11] = [
    "red", "yellow", "blue", "green", "pink", "brown", "purple", "brown", "white", "black",
    "orange",
];

/// The nouns a label ends with.
#[cfg(topcoat_client)]
const NOUNS: [&str; 13] = [
    "table",
    "chair",
    "house",
    "bbq",
    "desk",
    "car",
    "pony",
    "cookie",
    "sandwich",
    "burger",
    "pizza",
    "mouse",
    "keyboard",
];

/// How many rows the create and append buttons make.
#[cfg(topcoat_client)]
const BATCH: i32 = 1_000;

/// How many the big create button makes.
#[cfg(topcoat_client)]
const BIG_BATCH: i32 = 10_000;

/// The gap between the rows an update touches: every tenth from the first.
#[cfg(topcoat_client)]
const EVERY: f64 = 10.0;

/// The two rows a swap exchanges, and the length that makes the exchange legal.
///
/// The benchmark's numbers, and the guard is part of them: with 998 rows or fewer
/// the swap does nothing at all rather than swapping something else.
#[cfg(topcoat_client)]
const SWAP_LOW: f64 = 1.0;

#[cfg(topcoat_client)]
const SWAP_HIGH: f64 = 998.0;

/// One row, as the table's loop draws it.
///
/// Four values read out of the host's store. `index` is the row's position rather
/// than its id, because the two handlers act by position and the store is a list.
/// `class` is derived by the store from the one selected id, so a row carries no
/// flag of its own and a selection changes one row's class without touching any
/// other row's data.
#[derive(Clone, Copy)]
struct RowView {
    index: f64,
    id: f64,
    label: &'static str,
    class: &'static str,
}

/// The rows to draw.
///
/// The server's answer is no rows, which is the benchmark's opening page: the
/// table is empty until a button is pressed. That makes the markup the browser
/// adopts the markup it would have built, with nothing computed twice.
#[cfg(not(topcoat_client))]
fn rows(_version: f64) -> Vec<RowView> {
    Vec::new()
}

/// The rows to draw, read straight out of the host's store.
///
/// `version` is read for the subscription and not for the number: reading it is
/// what makes the table rebuild when an operation bumps it, and the store is where
/// the rows come from.
#[cfg(topcoat_client)]
fn rows(_version: f64) -> Rows {
    Rows {
        at: 0.0,
        len: unsafe { bench_len() },
    }
}

/// The rows of the store, one at a time.
///
/// An iterator rather than a collection: the rows are the host's and this crate
/// has no heap to copy them into.
#[cfg(topcoat_client)]
struct Rows {
    at: f64,
    len: f64,
}

#[cfg(topcoat_client)]
impl Iterator for Rows {
    type Item = RowView;

    fn next(&mut self) -> Option<RowView> {
        if self.at >= self.len {
            return None;
        }
        let index = self.at;
        self.at += 1.0;
        Some(RowView {
            index,
            id: unsafe { bench_id(index) },
            label: unsafe { bench_label(index) },
            class: unsafe { bench_class(index) },
        })
    }
}

/// What a handler may ask of the store.
///
/// Methods rather than functions because the view's expression language has no
/// free function calls, and written on `version` because that is the signal every
/// one of them ends by bumping. Life's arrangement, and for its reasons.
///
/// The server's half is every method with an empty body: its handlers are never
/// called, because the server renders markup and the browser's copy of the island
/// is what runs.
#[cfg(topcoat_client)]
trait Control {
    /// Replaces the table with a thousand fresh rows.
    fn run(self);

    /// Replaces it with ten thousand.
    fn runlots(self);

    /// Adds a thousand to whatever is there.
    fn add(self);

    /// Marks every tenth row from the first.
    fn update(self);

    /// Empties the table.
    fn clear(self);

    /// Exchanges the second row with the nine hundred and ninety-ninth.
    fn swaprows(self);

    /// Selects the row at `index`, which is the only selection there is.
    fn select(self, index: f64);

    /// Takes the row at `index` out.
    fn remove(self, index: f64);
}

#[cfg(topcoat_client)]
impl Control for ::view_abi::Sig<f64> {
    fn run(self) {
        unsafe { bench_clear() };
        append(BATCH);
        bump(self);
    }

    fn runlots(self) {
        unsafe { bench_clear() };
        append(BIG_BATCH);
        bump(self);
    }

    fn add(self) {
        append(BATCH);
        bump(self);
    }

    fn update(self) {
        let len = unsafe { bench_len() };
        let mut at = 0.0;
        while at < len {
            unsafe { bench_bang(at) };
            at += EVERY;
        }
        bump(self);
    }

    fn clear(self) {
        unsafe { bench_clear() };
        bump(self);
    }

    fn swaprows(self) {
        // Below this length the two rows are not both there, and the benchmark
        // asks for nothing to happen rather than for something else to swap. The
        // bump is inside the guard for the same reason: a table that did not
        // change should not be rebuilt.
        if unsafe { bench_len() } > SWAP_HIGH {
            unsafe { bench_swap(SWAP_LOW, SWAP_HIGH) };
            bump(self);
        }
    }

    fn select(self, index: f64) {
        unsafe { bench_select(index) };
        bump(self);
    }

    fn remove(self, index: f64) {
        unsafe { bench_remove(index) };
        bump(self);
    }
}

#[cfg(not(topcoat_client))]
trait Control {
    fn run(&self);

    fn runlots(&self);

    fn add(&self);

    fn update(&self);

    fn clear(&self);

    fn swaprows(&self);

    fn select(&self, index: ::topcoat::runtime::F64Surrogate);

    fn remove(&self, index: ::topcoat::runtime::F64Surrogate);
}

#[cfg(not(topcoat_client))]
impl Control for ::topcoat::runtime::SignalSurrogate<f64> {
    fn run(&self) {}

    fn runlots(&self) {}

    fn add(&self) {}

    fn update(&self) {}

    fn clear(&self) {}

    fn swaprows(&self) {}

    fn select(&self, _index: ::topcoat::runtime::F64Surrogate) {}

    fn remove(&self, _index: ::topcoat::runtime::F64Surrogate) {}
}

/// Says that the rows changed, which is what rebuilds the table.
#[cfg(topcoat_client)]
fn bump(version: ::view_abi::Sig<f64>) {
    version.set(version.get() + 1.0);
}

/// Puts `count` fresh rows on the end of the store.
///
/// The three words are picked here and joined there: this crate has no heap to
/// build `"pretty red pony"` in, so the store is handed the parts. Which parts is
/// the decision, and the decision is Rust.
#[cfg(topcoat_client)]
fn append(count: i32) {
    let mut made = 0;
    while made < count {
        let adjective = ADJECTIVES[random(ADJECTIVES.len())];
        let colour = COLOURS[random(COLOURS.len())];
        let noun = NOUNS[random(NOUNS.len())];
        unsafe { bench_push(adjective, colour, noun) };
        made += 1;
    }
}

/// A number below `max`, drawn the way the benchmark draws one.
///
/// `Math.round(Math.random() * 1000) % max` exactly, rounding and all. It is a
/// poor generator -- a thousand possible draws folded into twenty-five buckets is
/// visibly lumpy -- and it is the one every published entry uses, so it is the one
/// that produces a comparable amount of text.
#[cfg(topcoat_client)]
fn random(max: usize) -> usize {
    (math_round(math_random() * 1000.0) as usize) % max
}

// The browser's own generator. A `#[js_extern]` declaration that names no module
// is rooted at a global, so these are the two calls spelled exactly as the
// benchmark spells them, with nothing imported and no host function in between.
#[cfg(topcoat_client)]
#[::js_extern_macro::js_extern]
unsafe extern "C" {
    /// `Math.random()`.
    #[js(call = "Math.random")]
    fn math_random() -> f64;

    /// `Math.round(x)`.
    #[js(call = "Math.round")]
    fn math_round(x: f64) -> f64;
}

// The store, which the page lends the island. An `extern "C"` declaration in a
// crate the backend compiles is a call to `__rt.<name>(...)`, and `__rt` is the
// module the import map resolves for this crate, which demo-app serves itself.
// Nothing below is a rule: a length, four readers, and five writes that do exactly
// what they are named. The rules are up here.
#[cfg(topcoat_client)]
#[allow(improper_ctypes)]
unsafe extern "C" {
    /// How many rows the store holds.
    fn bench_len() -> f64;

    /// The id of the row at `index`.
    fn bench_id(index: f64) -> f64;

    /// Its label.
    fn bench_label(index: f64) -> &'static str;

    /// The class its `<tr>` carries, which is the selection made visible.
    fn bench_class(index: f64) -> &'static str;

    /// Puts a row on the end, labelled with the three words joined by spaces and
    /// numbered with the store's next id.
    fn bench_push(adjective: &str, colour: &str, noun: &str);

    /// Marks the label of the row at `index`, which is what an update does to it.
    fn bench_bang(index: f64);

    /// Exchanges the rows at two positions.
    fn bench_swap(a: f64, b: f64);

    /// Takes the row at `index` out.
    fn bench_remove(index: f64);

    /// Makes the row at `index` the selected one.
    fn bench_select(index: f64);

    /// Empties the store and forgets the selection.
    fn bench_clear();
}

/// The js-framework-benchmark app: six buttons and a table of rows.
///
/// The markup is the benchmark's contract and not a choice. The ids on the
/// buttons, the classes on the cells, the `<span class="glyphicon">` inside the
/// delete anchor and the empty fourth column are all read by the harness, and the
/// selected row is `<tr class="danger">`.
#[::view_dom_macro::island]
pub async fn bench() -> ::topcoat::Result {
    view! {
        <div class="container">
            // The one signal. Nobody reads it for its value: every operation
            // writes the store and bumps this, and the table's loop reads it, so
            // a bump is a rebuild. See the module docs.
            signal version = 0.0;

            <div class="jumbotron">
                <div class="row">
                    <div class="col-md-6">
                        <h1>"Topcoat-island"</h1>
                    </div>
                    <div class="col-md-6">
                        <div class="row">
                            <div class="col-sm-6 smallpad">
                                <button
                                    type="button"
                                    class="btn btn-primary btn-block"
                                    id="run"
                                    @click=$(|_e| version.run())
                                >
                                    "Create 1,000 rows"
                                </button>
                            </div>
                            <div class="col-sm-6 smallpad">
                                <button
                                    type="button"
                                    class="btn btn-primary btn-block"
                                    id="runlots"
                                    @click=$(|_e| version.runlots())
                                >
                                    "Create 10,000 rows"
                                </button>
                            </div>
                            <div class="col-sm-6 smallpad">
                                <button
                                    type="button"
                                    class="btn btn-primary btn-block"
                                    id="add"
                                    @click=$(|_e| version.add())
                                >
                                    "Append 1,000 rows"
                                </button>
                            </div>
                            <div class="col-sm-6 smallpad">
                                <button
                                    type="button"
                                    class="btn btn-primary btn-block"
                                    id="update"
                                    @click=$(|_e| version.update())
                                >
                                    "Update every 10th row"
                                </button>
                            </div>
                            <div class="col-sm-6 smallpad">
                                <button
                                    type="button"
                                    class="btn btn-primary btn-block"
                                    id="clear"
                                    @click=$(|_e| version.clear())
                                >
                                    "Clear"
                                </button>
                            </div>
                            <div class="col-sm-6 smallpad">
                                <button
                                    type="button"
                                    class="btn btn-primary btn-block"
                                    id="swaprows"
                                    @click=$(|_e| version.swaprows())
                                >
                                    "Swap Rows"
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            </div>

            <table class="table table-hover table-striped test-data">
                <tbody>
                    for row in rows(version.get()) {
                        // Bound here rather than read inside the handlers: the
                        // server's expression language captures a free identifier
                        // whole, and a `RowView` is not one of its types. See the
                        // module docs.
                        let index = row.index;

                        <tr class=(row.class)>
                            <td class="col-md-1">(row.id)</td>
                            <td class="col-md-4">
                                <a @click=$(move |_e| version.select(index))>(row.label)</a>
                            </td>
                            <td class="col-md-1">
                                <a @click=$(move |_e| version.remove(index))>
                                    <span class="glyphicon glyphicon-remove" aria-hidden="true">
                                    </span>
                                </a>
                            </td>
                            <td class="col-md-6"></td>
                        </tr>
                    }
                </tbody>
            </table>

            // Not decoration: the benchmark's pages carry this so the glyphicon
            // font is fetched before the first row that uses it is drawn, which
            // keeps a font load out of the measured operation. It is hidden by
            // the harness's own stylesheet.
            <span class="preloadicon glyphicon glyphicon-remove" aria-hidden="true"></span>
        </div>
    }
}
