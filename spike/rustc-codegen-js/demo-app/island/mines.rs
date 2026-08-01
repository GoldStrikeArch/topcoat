//! The Minesweeper island: a 12x10 field of 15 mines, played entirely in
//! compiled Rust.
//!
//! Like every island this file is a module of two crates. The server crate reads
//! it as ordinary Topcoat and renders the HTML with the hydration keys in it; the
//! client crate reads it with `topcoat_client` set and the rustc backend compiles
//! the same view to JavaScript. What is new here is that the thing being compiled
//! is a GAME: a seeded placement, a recursive flood fill and a win test, none of
//! which the old expression language could have said at all.
//!
//! # Where the field lives
//!
//! In the host, as one board of 121 bytes reached through
//! [`island-rt`](../src/island-rt.mjs)'s `board`/`board_ro` pair. The client crate
//! is `#![no_std]` with no heap and no mutable statics, so a field that survives
//! from one click to the next is not something it can declare -- but a `&[u8]` is
//! the `{ buf, off, len }` record the host hands out, and the backend indexes one
//! as `record.buf[record.off + i]`, so the array is read and written in place with
//! nothing copied either way.
//!
//! One byte per cell, three bits of it used ([`MINE`], [`REVEALED`], [`FLAGGED`]).
//! Adjacent counts are not stored: they are a function of the mines, and a stored
//! copy is a second answer that can disagree with the first.
//!
//! # Why a click writes signals and nothing else
//!
//! A `$( ... )` value is compiled by the server's expression language as well as
//! by the client compiler, and that language has no free function calls: `f(x)`
//! lowers to `f.call((x,))`, which is the unstable `Fn` trait. So a handler can
//! read an event and write a signal, and that is the whole of what these three do.
//!
//! Everything else -- placing the mines, flooding a region, counting what is
//! left -- runs in the plain Rust a `for` loop's iterable is on both halves.
//! [`cells`] and [`notes`] are those iterables, and both begin by calling
//! [`apply`].
//!
//! # Why applying is guarded by a token
//!
//! Two loops read the same three signals, so a click re-runs both, and neither
//! ordering is promised. Flagging is not idempotent -- applied twice it flags and
//! unflags -- so [`apply`] does its work AT MOST ONCE per value of `version`,
//! remembering the last one it acted on in the board's tail byte. Whichever loop
//! runs first performs the move; the other finds it done and only reads. Nothing
//! here writes a signal, so no render can cause another one.
//!
//! # Why the grid has one handler and not a hundred and twenty
//!
//! A `@click` on each cell of a `for` loop works -- a thunk built inside a loop
//! snapshots its environment, see `examples/dom-tests/23_for_row_handlers.rs` --
//! but it costs one closure per cell, rebuilt on every render. One handler on the
//! container plus `value=(index)` on each cell is the cheaper shape, because
//! `e.target_value()` compiles to `event.target.value || ""` -- the element the
//! click came FROM, not the one the listener sits on -- so a single listener for
//! the whole field can still tell the cells apart.
//!
//! # Why the server needs no board
//!
//! It renders the field before the first click, and that field is all zeroes. So
//! the server's half of [`field`] is a zeroed array and every rule above runs over
//! it unchanged, which makes the markup it writes the markup the client's first
//! render would build -- parity by construction rather than by agreement.

/// Columns of the field.
const COLUMNS: i32 = 12;

/// Rows of it.
const ROWS: i32 = 10;

/// Cells in it.
const CELLS: i32 = COLUMNS * ROWS;

/// How many of them hide a mine.
const MINE_COUNT: i32 = 15;

/// The board id the host keeps the field under. The ids are the islands' own:
/// LIFE_CUR=1, LIFE_NEXT=2, SAND_CUR=3, SAND_NEXT=4, MINES=5.
#[cfg(topcoat_client)]
const BOARD: f64 = 5.0;

/// Where the last applied `version` is remembered.
///
/// One byte past the cells, which is what makes the board 121 rather than the 120
/// a field alone needs. It is on the board rather than in a signal because a
/// signal written from inside a render is a loop, and because this is exactly as
/// long-lived as the field it guards.
const TOKEN: usize = CELLS as usize;

/// How long the board is, cells and tail together.
const LEN: usize = TOKEN + 1;

/// A cell hides a mine.
const MINE: u8 = 1;

/// A cell has been revealed.
const REVEALED: u8 = 2;

/// A cell has been flagged.
const FLAGGED: u8 = 4;

/// The eight cells that surround a cell, as offsets from it.
///
/// A table rather than a pair of nested loops, because the centre cell is not its
/// own neighbour and a table says so by leaving `(0, 0)` out.
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

/// What a revealed cell shows, by how many mines are around it.
///
/// A table of `&'static str` rather than a number formatted at render time: the
/// client crate has no heap to format into, and a zero shows nothing at all,
/// which no formatter would have done.
const COUNTS: [&str; 9] = ["", "1", "2", "3", "4", "5", "6", "7", "8"];

/// What a revealed cell is classed as, by the same count, so the digit can be
/// coloured the way every minesweeper colours it.
const OPEN: [&str; 9] = [
    "mines-cell open",
    "mines-cell open n1",
    "mines-cell open n2",
    "mines-cell open n3",
    "mines-cell open n4",
    "mines-cell open n5",
    "mines-cell open n6",
    "mines-cell open n7",
    "mines-cell open n8",
];

/// An untouched cell: what the server renders, and what the browser adopts.
const HIDDEN: &str = "mines-cell";

/// A flagged one.
const FLAGGED_CLASS: &str = "mines-cell flagged";

/// A revealed mine.
const BLOWN: &str = "mines-cell blown";

/// What a cell that shows nothing shows.
const BLANK: &str = "";

/// The mark a flag leaves.
const FLAG_MARK: &str = "\u{2691}";

/// The mark a mine leaves once it is out.
const MINE_MARK: &str = "\u{2731}";

/// A game still being played, as a class and a word.
const PLAYING: &str = "mines-state";
const PLAYING_WORD: &str = "playing";

/// One that has been won.
const WON: &str = "mines-state won";
const WON_WORD: &str = "cleared";

/// One that has been lost.
const LOST: &str = "mines-state lost";
const LOST_WORD: &str = "boom";

/// What a click asks for. The view writes these numbers into `action`, as
/// literals: a captured constant would be one more thing for the server's
/// expression language to carry across, and three numbers with a table beside
/// them are the same information.
#[cfg(topcoat_client)]
const RESTART: f64 = 0.0;

/// Reveal the cell the click came from.
#[cfg(topcoat_client)]
const REVEAL: f64 = 1.0;

/// Flag it, or take a flag off it.
#[cfg(topcoat_client)]
const FLAG: f64 = 2.0;

/// How many draws a seeded placement may reject before it gives up and sweeps.
///
/// A rejection loop over a full-period generator terminates, but "terminates"
/// and "terminates soon" are different promises and this one runs in a browser.
#[cfg(topcoat_client)]
const DRAWS: i32 = 4096;

/// The state a seed of zero is replaced by, because `xorshift` fed zero produces
/// zero for ever.
#[cfg(topcoat_client)]
const DEFAULT_SEED: u32 = 0x2545_f491;

/// The event a handler is handed.
///
/// The browser's compiler hands a handler a borrowed event and the server's hands
/// it an owned one, so the type is declared once per target and the handler names
/// only this. The lifetime is the borrow on the client side and unused on the
/// server's.
#[cfg(topcoat_client)]
type Ev<'a> = &'a ::view_abi::Event;

#[cfg(not(topcoat_client))]
type Ev<'a> = ::topcoat::runtime::Event;

/// What a click leaves behind: the `value` of the cell it came from.
///
/// The server's signal holds a `String`, because that is the type its runtime can
/// serialize; the browser's holds a `&'static str`, because its crate has no heap.
/// One alias so that everything reading it is written once.
#[cfg(topcoat_client)]
type Pick = &'static str;

#[cfg(not(topcoat_client))]
type Pick = String;

/// The pick a fresh island starts with, which is no cell at all.
#[cfg(topcoat_client)]
fn no_pick() -> Pick {
    ""
}

#[cfg(not(topcoat_client))]
fn no_pick() -> Pick {
    String::new()
}

/// One cell, as the view draws it.
///
/// `index` is what makes the container handler work: it is written into the
/// button's `value`, and a click reads it back out of `event.target`.
#[derive(Clone, Copy)]
struct Cell {
    index: f64,
    class: &'static str,
    label: &'static str,
}

/// The status line: how many mines are still unaccounted for, and how the game
/// stands.
#[derive(Clone, Copy)]
struct Note {
    left: f64,
    class: &'static str,
    text: &'static str,
}

/// The field as it stands, borrowed for reading.
///
/// The host keeps it, so a render sees what the last click left. Reading and
/// writing are two `extern` declarations because they are two Rust types and an
/// `extern` name is its JavaScript name; they are the same array.
#[cfg(topcoat_client)]
fn field() -> &'static [u8] {
    unsafe { board_ro(BOARD, LEN as f64) }
}

/// The field the server renders: an untouched one.
///
/// Not a placeholder. The server's answer is the field before the first click,
/// every byte of it zero, and every rule in this file run over zeroes produces
/// the all-hidden grid -- which is exactly what the browser's first render
/// produces from the host's freshly made board. See the module docs.
#[cfg(not(topcoat_client))]
fn field() -> &'static [u8] {
    &UNTOUCHED
}

#[cfg(not(topcoat_client))]
static UNTOUCHED: [u8; LEN] = [0; LEN];

/// How many of the eight cells around `at` hide a mine.
///
/// The field does not wrap: a neighbour off the edge is not a cell, which is what
/// separates a minesweeper from a torus.
fn around(cells: &[u8], at: i32) -> i32 {
    let x = at % COLUMNS;
    let y = at / COLUMNS;
    // Not `mines`: the island's own name is a unit struct in this module, and a
    // local of that name would shadow it.
    let mut found = 0;
    for &(dx, dy) in NEIGHBOURS.iter() {
        let nx = x + dx;
        let ny = y + dy;
        if !(0..COLUMNS).contains(&nx) || !(0..ROWS).contains(&ny) {
            continue;
        }
        if cells[(ny * COLUMNS + nx) as usize] & MINE != 0 {
            found += 1;
        }
    }
    found
}

/// How the cell at `index` draws.
fn cell_at(cells: &[u8], index: i32) -> Cell {
    let cell = cells[index as usize];

    if cell & REVEALED != 0 {
        if cell & MINE != 0 {
            return Cell {
                index: index as f64,
                class: BLOWN,
                label: MINE_MARK,
            };
        }
        let count = around(cells, index) as usize;
        return Cell {
            index: index as f64,
            class: OPEN[count],
            label: COUNTS[count],
        };
    }

    if cell & FLAGGED != 0 {
        return Cell {
            index: index as f64,
            class: FLAGGED_CLASS,
            label: FLAG_MARK,
        };
    }

    Cell {
        index: index as f64,
        class: HIDDEN,
        label: BLANK,
    }
}

/// How the game stands.
///
/// Lost the moment a mine is revealed, won when every cell that is not a mine
/// is. Both are read off the field rather than remembered, so a restart that
/// zeroes the cells has nothing else to put back.
fn status_of(cells: &[u8]) -> Note {
    let mut flagged = 0;
    let mut hidden = 0;
    let mut blown = false;

    for index in 0..CELLS {
        let cell = cells[index as usize];
        if cell & FLAGGED != 0 {
            flagged += 1;
        }
        if cell & MINE != 0 && cell & REVEALED != 0 {
            blown = true;
        }
        if cell & MINE == 0 && cell & REVEALED == 0 {
            hidden += 1;
        }
    }

    let left = (MINE_COUNT - flagged) as f64;
    if blown {
        return Note {
            left,
            class: LOST,
            text: LOST_WORD,
        };
    }
    // Before the first click every cell is hidden and none is a mine, so this
    // asks for 120 reveals and is false, which is what makes the opening state
    // `playing` on both halves.
    if hidden == 0 {
        return Note {
            left,
            class: WON,
            text: WON_WORD,
        };
    }
    Note {
        left,
        class: PLAYING,
        text: PLAYING_WORD,
    }
}

/// The cells of the field, one at a time.
///
/// An iterator rather than a collection: the field is the host's and this crate
/// has nowhere to copy it to.
struct Cells {
    at: i32,
    cells: &'static [u8],
}

impl Iterator for Cells {
    type Item = Cell;

    fn next(&mut self) -> Option<Cell> {
        if self.at >= CELLS {
            return None;
        }
        let cell = cell_at(self.cells, self.at);
        self.at += 1;
        Some(cell)
    }
}

/// The status line, as an iterator of exactly one.
///
/// A `for` loop's iterable is the only place in a view where a function may be
/// called, and the status has to be recomputed when the field changes, so the one
/// line is written as a loop over one note. See the module docs.
struct Notes {
    at: i32,
    note: Note,
}

impl Iterator for Notes {
    type Item = Note;

    fn next(&mut self) -> Option<Note> {
        if self.at > 0 {
            return None;
        }
        self.at += 1;
        Some(self.note)
    }
}

/// The cells to draw, having first played whatever click `version` is counting.
fn cells(version: f64, pick: Pick, action: f64, seed: f64) -> Cells {
    apply(version, pick, action, seed);
    Cells {
        at: 0,
        cells: field(),
    }
}

/// The status line, having first played the same click.
///
/// Calling [`apply`] from both loops is deliberate: it is the token that makes
/// the move happen once, not the order the two loops run in.
fn notes(version: f64, pick: Pick, action: f64, seed: f64) -> Notes {
    apply(version, pick, action, seed);
    Notes {
        at: 0,
        note: status_of(field()),
    }
}

/// Plays the click `version` is counting, if it has not been played already.
///
/// The server plays nothing: it renders the field before the first click, and
/// `version` is still the zero its signal was seeded with.
#[cfg(not(topcoat_client))]
fn apply(_version: f64, _pick: Pick, _action: f64, _seed: f64) {}

#[cfg(topcoat_client)]
fn apply(version: f64, pick: Pick, action: f64, seed: f64) {
    let cells = unsafe { board(BOARD, LEN as f64) };

    // The tail byte is the low eight bits of the last `version` acted on, and the
    // signal is seeded with zero, so the first render finds them equal and plays
    // nothing -- which is what makes it the markup the server sent. Eight bits are
    // enough because a click happens between renders and a render happens between
    // clicks: two versions 256 apart cannot both be pending.
    let token = (version as u32 & 0xff) as u8;
    if cells[TOKEN] == token {
        return;
    }
    cells[TOKEN] = token;

    if action == RESTART {
        for index in 0..CELLS {
            cells[index as usize] = 0;
        }
        return;
    }

    // A click on the grid itself rather than on a cell -- a gap between buttons --
    // has no value to read, so `target_value` answers the empty string and this
    // answers "no cell". That is the whole reason it answers `|| ""` rather than
    // undefined.
    let at = parse_index(pick);
    if at < 0 {
        return;
    }

    if action == FLAG {
        // A revealed cell cannot be flagged: the flag is a note about what is
        // still hidden.
        if cells[at as usize] & REVEALED == 0 {
            cells[at as usize] ^= FLAGGED;
        }
        return;
    }

    if action != REVEAL || cells[at as usize] & FLAGGED != 0 {
        return;
    }

    // The mines are laid on the FIRST reveal, around the cell that was clicked,
    // so an opening click can never be the one that loses. Nothing is placed
    // before that, which is why the server can render a field it never seeded.
    if !placed(cells) {
        place(cells, at, seed);
    }

    reveal_at(cells, at);

    // Losing turns the whole field face up, which is the only way to see what the
    // game was.
    if cells[at as usize] & MINE != 0 {
        for index in 0..CELLS {
            if cells[index as usize] & MINE != 0 {
                cells[index as usize] |= REVEALED;
            }
        }
    }
}

/// Whether the mines have been laid.
///
/// Read off the field rather than remembered: [`place`] always lays at least one,
/// and a restart zeroes every cell, so the mines themselves are the flag.
#[cfg(topcoat_client)]
fn placed(cells: &[u8]) -> bool {
    for index in 0..CELLS {
        if cells[index as usize] & MINE != 0 {
            return true;
        }
    }
    false
}

/// Lays [`MINE_COUNT`] mines, none of them within one cell of `safe`.
///
/// Nine cells are kept clear rather than one, so the opening reveal is a zero and
/// floods -- a first click that survives but shows a `4` is a worse first move
/// than the game should hand out. That leaves 111 cells for 15 mines.
#[cfg(topcoat_client)]
fn place(cells: &mut [u8], safe: i32, seed: f64) {
    // Folded rather than cast. A page seeds this with a clock, and a clock is
    // past `u32::MAX` by a factor of four hundred: cast, every seed would
    // saturate to the same number and every field would be the same field.
    let mut state = (seed % 4_294_967_296.0) as u32;
    if state == 0 {
        state = DEFAULT_SEED;
    }

    let mut laid = 0;
    let mut draws = 0;
    while laid < MINE_COUNT && draws < DRAWS {
        draws += 1;
        state = xorshift(state);
        if lay(cells, (state % CELLS as u32) as i32, safe) {
            laid += 1;
        }
    }

    // A draw can be rejected, so the loop above is bounded rather than trusted.
    // Whatever it did not lay goes at the first cell that will take it.
    let mut at = 0;
    while laid < MINE_COUNT && at < CELLS {
        if lay(cells, at, safe) {
            laid += 1;
        }
        at += 1;
    }
}

/// Puts a mine at `at` unless there is one there or it is too close to `safe`,
/// and answers whether it put one.
#[cfg(topcoat_client)]
fn lay(cells: &mut [u8], at: i32, safe: i32) -> bool {
    if cells[at as usize] & MINE != 0 || near(at, safe) {
        return false;
    }
    cells[at as usize] |= MINE;
    true
}

/// Whether `at` is `safe` or touches it.
#[cfg(topcoat_client)]
fn near(at: i32, safe: i32) -> bool {
    let dx = at % COLUMNS - safe % COLUMNS;
    let dy = at / COLUMNS - safe / COLUMNS;
    dx >= -1 && dx <= 1 && dy >= -1 && dy <= 1
}

/// One step of a 32 bit xorshift generator.
///
/// The caller carries the state, so a field is a function of its seed alone and
/// the same seed lays the same mines -- which is what lets a check assert where
/// they are.
#[cfg(topcoat_client)]
fn xorshift(state: u32) -> u32 {
    let mut x = state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Reveals `at`, and everything a zero opens onto.
///
/// The recursion is the point: a region is not a shape anything computes, it is
/// wherever the zeroes reach. A flagged cell stops it, so a flag protects what it
/// is on even when the flood arrives from behind.
#[cfg(topcoat_client)]
fn reveal_at(cells: &mut [u8], at: i32) {
    let cell = cells[at as usize];
    if cell & (REVEALED | FLAGGED) != 0 {
        return;
    }
    cells[at as usize] = cell | REVEALED;

    if cell & MINE != 0 || around(cells, at) != 0 {
        return;
    }

    let x = at % COLUMNS;
    let y = at / COLUMNS;
    for &(dx, dy) in NEIGHBOURS.iter() {
        let nx = x + dx;
        let ny = y + dy;
        if !(0..COLUMNS).contains(&nx) || !(0..ROWS).contains(&ny) {
            continue;
        }
        reveal_at(cells, ny * COLUMNS + nx);
    }
}

/// Which cell a click's `value` names, or `-1` for none.
///
/// The digits are read out of the string's own bytes rather than parsed by the
/// library, because what arrives is whatever was on the element the click came
/// from and this has to answer "not a cell" for all of it.
#[cfg(topcoat_client)]
fn parse_index(pick: Pick) -> i32 {
    let bytes = pick.as_bytes();
    if bytes.is_empty() || bytes.len() > 3 {
        return -1;
    }

    let mut value = 0;
    for byte in bytes.iter() {
        let digit = *byte as i32 - b'0' as i32;
        if digit < 0 || digit > 9 {
            return -1;
        }
        value = value * 10 + digit;
    }

    if value >= CELLS { -1 } else { value }
}

// The field itself, which the page lends the island. An `extern "C"` declaration
// in a crate the backend compiles is a call to `__rt.<name>(...)`, and `__rt` is
// the module the import map resolves for this crate, which demo-app serves
// itself.
#[cfg(topcoat_client)]
#[allow(improper_ctypes)]
unsafe extern "C" {
    /// The host's board under `id`, `len` bytes of it, borrowed to write.
    ///
    /// The array is made once and kept, so what a click writes is what the next
    /// render reads.
    fn board(id: f64, len: f64) -> &'static mut [u8];

    /// The same board, borrowed to read.
    fn board_ro(id: f64, len: f64) -> &'static [u8];
}

/// A minefield: 12 by 10, 15 mines, first click safe.
///
/// `seed` is where the mines come from, and the page passes it in through the
/// island's arguments. Nothing is placed until the first reveal, so the server
/// renders an untouched field and the seed does not have to mean anything to it.
#[::view_dom_macro::island]
pub async fn mines(seed: f64) -> ::topcoat::Result {
    view! {
        <div class="island mines">
            signal version = 0.0;
            signal pick = no_pick();
            signal action = 0.0;

            <p class="mines-status">
                for note in notes(version.get(), pick.get(), action.get(), seed) {
                    <span class=(note.class)>
                        <span class="mines-left">(note.left)</span>
                        <span class="mines-word">(note.text)</span>
                    </span>
                }
            </p>

            // ONE handler for the whole field, not one per cell: a `@click` inside
            // the loop below works, but it compiles to a hundred and twenty
            // closures rebuilt on every render. See the module docs, and
            // `examples/dom-tests/23_for_row_handlers.rs`.
            <div
                class="mines-grid"
                @click=$(|e| {
                    let e: Ev = e;
                    pick.set(e.target_value());
                    action.set(1.0);
                    version.set(version.get() + 1.0)
                })
                @contextmenu=$(|e| {
                    let e: Ev = e;
                    e.prevent_default();
                    pick.set(e.target_value());
                    action.set(2.0);
                    version.set(version.get() + 1.0)
                })
            >
                for cell in cells(version.get(), pick.get(), action.get(), seed) {
                    <button class=(cell.class) type="button" value=(cell.index)>
                        (cell.label)
                    </button>
                }
            </div>

            <div class="island-controls">
                <button
                    class="island-step"
                    type="button"
                    @click=$(|_e| {
                        action.set(0.0);
                        version.set(version.get() + 1.0)
                    })
                >
                    "restart"
                </button>
            </div>
        </div>
    }
}

// The rules, checked on the half that has a test harness.
//
// Everything below is SHARED code -- `around`, `cell_at`, `status_of`, `cells`
// and `notes` all take the field as a slice and neither half has its own copy --
// so a rule that holds here holds in the browser too. What is client only is the
// half that MUTATES a field (`place`, `reveal_at`, `parse_index`), and that is
// checked against the compiled chunk by `smoke/mines.mjs`.
//
// The first two are the parity checks: what the server renders for an untouched
// field. `smoke/mines.mjs` asserts the compiled island's first render against the
// same numbers and the same tables, so the two halves are pinned to one answer
// from opposite sides.
#[cfg(all(test, not(topcoat_client)))]
mod tests {
    use super::*;

    /// A field with mines at the named cells and nothing else touched.
    ///
    /// Not `mines`: the island's own name is a unit struct in this module, so a
    /// parameter of that name would be read as a pattern matching it.
    fn laid(at_cells: &[i32]) -> [u8; LEN] {
        let mut field = [0; LEN];
        for &at in at_cells {
            field[at as usize] = MINE;
        }
        field
    }

    #[test]
    fn the_server_renders_one_hidden_cell_per_square_in_index_order() {
        let drawn: Vec<Cell> = cells(0.0, no_pick(), 0.0, 1.0).collect();
        assert_eq!(drawn.len(), CELLS as usize);
        for (index, cell) in drawn.iter().enumerate() {
            assert_eq!(cell.index, index as f64);
            assert_eq!(cell.class, HIDDEN);
            assert_eq!(cell.label, BLANK);
        }
    }

    #[test]
    fn the_server_renders_every_mine_as_unaccounted_for() {
        let notes: Vec<Note> = notes(0.0, no_pick(), 0.0, 1.0).collect();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].left, MINE_COUNT as f64);
        assert_eq!(notes[0].class, PLAYING);
        assert_eq!(notes[0].text, PLAYING_WORD);
    }

    #[test]
    fn a_count_is_the_mines_that_touch_a_cell_and_the_field_does_not_wrap() {
        // Mines at 0 and 13, which are the corner and the cell diagonally in
        // from it. Cell 1 touches both; cell 11 is the far end of the top row
        // and touches neither, which is what a field that wrapped would get
        // wrong.
        let field = laid(&[0, 13]);
        assert_eq!(around(&field, 1), 2);
        assert_eq!(around(&field, 11), 0);
        assert_eq!(around(&field, 12), 2);
    }

    #[test]
    fn a_revealed_cell_shows_the_count_its_class_is_named_for() {
        let mut field = laid(&[0, 13]);
        field[1] |= REVEALED;
        let cell = cell_at(&field, 1);
        assert_eq!(cell.label, COUNTS[2]);
        assert_eq!(cell.class, OPEN[2]);
    }

    #[test]
    fn a_revealed_mine_is_the_end_of_it() {
        let mut field = laid(&[7]);
        field[7] |= REVEALED;
        assert_eq!(cell_at(&field, 7).class, BLOWN);
        assert_eq!(cell_at(&field, 7).label, MINE_MARK);
        assert_eq!(status_of(&field).text, LOST_WORD);
    }

    #[test]
    fn a_flag_is_a_note_about_what_is_still_hidden() {
        let mut field = laid(&[7]);
        field[7] |= FLAGGED;
        assert_eq!(cell_at(&field, 7).class, FLAGGED_CLASS);
        assert_eq!(cell_at(&field, 7).label, FLAG_MARK);
        assert_eq!(status_of(&field).left, (MINE_COUNT - 1) as f64);
        assert_eq!(status_of(&field).text, PLAYING_WORD);
    }

    #[test]
    fn a_field_is_cleared_when_every_cell_that_is_not_a_mine_is_revealed() {
        let mut field = laid(&[7]);
        for cell in field.iter_mut().take(CELLS as usize) {
            if *cell & MINE == 0 {
                *cell |= REVEALED;
            }
        }
        assert_eq!(status_of(&field).text, WON_WORD);
        // The mine is still hidden, and still counted as unaccounted for.
        assert_eq!(status_of(&field).left, MINE_COUNT as f64);
    }
}
