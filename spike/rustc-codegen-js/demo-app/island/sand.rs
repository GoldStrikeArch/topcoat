//! The falling sand island: a drawing surface whose grains fall, and the first
//! thing in this repository that paints.
//!
//! Like the other islands this file is a module of two crates. What is new here
//! is the SURFACE: the browser half owns a `<canvas>` and every pixel that lands
//! on it is a `fillRect` the compiled Rust asked for, through a `#[js_extern]`
//! block of eight declarations and nothing else. No drawing loop was written in
//! JavaScript, and there is none to write.
//!
//! # What is compiled Rust
//!
//! The physics. A generation reads the current board and writes the next one:
//! a grain falls straight down if it can, otherwise diagonally, and the side it
//! prefers comes from a hash of its own coordinates and the frame number. That
//! is the whole of [`fall`], and it is the reason the toy is here rather than a
//! `requestAnimationFrame` loop with a `Math.random()` in it.
//!
//! The painting decision is compiled Rust too, and it is the interesting half:
//! a frame paints the cells whose value CHANGED and no others, which is a
//! comparison of two boards rather than a redraw. A full 60x40 redraw would be
//! 2400 `fillRect` calls a frame; a settled pile is zero.
//!
//! The pointer arithmetic is compiled Rust as well. What crosses from the
//! browser is `clientX` and `clientY`, untranslated; turning those into a cell
//! is [`deposit`]'s subtraction of the canvas's own rectangle, read through two
//! more declarations.
//!
//! # What the host module lends it
//!
//! Two things a declared interface cannot say.
//!
//! A PLACE TO KEEP A BOARD. This crate is `#![no_std]` with no heap and no
//! mutable statics, so a `[u8; 2400]` that survives from one frame to the next
//! is not something it can declare. It can borrow one: `board` hands back the
//! `{ buf, off, len }` record a slice is, over an array the host keeps, and the
//! backend indexes one as `record.buf[record.off + i]`. So the boards below are
//! read and written in place, with nothing copied in either direction.
//!
//! TWO CALLBACKS. A clock and a pointer both call a function, and handing
//! JavaScript a function means handing it a compiled Rust closure. So both are
//! made in [`island-rt`](../src/island-rt.mjs) and handed the signals to write.
//! Neither knows anything about sand.
//!
//! # Two holes, and why they do not overlap
//!
//! The view has two reactive holes and they subscribe to disjoint sets of
//! signals, which is what keeps a pointer move off the physics and a frame off
//! the pointer.
//!
//! [`frame`] reads `tick` and nothing else, so the clock is the only thing that
//! runs a generation. [`deposit`] reads `px`, `py`, `pdown` and `brush`, so the
//! pointer and the brush picker are the only things that stamp. Both are written
//! as a `for` over an iterator of one value, because a `$( .. )` is compiled by
//! the server's expression language as well as by the client's and that language
//! has no free function calls; a loop's iterable is plain Rust on both halves.
//!
//! # Why setup is safe where it is
//!
//! An island's setup must be synchronous: hydration's window is exactly one
//! synchronous call stack, and anything resuming after a suspension point finds
//! it closed and silently rebuilds the DOM instead of adopting the server's.
//! Finding the canvas, registering four listeners and opening an interval are
//! all synchronous calls, so all of them are made on the first run of the frame
//! hole's effect, inside that window. Nothing can arrive during it: a pointer
//! event and a timer are both delivered in later tasks. So no mount hook is
//! needed, and none was added.

/// Columns of the grid.
///
/// The grid is the browser's alone -- the server renders an empty canvas and a
/// blank readout -- so this constant and the three below it carry the client
/// gate their callers carry. `test` joins the gate because the parity test at
/// the foot of the file multiplies them back out against the literals in the
/// view's `width` and `height`, and that test is a server-side one.
#[cfg(any(topcoat_client, test))]
pub const COLS: usize = 60;

/// Rows of it.
#[cfg(any(topcoat_client, test))]
pub const ROWS: usize = 40;

/// Cells in it, which is also the length the host makes its boards.
#[cfg(any(topcoat_client, test))]
pub const CELLS: usize = COLS * ROWS;

/// How many pixels wide and tall one cell is drawn.
///
/// The canvas is `COLS * SCALE` by `ROWS * SCALE`, and the two numbers in the
/// view's `width` and `height` are that product written out: an attribute is a
/// literal on both halves and cannot be computed from these.
#[cfg(any(topcoat_client, test))]
pub const SCALE: f64 = 6.0;

/// An empty cell.
#[cfg(topcoat_client)]
pub const EMPTY: u8 = 0;

/// A grain of sand, which falls.
#[cfg(topcoat_client)]
pub const SAND: u8 = 1;

/// A wall, which does not.
#[cfg(topcoat_client)]
pub const WALL: u8 = 2;

/// What each cell value is painted.
///
/// `EMPTY`'s colour is the canvas's own background rather than transparent,
/// because a cell is repainted when it EMPTIES and clearing one at a time would
/// be a second declaration for no gain. `.sand-canvas` carries the same colour
/// in `demo.css`, so an unpainted cell and an emptied one look alike.
#[cfg(topcoat_client)]
pub const COLOURS: [&str; 3] = ["#1d1d22", "#dcb173", "#6c7080"];

/// What each brush is called, in the order the picker's buttons set.
///
/// One table, read by both halves: the server renders the name of the brush a
/// fresh island starts with, and the browser's first run has to reach the same
/// string or the markup it adopts is not the markup it would have built.
pub const BRUSHES: [&str; 3] = ["sand", "wall", "erase"];

/// The canvas the grid is drawn on.
#[cfg(topcoat_client)]
const CANVAS: &str = ".sand-canvas";

/// The rendering context asked for.
#[cfg(topcoat_client)]
const CONTEXT: &str = "2d";

/// The pointer events the island listens for.
///
/// `pointerleave` as well as `pointerup`, because a button released outside the
/// canvas is never reported to it and the last thing the sink saw would stay
/// held down for ever.
#[cfg(topcoat_client)]
const POINTER: [&str; 4] = ["pointerdown", "pointermove", "pointerup", "pointerleave"];

/// How long a frame lasts, in milliseconds.
#[cfg(topcoat_client)]
const FRAME_MS: f64 = 33.0;

/// How far the brush reaches from the cell under the pointer.
#[cfg(topcoat_client)]
const REACH: i32 = 2;

/// The board a frame reads and the pointer writes.
///
/// Also the island's claim number. The board ids are handed out by the page's
/// plan and are unique per island already, so borrowing one of them is a claim
/// that cannot collide with another island's; a number of its own would be a
/// second thing to keep unique.
#[cfg(topcoat_client)]
const CUR: f64 = 3.0;

/// The board a frame writes, which becomes the current one when they exchange.
#[cfg(topcoat_client)]
const NEXT: f64 = 4.0;

/// The number of grains a frame ends with, as an iterator of one.
///
/// An iterator because the view reaches this through a `for`, which is where a
/// hole may call a function.
///
/// A flag rather than an `Option` the iterator takes from, and that is measured:
/// `Option::take` lowers to `__rt.overwrite`, which `src/island-rt.mjs` does not
/// supply, so a `take` here fails the build's shim check. `Hits` counts too.
pub struct Grains {
    count: f64,
    given: bool,
}

impl Iterator for Grains {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        if self.given {
            return None;
        }
        self.given = true;
        Some(self.count)
    }
}

/// The brush's name, as an iterator of one.
pub struct Brush {
    name: &'static str,
    given: bool,
}

impl Iterator for Brush {
    type Item = &'static str;

    fn next(&mut self) -> Option<&'static str> {
        if self.given {
            return None;
        }
        self.given = true;
        Some(self.name)
    }
}

/// What the brush the picker last chose is called.
fn brush_name(brush: f64) -> &'static str {
    let pick = brush as usize;
    if pick < BRUSHES.len() {
        BRUSHES[pick]
    } else {
        BRUSHES[0]
    }
}

/// What the brush the picker last chose puts in a cell.
#[cfg(topcoat_client)]
fn brush_value(brush: f64) -> u8 {
    match brush as u32 {
        0 => SAND,
        1 => WALL,
        _ => EMPTY,
    }
}

/// Which way a grain at `col`, `row` leans on frame `tick` when it cannot fall
/// straight down.
///
/// A hash rather than a generator, and that is the point rather than a
/// shortcut: an island has nowhere to keep a seed between frames, and a toy that
/// asked the host for a random number would have put the one interesting
/// decision in JavaScript. This is a function of the three numbers it is given,
/// so the same board on the same frame falls the same way twice, which is what
/// lets a check assert a pile rather than look at one.
#[cfg(topcoat_client)]
fn leans_left(col: usize, row: usize, tick: f64) -> bool {
    let mut mixed = (col as u32).wrapping_mul(374_761_393)
        ^ (row as u32).wrapping_mul(668_265_263)
        ^ (tick as u32).wrapping_mul(2_246_822_519);
    mixed ^= mixed >> 13;
    mixed = mixed.wrapping_mul(1_274_126_177);
    ((mixed ^ (mixed >> 16)) & 1) == 0
}

/// Moves every grain of `cells` one step, bottom row first.
///
/// Bottom first is what makes a fall one cell per frame rather than the whole
/// column at once: a grain moves into a row that has already been settled, so
/// nothing is moved twice on the same pass. The bottom row itself is never a
/// source, which is what makes a pile.
#[cfg(topcoat_client)]
fn fall(cells: &mut [u8], tick: f64) {
    for row in (0..ROWS - 1).rev() {
        for col in 0..COLS {
            let from = row * COLS + col;
            if cells[from] != SAND {
                continue;
            }

            let below = from + COLS;
            if cells[below] == EMPTY {
                cells[below] = SAND;
                cells[from] = EMPTY;
                continue;
            }

            // Both diagonals are tried, in an order the hash picks, so a grain
            // on a slope is not biased towards one side of it.
            let first = if leans_left(col, row, tick) { -1 } else { 1 };
            if slide(cells, col, below, first) || slide(cells, col, below, -first) {
                cells[from] = EMPTY;
            }
        }
    }
}

/// Puts a grain in the cell one row down and `step` columns across, if that cell
/// exists and is empty. Answers whether it did.
#[cfg(topcoat_client)]
fn slide(cells: &mut [u8], col: usize, below: usize, step: i32) -> bool {
    let across = col as i32 + step;
    if across < 0 || across >= COLS as i32 {
        return false;
    }

    let into = (below as i32 + step) as usize;
    if cells[into] != EMPTY {
        return false;
    }

    cells[into] = SAND;
    true
}

/// Puts `value` in every cell the brush covers at `col`, `row`, and paints the
/// ones that changed.
///
/// The stamp paints rather than leaving it to the next frame, and it has to: a
/// frame paints the difference between the board it read and the board it wrote,
/// and a cell the pointer changed is the same in both. Painting here is also
/// what makes drawing feel immediate instead of arriving up to a frame late.
#[cfg(topcoat_client)]
fn stamp(context: JsValue, col: i32, row: i32, value: u8) {
    let cells = unsafe { board(CUR, CELLS as f64) };
    let mut styled = false;

    for down in 0..REACH * 2 + 1 {
        for across in 0..REACH * 2 + 1 {
            let step_down = down - REACH;
            let step_across = across - REACH;
            if step_down * step_down + step_across * step_across > REACH * REACH {
                continue;
            }

            let at_col = col + step_across;
            let at_row = row + step_down;
            if at_col < 0 || at_row < 0 || at_col >= COLS as i32 || at_row >= ROWS as i32 {
                continue;
            }

            let index = at_row as usize * COLS + at_col as usize;
            if cells[index] == value {
                continue;
            }
            cells[index] = value;

            // Set once, and only if something is actually painted: an erase
            // over empty ground is a whole stamp that draws nothing.
            if !styled {
                set_fill_style(context, COLOURS[value as usize]);
                styled = true;
            }
            fill_rect(
                context,
                at_col as f64 * SCALE,
                at_row as f64 * SCALE,
                SCALE,
                SCALE,
            );
        }
    }
}

/// How many grains the board holds once `tick`'s generation has been stepped.
///
/// The server has run no frames, so its answer is the empty board's, which is
/// what the browser's first run answers too: the board starts empty, an empty
/// board steps to an empty board, and nothing is painted. That is what makes the
/// markup the browser adopts the markup it would have built.
#[cfg(not(topcoat_client))]
fn frame(
    _tick: f64,
    _clock: &::topcoat::runtime::Signal<f64>,
    _x: &::topcoat::runtime::Signal<f64>,
    _y: &::topcoat::runtime::Signal<f64>,
    _down: &::topcoat::runtime::Signal<f64>,
) -> Grains {
    Grains {
        count: 0.0,
        given: false,
    }
}

#[cfg(topcoat_client)]
fn frame(
    tick: f64,
    clock: ::view_abi::Sig<f64>,
    x: ::view_abi::Sig<f64>,
    y: ::view_abi::Sig<f64>,
    down: ::view_abi::Sig<f64>,
) -> Grains {
    let canvas = query_selector(CANVAS);

    // The first run is the island's one synchronous entry point, and everything
    // here is a synchronous call. See the module docs on why that is the safe
    // place for it.
    //
    // The four listeners are registered directly rather than through the view's
    // `@` handlers, which is not only because a coordinate cannot cross a
    // handler: a delegated event has to be named in the page's `dom::script`
    // list, and four pointer events named there would be delegated for every
    // island on the page rather than for the one that draws.
    if unsafe { claim(CUR) } {
        let sink = unsafe { pointer_sink(x, y, down) };
        // The array is iterated BY VALUE, which is the shape that reads best
        // and the one `examples/core-tests/28_array_into_iter.rs` pins: a
        // reference to `core`'s `PolymorphicIter` is a `{ ptr, meta }` fat
        // pointer over the record rather than a fat slice over it.
        for kind in POINTER {
            add_listener(canvas, kind, sink);
        }
        unsafe { clock_start(clock, FRAME_MS) };
    }

    let context = get_context(canvas, CONTEXT);
    let cur = unsafe { board_ro(CUR, CELLS as f64) };
    let next = unsafe { board(NEXT, CELLS as f64) };

    // The generation is written into the other board rather than in place, so
    // that the two can be compared afterwards. Starting it as a copy is what
    // makes a cell nothing moved into or out of compare equal.
    for index in 0..CELLS {
        next[index] = cur[index];
    }
    fall(next, tick);

    // One pass, which both counts the grains and paints the difference. The
    // style is set only when it differs from the last cell painted, and the
    // cells are visited top to bottom: a grain that fell EMPTIED a cell above
    // one it FILLED, so the empties come first and a frame of ordinary falling
    // sets the style twice. `styled` starts at a value no cell holds, so the
    // first cell painted always sets it.
    let mut grains = 0u32;
    let mut styled = 255u8;
    for index in 0..CELLS {
        let value = next[index];
        if value == SAND {
            grains += 1;
        }
        if value == cur[index] {
            continue;
        }
        if value != styled {
            set_fill_style(context, COLOURS[value as usize]);
            styled = value;
        }
        fill_rect(
            context,
            (index % COLS) as f64 * SCALE,
            (index / COLS) as f64 * SCALE,
            SCALE,
            SCALE,
        );
    }

    unsafe { board_swap(CUR, NEXT) };
    Grains {
        count: grains as f64,
        given: false,
    }
}

/// Stamps the brush where the pointer is, and answers what the brush is called.
///
/// The name is the hole's value rather than a separate one because the stamp
/// itself renders nothing: a hole has to render something, and the brush is what
/// this hole is about. The server has seen no pointer, so its answer is the name
/// of the brush a fresh island starts with -- which is the browser's first
/// answer too, because `pdown` starts at zero and nothing is stamped.
#[cfg(not(topcoat_client))]
fn deposit(_x: f64, _y: f64, _down: f64, brush: f64) -> Brush {
    Brush {
        name: brush_name(brush),
        given: false,
    }
}

#[cfg(topcoat_client)]
fn deposit(x: f64, y: f64, down: f64, brush: f64) -> Brush {
    // `buttons` rather than a flag, so this is "nothing is held" and not "the
    // primary button is not held".
    if down != 0.0 {
        let canvas = query_selector(CANVAS);
        let rect = bounding_rect(canvas);
        let col = ((x - rect_left(rect)) / SCALE) as i32;
        let row = ((y - rect_top(rect)) / SCALE) as i32;
        stamp(get_context(canvas, CONTEXT), col, row, brush_value(brush));
    }

    Brush {
        name: brush_name(brush),
        given: false,
    }
}

/// An opaque JavaScript value: an element, a context, a rectangle, a function.
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

// The browser's own globals. A declaration that names no module is rooted at a
// global, so these are the operations spelled exactly as a page would spell
// them, with nothing imported. Eight of them are the whole foreign surface of a
// drawing toy.
#[cfg(topcoat_client)]
#[::js_extern_macro::js_extern]
unsafe extern "C" {
    /// `document.querySelector(selector)`.
    ///
    /// Called once per frame and once per stamp rather than held. Caching the
    /// canvas would need somewhere to put it, and the only somewhere is a host
    /// slot; the shared `__rt` surface offers none on purpose, and two property
    /// lookups a frame is not what a frame costs.
    #[js(call = "document.querySelector")]
    fn query_selector(selector: &str) -> JsValue;

    /// `canvas.getContext(kind)`.
    #[js(method = "getContext")]
    fn get_context(canvas: JsValue, kind: &str) -> JsValue;

    /// `context.fillStyle = colour`.
    ///
    /// A property write, which is a shape of its own: the value is assigned
    /// rather than passed.
    #[js(set = "fillStyle")]
    fn set_fill_style(context: JsValue, colour: &str);

    /// `context.fillRect(x, y, width, height)`.
    #[js(method = "fillRect")]
    fn fill_rect(context: JsValue, x: f64, y: f64, width: f64, height: f64);

    /// `target.addEventListener(kind, handler)`.
    #[js(method = "addEventListener")]
    fn add_listener(target: JsValue, kind: &str, handler: JsValue);

    /// `element.getBoundingClientRect()`.
    #[js(method = "getBoundingClientRect")]
    fn bounding_rect(element: JsValue) -> JsValue;

    /// `rect.left`, which is where the canvas starts across the viewport.
    #[js(get = "left")]
    fn rect_left(rect: JsValue) -> f64;

    /// `rect.top`, the same down it.
    #[js(get = "top")]
    fn rect_top(rect: JsValue) -> f64;
}

// What the page lends the island: a board to keep, a clock, and a pointer. Every
// one of these is shared with the other two showcase islands and knows nothing
// about sand. See the note above them in `src/island-rt.mjs`.
#[cfg(topcoat_client)]
#[allow(improper_ctypes)]
unsafe extern "C" {
    /// True the first time `id` asks and false afterwards.
    ///
    /// The frame hole's effect re-runs on every tick, and setting up twice would
    /// register eight listeners and open a second clock.
    fn claim(id: f64) -> bool;

    /// The host's board of `len` cells, borrowed to write.
    fn board(id: f64, len: f64) -> &'static mut [u8];

    /// The same board, borrowed to read.
    fn board_ro(id: f64, len: f64) -> &'static [u8];

    /// Exchanges what two boards hold, which is what makes the board just
    /// written the current one.
    fn board_swap(a: f64, b: f64);

    /// Moves `sig` on by one every `ms`, which is what runs a frame.
    fn clock_start(sig: ::view_abi::Sig<f64>, ms: f64);

    /// The function a pointer event calls, which writes the event's own
    /// `clientX`, `clientY` and `buttons` into the three signals.
    fn pointer_sink(
        x: ::view_abi::Sig<f64>,
        y: ::view_abi::Sig<f64>,
        down: ::view_abi::Sig<f64>,
    ) -> JsValue;
}

/// A drawing surface whose grains fall, rendered by the server and taken over by
/// the browser.
#[::view_dom_macro::island]
pub async fn sand() -> ::topcoat::Result {
    view! {
        <div class="island sand">
            signal tick = 0.0;
            signal px = 0.0;
            signal py = 0.0;
            signal pdown = 0.0;
            signal brush = 0.0;

            // 360 by 240: `COLS * SCALE` by `ROWS * SCALE`, written out because
            // an attribute is a literal on both halves.
            <canvas class="sand-canvas" width="360" height="240"></canvas>

            <p class="sand-count">
                "grains "
                // The frame hole. It reads `tick` and nothing else, so the clock
                // is the only thing that runs a generation; the other three
                // signals are passed unread, for the sink to write.
                <span class="sand-grains">
                    for grains in frame(tick.get(), tick, px, py, pdown) {
                        (grains)
                    }
                </span>
                " brush "
                // The pointer hole. It reads the pointer and the brush and
                // nothing else, so a frame never re-stamps.
                <span class="sand-brush">
                    for name in deposit(px.get(), py.get(), pdown.get(), brush.get()) {
                        (name)
                    }
                </span>
            </p>

            // Each button lets go of the pointer before it changes the brush,
            // and both halves of that are needed. A pointer released OUTSIDE the
            // canvas is never reported to it, so `pdown` can still be held when
            // the picker is reached; the hole that stamps is the hole that reads
            // the brush, so changing the brush with the pointer still held would
            // stamp once under it. Releasing FIRST is what makes the brush's own
            // re-run land with nothing held.
            <div class="island-controls">
                <button class="island-step" type="button" @click=$(|_e| {
                    pdown.set(0.0);
                    brush.set(0.0)
                })>
                    "sand"
                </button>
                <button class="island-step" type="button" @click=$(|_e| {
                    pdown.set(0.0);
                    brush.set(1.0)
                })>
                    "wall"
                </button>
                <button class="island-step" type="button" @click=$(|_e| {
                    pdown.set(0.0);
                    brush.set(2.0)
                })>
                    "erase"
                </button>
            </div>
        </div>
    }
}

/// What the server answers, which is the whole of what it decides.
///
/// The browser's first run has to reach the same two values or the markup it
/// adopts is not the markup it would have built. `smoke/sand.mjs` asserts the
/// browser's half of that against the real chunk; this is the server's half,
/// asserted where it is written.
#[cfg(all(test, not(topcoat_client)))]
mod tests {
    use super::*;

    /// A signal holding `value`, for the arguments the frame hole is handed but
    /// does not read on this half.
    fn held(value: f64) -> ::topcoat::runtime::Signal<f64> {
        ::topcoat::runtime::Signal::new(value)
    }

    #[test]
    fn the_server_has_run_no_frames_so_it_renders_no_grains() {
        let clock = held(0.0);
        let x = held(0.0);
        let y = held(0.0);
        let down = held(0.0);
        let counts: Vec<f64> = frame(0.0, &clock, &x, &y, &down).collect();
        assert_eq!(counts, vec![0.0]);
    }

    #[test]
    fn the_server_renders_the_name_of_the_brush_an_island_starts_with() {
        let names: Vec<&str> = deposit(0.0, 0.0, 0.0, 0.0).collect();
        assert_eq!(names, vec![BRUSHES[0]]);
    }

    #[test]
    fn a_brush_is_named_by_its_number_and_an_unknown_one_is_the_first() {
        for (number, name) in BRUSHES.iter().enumerate() {
            assert_eq!(brush_name(number as f64), *name);
        }
        assert_eq!(brush_name(BRUSHES.len() as f64), BRUSHES[0]);
    }

    #[test]
    fn the_canvas_the_view_declares_is_the_grid_the_island_steps() {
        // The width and height are literals in the view, because an attribute
        // is one on both halves. This is the arithmetic they were written from,
        // so a grid resized without resizing the canvas fails here rather than
        // by drawing off the edge.
        assert_eq!(COLS as f64 * SCALE, 360.0);
        assert_eq!(ROWS as f64 * SCALE, 240.0);
        assert_eq!(CELLS, COLS * ROWS);
    }
}
