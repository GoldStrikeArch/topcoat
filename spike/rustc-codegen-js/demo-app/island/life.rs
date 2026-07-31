//! The Game of Life island: a board the browser plays, stepped by compiled Rust.
//!
//! Like every island this file is a module of two crates. The server crate reads
//! it as ordinary Topcoat and renders the first generation as HTML with the
//! hydration keys in it; the client crate reads it with `topcoat_client` set and
//! the rustc backend compiles the same view to JavaScript. What is new here is
//! that the thing being compiled is a *program*: a wrapping neighbour count, a
//! `match` over a pair that is the whole of Conway's rule, and a double-buffered
//! step, none of which the old runtime expression language could say at all.
//!
//! # Where the board lives
//!
//! Nowhere in this crate. The island crate is `#![no_std]` with no heap and no
//! mutable statics, so a `[u8; 640]` that survives from one generation to the
//! next is not a thing it can declare. It borrows one instead: `__rt.board` hands
//! back the `{ buf, off, len }` record a slice is, over an array the page keeps,
//! and the backend indexes one as `record.buf[record.off + i]`. So `cur()` and
//! `next()` below are a read and a write of the page's own array with nothing
//! copied in either direction. See the showcase section of
//! [`island-rt`](../src/island-rt.mjs).
//!
//! # What the view may say, and what that costs
//!
//! A `$( ... )` value -- a reactive hole or a handler -- is compiled by the
//! server's expression language as well as by the client compiler, and that
//! language has no free function calls: `f(a)` lowers to `f.call((a,))`, which is
//! not a thing a Rust fn item is. It does have method calls. So everything a hole
//! or a handler asks of the simulation is a method of [`Control`], written on the
//! signal that the answer bumps -- `edits.toggle(..)` changes the board and says
//! so, `generation.advance()` steps it and says so. Plain Rust is still plain
//! Rust in the two places a view has it: a `for` loop's iterable and a `signal`
//! initialiser.
//!
//! # Which hole re-runs when
//!
//! Two subscriptions, kept apart on purpose.
//!
//! * The driver hole reads `tick`, which a 20 Hz interval bumps. It is one number of text, so
//!   running it 20 times a second costs nothing.
//! * The grid's loop reads `generation` and `edits`, which move only when the board actually
//!   changed. 640 rows are rebuilt then and not otherwise, so a paused board is a page doing no DOM
//!   work at all.
//!
//! The clock runs at a fixed rate and the speed slider picks a stride over it,
//! because an interval cannot be re-timed through the exports the page lends and
//! starting a second one would leave the first running.
//!
//! # Why writing `generation` from the driver does not loop
//!
//! The driver runs inside an effect, and an effect that reads what it writes
//! re-runs itself for ever unless the write makes the condition false. It does
//! here: `stepped` holds the tick the last step happened at, so the re-run the
//! write provokes finds `stepped == tick` and does nothing. That fixed point is
//! also what keeps a change to `playing` or `speed` -- both read by the driver,
//! both able to re-run it mid-tick -- from stepping the board a second time.
//!
//! # Why the first render matches on both sides
//!
//! The server renders the opening board and the client adopts that markup, so the
//! two have to agree cell for cell. They agree because there is one
//! [`initial_alive`], shared by both halves with no cfg on it: the server maps it
//! over `0..640` and the client stamps the same function into the board before
//! the first render reads it. What that produces is written down twice -- in the
//! test at the foot of this file, against the server half, and in `smoke/life.mjs`,
//! against the compiled client -- so neither side can drift alone.

/// Columns of the board.
const WIDTH: i32 = 32;

/// Rows of it.
const HEIGHT: i32 = 20;

/// How many cells that is. The loop rebuilds all of them whenever the board
/// changes, so it is the number to move if the page ever stutters.
const CELLS: i32 = WIDTH * HEIGHT;

/// The board being read, as [`island-rt`](../src/island-rt.mjs) numbers it.
///
/// The four constants from here to [`NEIGHBOURS`] are the simulation's own, and
/// the simulation is the client's half alone: the server renders the opening
/// board from [`initial_alive`] and never steps it. So they carry the same
/// `topcoat_client` gate their callers do, rather than sitting outside it and
/// being dead code in every server build.
#[cfg(topcoat_client)]
const CUR: f64 = 1.0;

/// The board being written. A generation depends on its neighbours *before* the
/// step, so writing into the board being read would let the first half of a
/// generation change the second half.
#[cfg(topcoat_client)]
const NEXT: f64 = 2.0;

/// The number this island claims its one-time setup under.
///
/// The same number as [`CUR`] rather than a new one, and that is the convention
/// and not a coincidence: three islands share `claim`, the plan hands out board
/// ids and nothing else, so an island claiming under its first board id is an
/// island that invents no number at all.
#[cfg(topcoat_client)]
const CLAIM: f64 = CUR;

/// The eight cells that surround a cell, as offsets from it.
///
/// A table rather than a pair of nested loops, because the centre cell is not its
/// own neighbour and a table says so by leaving `(0, 0)` out.
#[cfg(topcoat_client)]
const NEIGHBOURS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// The five living cells of a glider, as offsets from the cell it is stamped at.
const GLIDER: [(i32, i32); 5] = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];

/// Where the glider starts.
const GLIDER_AT: (i32, i32) = (1, 1);

/// The five living cells of an r-pentomino, which is the smallest pattern worth
/// watching: it takes over a thousand generations to settle.
const R_PENTOMINO: [(i32, i32); 5] = [(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)];

/// Where it starts, which is roughly the middle of the board.
const R_PENTOMINO_AT: (i32, i32) = (15, 8);

/// Whether the cell at `index` is alive before anything has stepped.
///
/// Shared by both halves with no cfg on it, and that is the whole of the parity
/// story: the server renders this over `0..CELLS` and the client stamps the same
/// answers into the board before its first render reads it, so the markup the
/// browser adopts is the markup it would have built.
///
/// Integer arithmetic only. A float that reads back as `1` on one side and
/// `1.0000000000000002` on the other would be a difference in the HTML.
fn initial_alive(index: i32) -> bool {
    if !(0..CELLS).contains(&index) {
        return false;
    }

    let x = index % WIDTH;
    let y = index / WIDTH;
    covers(&GLIDER, x - GLIDER_AT.0, y - GLIDER_AT.1)
        || covers(&R_PENTOMINO, x - R_PENTOMINO_AT.0, y - R_PENTOMINO_AT.1)
}

/// Whether `shape` has a living cell at the offset `(dx, dy)`.
///
/// An indexed `while` rather than an iterator: this runs 640 times per pattern on
/// the first render of both halves, and a plain loop is the same answer with
/// nothing borrowed.
fn covers(shape: &[(i32, i32)], dx: i32, dy: i32) -> bool {
    let mut at = 0;
    while at < shape.len() {
        let (ox, oy) = shape[at];
        if ox == dx && oy == dy {
            return true;
        }
        at += 1;
    }
    false
}

/// How many cells the opening board has alive.
///
/// What the driver hole renders before anything has stepped, on both halves. The
/// server has no board to count, and the client's first run counts the board it
/// has just stamped, so this is the one place the two answers are made the same.
#[cfg(not(topcoat_client))]
fn initial_population() -> f64 {
    let mut alive = 0.0;
    let mut index = 0;
    while index < CELLS {
        if initial_alive(index) {
            alive += 1.0;
        }
        index += 1;
    }
    alive
}

/// One cell of the board, as the grid's loop renders it.
#[derive(Clone, Copy)]
struct Cell {
    /// Which cell this is, which the button carries as its `value` so that one
    /// handler on the container can tell which one was clicked.
    index: i32,
    /// `"cell"` or `"cell on"`.
    class: &'static str,
}

/// How a cell in that state is dressed.
///
/// Two `&'static str`s rather than a formatted string, because the island crate
/// has no heap to format into.
fn alive_class(alive: bool) -> &'static str {
    if alive { "cell on" } else { "cell" }
}

/// The event a handler is handed.
///
/// The browser's compiler hands a handler a borrowed event and the server's hands
/// it an owned one, so the type is declared once per target and the handler names
/// only this. Naming the type in the parameter instead would name one target's
/// type in both. The search island's arrangement, and for its reasons.
#[cfg(topcoat_client)]
type Ev<'a> = &'a ::view_abi::Event;

#[cfg(not(topcoat_client))]
type Ev<'a> = ::topcoat::runtime::Event;

/// The cells to draw.
///
/// The server has stepped nothing, so its answer is the opening board. Both
/// arguments are read for the subscription rather than for the number: reading
/// them is what makes the loop run again when the board changes, and the board is
/// where the cells come from.
#[cfg(not(topcoat_client))]
fn cells(_generation: f64, _edits: f64, _tick: &::topcoat::runtime::Signal<f64>) -> Vec<Cell> {
    (0..CELLS)
        .map(|index| Cell {
            index,
            class: alive_class(initial_alive(index)),
        })
        .collect()
}

/// The cells to draw, read straight out of the page's array.
///
/// `tick` is taken and never read. It is what [`open`] needs to start the clock,
/// and this loop is one of the two holes that might be the first to run: whichever
/// of them gets there opens the island, so neither has to be written above the
/// other. Taking it does not subscribe the loop to it -- only `get` does that --
/// which is what keeps a 20 Hz clock from rebuilding 640 rows 20 times a second.
#[cfg(topcoat_client)]
fn cells(_generation: f64, _edits: f64, tick: ::view_abi::Sig<f64>) -> Cells {
    open(tick);
    Cells {
        at: 0,
        board: cur(),
    }
}

/// The cells of the current board, one at a time.
///
/// An iterator rather than a collection, for the reason the search island's is:
/// there is no heap here to collect into.
#[cfg(topcoat_client)]
struct Cells {
    at: i32,
    board: &'static [u8],
}

#[cfg(topcoat_client)]
impl Iterator for Cells {
    type Item = Cell;

    fn next(&mut self) -> Option<Cell> {
        if self.at >= CELLS {
            return None;
        }
        let index = self.at;
        self.at += 1;
        Some(Cell {
            index,
            class: alive_class(self.board[index as usize] != 0),
        })
    }
}

/// What a hole or a handler may ask of the simulation.
///
/// Methods rather than functions because the view's expression language has no
/// free function calls, and written on the signal each one bumps because that is
/// the part a reader of the view needs: `edits.toggle(..)` says that a click
/// changes the board, `generation.advance()` says that the step button moves the
/// generation on.
///
/// The server's half is every method with an empty body. Its handlers are never
/// called -- the server renders markup and the browser's copy of the island is
/// what runs -- and its driver renders the opening population, which is the one
/// answer it has to get right.
#[cfg(topcoat_client)]
trait Control {
    /// Steps the board if this tick is one the speed asks for, and answers with
    /// the population either way. Written on `tick`.
    fn drive(
        self,
        playing: ::view_abi::Sig<f64>,
        speed: ::view_abi::Sig<f64>,
        stepped: ::view_abi::Sig<f64>,
        generation: ::view_abi::Sig<f64>,
    ) -> f64;

    /// Steps the board once, whatever the clock is doing. Written on
    /// `generation`.
    fn advance(self);

    /// Flips the cell the click came from, or does nothing if it came from
    /// between the cells. Written on `edits`.
    fn toggle(self, target: &'static str);

    /// Puts the opening board back. Written on `edits`.
    fn reset(self);

    /// Kills every cell. Written on `edits`.
    fn clear(self);

    /// Fills the board at random from `seed`. Written on `edits`.
    fn scatter(self, seed: f64);

    /// Takes the slider's position, which arrives as text. Written on `speed`.
    fn tune(self, target: &'static str);
}

#[cfg(topcoat_client)]
impl Control for ::view_abi::Sig<f64> {
    fn drive(
        self,
        playing: ::view_abi::Sig<f64>,
        speed: ::view_abi::Sig<f64>,
        stepped: ::view_abi::Sig<f64>,
        generation: ::view_abi::Sig<f64>,
    ) -> f64 {
        open(self);

        let now = self.get();
        // `now > 0.0` is what keeps the first run -- the one that happens inside
        // hydration -- from stepping: the markup the browser is adopting is the
        // board before any step, so a step here would be a mutation on a hydrate
        // that should have none.
        //
        // `stepped.get() != now` is the fixed point. Writing `stepped` and
        // `generation` below re-runs this hole, because it read them; the re-run
        // finds this false and stops. See the module docs.
        if playing.get() != 0.0
            && now > 0.0
            && stepped.get() != now
            && (now as i32) % stride(speed.get()) == 0
        {
            let population = step(cur(), next());
            unsafe { board_swap(CUR, NEXT) };
            stepped.set(now);
            generation.set(generation.get() + 1.0);
            return population;
        }

        population_of(cur())
    }

    fn advance(self) {
        let _ = step(cur(), next());
        unsafe { board_swap(CUR, NEXT) };
        self.set(self.get() + 1.0);
    }

    fn toggle(self, target: &'static str) {
        let index = cell_index(target);
        if index < 0 {
            return;
        }
        let board = cur_mut();
        board[index as usize] = if board[index as usize] == 0 { 1 } else { 0 };
        self.set(self.get() + 1.0);
    }

    fn reset(self) {
        stamp();
        self.set(self.get() + 1.0);
    }

    fn clear(self) {
        let board = cur_mut();
        let mut index = 0;
        while index < CELLS {
            board[index as usize] = 0;
            index += 1;
        }
        self.set(self.get() + 1.0);
    }

    fn scatter(self, seed: f64) {
        let mut state = seed as u32;
        if state == 0 {
            state = DEFAULT_SEED;
        }

        let board = cur_mut();
        let mut index = 0;
        while index < CELLS {
            state = xorshift(state);
            board[index as usize] = if state % 100 < DENSITY { 1 } else { 0 };
            index += 1;
        }
        self.set(self.get() + 1.0);
    }

    fn tune(self, target: &'static str) {
        let steps = digits(target);
        if steps < 0 {
            return;
        }
        self.set(steps as f64);
    }
}

#[cfg(not(topcoat_client))]
trait Control {
    fn drive(
        &self,
        playing: &::topcoat::runtime::SignalSurrogate<f64>,
        speed: &::topcoat::runtime::SignalSurrogate<f64>,
        stepped: &::topcoat::runtime::SignalSurrogate<f64>,
        generation: &::topcoat::runtime::SignalSurrogate<f64>,
    ) -> ::topcoat::runtime::F64Surrogate;

    fn advance(&self);

    fn toggle(&self, target: ::topcoat::runtime::StringSurrogate);

    fn reset(&self);

    fn clear(&self);

    fn scatter(&self, seed: ::topcoat::runtime::F64Surrogate);

    fn tune(&self, target: ::topcoat::runtime::StringSurrogate);
}

#[cfg(not(topcoat_client))]
impl Control for ::topcoat::runtime::SignalSurrogate<f64> {
    /// The population before anything has stepped, which is what the browser's
    /// first run answers too.
    fn drive(
        &self,
        _playing: &::topcoat::runtime::SignalSurrogate<f64>,
        _speed: &::topcoat::runtime::SignalSurrogate<f64>,
        _stepped: &::topcoat::runtime::SignalSurrogate<f64>,
        _generation: &::topcoat::runtime::SignalSurrogate<f64>,
    ) -> ::topcoat::runtime::F64Surrogate {
        ::topcoat::runtime::Surrogated::into_surrogate(initial_population())
    }

    fn advance(&self) {}

    fn toggle(&self, _target: ::topcoat::runtime::StringSurrogate) {}

    fn reset(&self) {}

    fn clear(&self) {}

    fn scatter(&self, _seed: ::topcoat::runtime::F64Surrogate) {}

    fn tune(&self, _target: ::topcoat::runtime::StringSurrogate) {}
}

/// How many ticks the clock delivers a second.
///
/// A fixed rate with a stride over it, rather than an interval the slider
/// re-times: the page lends this island a clock it can start and nothing that
/// stops one, so re-timing would mean a second interval beside the first.
#[cfg(topcoat_client)]
const TICKS_PER_SECOND: i32 = 20;

/// How long that is, as the interval takes it.
#[cfg(topcoat_client)]
const TICK_MS: f64 = 1000.0 / TICKS_PER_SECOND as f64;

/// How many cells in a hundred [`Control::scatter`] leaves alive. Below about a
/// fifth a random board dies out; well above it, it jams.
#[cfg(topcoat_client)]
const DENSITY: u32 = 30;

/// The state [`Control::scatter`] falls back to, because a seed of zero would
/// leave [`xorshift`] producing zero for ever.
#[cfg(topcoat_client)]
const DEFAULT_SEED: u32 = 0x2545_f491;

/// How many ticks apart two steps are, at `speed` steps a second.
///
/// Clamped rather than trusted: `speed` arrives from a range input, and the text
/// of an input is whatever the page was handed.
#[cfg(topcoat_client)]
fn stride(speed: f64) -> i32 {
    let steps = speed as i32;
    let steps = if steps < 1 {
        1
    } else if steps > TICKS_PER_SECOND {
        TICKS_PER_SECOND
    } else {
        steps
    };
    TICKS_PER_SECOND / steps
}

/// Opens the island: stamps the opening board, makes the second one, and starts
/// the clock.
///
/// Called by both of the holes that could be the first to run, and guarded by the
/// page's `claim` so that only the first call does anything. An island's setup
/// must be synchronous -- hydration's window is exactly one synchronous call
/// stack -- and every call here is, so the first run of a hole is the right place
/// for it. That is the dashboard island's arrangement.
#[cfg(topcoat_client)]
fn open(tick: ::view_abi::Sig<f64>) {
    if !unsafe { claim(CLAIM) } {
        return;
    }

    stamp();
    // Made now rather than in the middle of the first step, so that the step is
    // a write into an array that already exists.
    let _ = next();
    unsafe { clock_start(tick, TICK_MS) };
}

/// Writes the opening board.
#[cfg(topcoat_client)]
fn stamp() {
    let board = cur_mut();
    let mut index = 0;
    while index < CELLS {
        board[index as usize] = if initial_alive(index) { 1 } else { 0 };
        index += 1;
    }
}

/// The board being read.
#[cfg(topcoat_client)]
fn cur() -> &'static [u8] {
    unsafe { board_ro(CUR, CELLS as f64) }
}

/// The same board, to write.
#[cfg(topcoat_client)]
fn cur_mut() -> &'static mut [u8] {
    unsafe { board(CUR, CELLS as f64) }
}

/// The board the next generation is written into.
#[cfg(topcoat_client)]
fn next() -> &'static mut [u8] {
    unsafe { board(NEXT, CELLS as f64) }
}

/// How many of the eight neighbours of `(x, y)` are alive.
///
/// The board wraps: a coordinate that runs off one edge comes back on the
/// opposite one. That is what `rem_euclid` is for, and why it is not `%`. The two
/// differ exactly on negative numbers, which is exactly the case that arises
/// here: `-1 % w` is `-1`, but `(-1).rem_euclid(w)` is `w - 1`, the far edge.
#[cfg(topcoat_client)]
fn living_neighbours(board: &[u8], x: i32, y: i32) -> i32 {
    let mut alive = 0;
    let mut at = 0;
    while at < NEIGHBOURS.len() {
        let (dx, dy) = NEIGHBOURS[at];
        at += 1;
        let nx = (x + dx).rem_euclid(WIDTH);
        let ny = (y + dy).rem_euclid(HEIGHT);
        if board[(ny * WIDTH + nx) as usize] != 0 {
            alive += 1;
        }
    }
    alive
}

/// Writes the generation after `cur` into `next` and answers with how many cells
/// it left alive.
///
/// The rule itself is one `match` over a pair, which is the whole of Life: a
/// living cell with two or three living neighbours stays alive, a dead cell with
/// exactly three is born, and every other case is dead. It is also the thing this
/// page is about -- the expression language the old client had could not say a
/// `match` at all.
#[cfg(topcoat_client)]
fn step(cur: &[u8], next: &mut [u8]) -> f64 {
    let mut population = 0.0;
    let mut y = 0;
    while y < HEIGHT {
        let mut x = 0;
        while x < WIDTH {
            let index = (y * WIDTH + x) as usize;
            let alive = match (cur[index] != 0, living_neighbours(cur, x, y)) {
                (true, 2) | (true, 3) | (false, 3) => 1,
                _ => 0,
            };

            next[index] = alive;
            population += alive as f64;
            x += 1;
        }
        y += 1;
    }
    population
}

/// How many cells are alive.
///
/// [`step`] already answers this for the board it wrote, so this is for the runs
/// that stepped nothing.
#[cfg(topcoat_client)]
fn population_of(board: &[u8]) -> f64 {
    let mut alive = 0.0;
    let mut index = 0;
    while index < CELLS {
        if board[index as usize] != 0 {
            alive += 1.0;
        }
        index += 1;
    }
    alive
}

/// One step of a 32 bit xorshift generator.
///
/// The caller carries the state, so this is a pure function of its input and a
/// board can be replayed from the seed alone.
#[cfg(topcoat_client)]
fn xorshift(state: u32) -> u32 {
    let mut x = state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Which cell a click landed on, or `-1` for a click that landed on none.
///
/// A delegated click reports the element it came FROM, and every cell is a
/// `<button value=...>`, so the cell names itself and the grid needs one handler
/// rather than 640 closures. A click on the gap between cells reports the
/// container, which has no value, and `target_value` reads that back as the empty
/// string -- which is why "no cell" is a case here rather than an accident.
#[cfg(topcoat_client)]
fn cell_index(target: &str) -> i32 {
    let index = digits(target);
    if index < 0 || index >= CELLS {
        return -1;
    }
    index
}

/// The decimal number `text` spells, or `-1` if it spells anything else.
///
/// Written out rather than `str::parse`, which answers with a `Result` carrying
/// an error type this crate has no reason to bring in. `as_bytes` on a `&str` is
/// the host's own UTF-8 bytes, so this walks the string the page handed over with
/// nothing decoded.
#[cfg(topcoat_client)]
fn digits(text: &str) -> i32 {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return -1;
    }

    let mut value = 0;
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if !byte.is_ascii_digit() {
            return -1;
        }
        value = value * 10 + (byte - b'0') as i32;
        at += 1;
    }
    value
}

// What the page lends the island. An `extern "C"` declaration in a crate the
// backend compiles is a call to `__rt.<name>(...)`, and `__rt` is the module the
// import map resolves for this crate, which demo-app serves itself. None of these
// is about Life: a numbered array, an exchange, a counter that goes up. The rules
// are up here.
#[cfg(topcoat_client)]
#[allow(improper_ctypes)]
unsafe extern "C" {
    /// True the first time `id` asks and false afterwards.
    fn claim(id: f64) -> bool;

    /// A board of `len` cells, to write. The array is the page's and is kept, so
    /// what this hands back on the next frame is over the cells the last frame
    /// wrote.
    fn board(id: f64, len: f64) -> &'static mut [u8];

    /// The same board, to read. Two declarations because one borrows it mutably
    /// and the other does not, and an `extern` name is its JavaScript name.
    fn board_ro(id: f64, len: f64) -> &'static [u8];

    /// Exchanges what two boards hold. The `buf` moves and the record does not,
    /// so a slice taken before the exchange is over the new cells afterwards.
    fn board_swap(a: f64, b: f64);

    /// Moves `sig` on by one every `ms`, for ever.
    fn clock_start(sig: ::view_abi::Sig<f64>, ms: f64);
}

/// A board that plays itself, rendered by the server and taken over by the
/// browser.
#[::view_dom_macro::island]
pub async fn life() -> ::topcoat::Result {
    view! {
        <div class="island life">
            // How many steps have happened. The grid reads it, and the driver
            // writes it.
            signal generation = 0.0;
            // How many times the reader has changed the board by hand. A second
            // signal rather than one, because a click is not a generation and
            // saying it is would be a lie in the readout.
            signal edits = 0.0;
            // Whether the clock steps the board. A number and not a bool because
            // every signal in this island is one, and one type is one less thing
            // for the two halves to disagree about.
            signal playing = 1.0;
            // Steps a second the reader asked for.
            signal speed = 10.0;
            // The tick the last step happened at. See the module docs: this is
            // what stops the driver re-running itself.
            signal stepped = 0.0;
            // The clock.
            signal tick = 0.0;

            <p class="life-readout">
                "generation "
                <span class="life-generation">$(generation.get())</span>
                " · population "
                <span class="life-population">
                    $(tick.drive(playing, speed, stepped, generation))
                </span>
            </p>

            // ONE handler for the whole grid, and each cell carries its own
            // number. A handler per cell is the shape this asks for first; it
            // works, but it is 640 closures rebuilt on every render. See
            // `examples/dom-tests/23_for_row_handlers.rs`, which pins both.
            <div
                class="life-grid"
                @click=$(|e| {
                    let e: Ev = e;
                    edits.toggle(e.target_value())
                })
            >
                // `tabindex="-1"` because a board is 640 buttons and a page with
                // 640 tab stops in it is a page nobody can tab past. The grid is
                // worked by clicking, and the controls below it are not.
                for cell in cells(generation.get(), edits.get(), tick) {
                    <button
                        class=(cell.class)
                        type="button"
                        tabindex="-1"
                        value=(cell.index)
                    ></button>
                }
            </div>

            <div class="island-controls">
                <button
                    class="island-step"
                    type="button"
                    @click=$(|_e| playing.set(1.0 - playing.get()))
                >
                    "play / pause"
                </button>
                <button class="island-step" type="button" @click=$(|_e| generation.advance())>
                    "step"
                </button>
                <button class="island-step" type="button" @click=$(|_e| edits.reset())>
                    "reset"
                </button>
                <button class="island-step" type="button" @click=$(|_e| edits.clear())>
                    "clear"
                </button>
                <button class="island-step" type="button" @click=$(|_e| edits.scatter(tick.get()))>
                    "random"
                </button>
            </div>

            <label class="life-speed">
                "speed "
                <input
                    class="life-rate"
                    type="range"
                    min="1"
                    max="20"
                    value="10"
                    @input=$(|e| {
                        let e: Ev = e;
                        speed.tune(e.target_value())
                    })
                >
            </label>
        </div>
    }
}

/// The opening board, as the cells that are alive in it.
///
/// Written down rather than derived, and written down a second time in
/// `smoke/life.mjs`, which asserts the same list against the compiled client's
/// first render. That is the parity check: the server half is measured here, the
/// client half is measured there, and a change to [`initial_alive`] that reached
/// only one of them fails one of the two.
#[cfg(all(test, not(topcoat_client)))]
const OPENING_BOARD: &str = "34,67,97,98,99,272,273,303,304,336";

#[cfg(all(test, not(topcoat_client)))]
mod tests {
    use super::*;

    /// The cells the server renders alive, as `OPENING_BOARD` spells them.
    fn opening() -> String {
        cells(0.0, 0.0, &::topcoat::runtime::Signal::new(0.0))
            .into_iter()
            .filter(|cell| cell.class == "cell on")
            .map(|cell| cell.index.to_string())
            .collect::<Vec<_>>()
            .join(",")
    }

    #[test]
    fn the_server_renders_one_cell_per_position() {
        let rendered = cells(0.0, 0.0, &::topcoat::runtime::Signal::new(0.0));
        assert_eq!(rendered.len(), CELLS as usize);
        for (at, cell) in rendered.iter().enumerate() {
            assert_eq!(cell.index, at as i32);
        }
    }

    #[test]
    fn the_server_renders_the_opening_board() {
        assert_eq!(opening(), OPENING_BOARD);
    }

    #[test]
    fn the_opening_population_is_what_the_driver_renders() {
        // The driver hole's first answer on both halves. The client counts the
        // board it stamped; this counts the function it stamped it from.
        assert_eq!(
            initial_population() as usize,
            OPENING_BOARD.split(',').count()
        );
    }

    #[test]
    fn a_cell_off_the_board_is_dead_rather_than_a_panic() {
        // `initial_alive` is handed an index by two callers with different ideas
        // of the range, so out of range is an answer rather than a crash.
        assert!(!initial_alive(-1));
        assert!(!initial_alive(CELLS));
    }
}
