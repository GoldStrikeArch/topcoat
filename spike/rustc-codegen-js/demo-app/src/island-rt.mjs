// The functions the islands borrow from the page.
//
// A compiled island calls an `extern "C"` function as `__rt.<name>(...)`, and
// `__rt` is whatever module the island's build was told to import it from. This
// is that module: the import map resolves `topcoat-island-rt` to it, so an
// island's chunk imports the names it uses and nothing else.
//
// What lives here is what the islands cannot hold themselves. Their crate is
// `#![no_std]` with no heap and no mutable statics, so state that outlives a
// call stays on this side. And a callback is the one thing a declared interface
// cannot express -- handing JavaScript a function would mean handing it a
// compiled Rust closure -- so the functions events call are made here, closing
// over the signals they write.

// ----------------------------------------------------- the showcase islands
//
// Life, the falling sand toy and Minesweeper are three programs with the same
// two problems, so they are answered once here rather than three times: a game
// needs somewhere to keep a board between frames, and something to move time
// forward.
//
// WHY THE BOARDS ARE HERE. The island crate is `#![no_std]` with no heap and no
// mutable statics, so a `[u8; 640]` that survives from one generation to the
// next is not a thing it can declare. It can borrow one, though: a `&[u8]` is
// the `{ buf, off, len }` record `str_bytes` already returns, and the backend
// indexes one as `record.buf[record.off + i]`, so an array made here and handed
// out as that record is a board the compiled Rust reads AND writes directly.
// Nothing is copied in either direction, and no island holds any state.
//
// WHY THE CLOCK AND THE POINTER ARE HERE. Both are callbacks, and a callback is
// the one thing `#[js_extern]` cannot declare, for the reason the header gives:
// handing JavaScript a function means handing it a compiled Rust closure. So
// the function is made here and given the signals to write.
//
// WHAT IS NOT HERE, deliberately: any rule of any game. Every export below is
// game agnostic -- a numbered array, an exchange, a counter that goes up. The
// physics, the flood fill and the neighbour counting are compiled Rust, which is
// the whole claim the page is making.

/// The boards, by id. The ids are the islands' own and mean nothing here:
/// LIFE_CUR=1, LIFE_NEXT=2, SAND_CUR=3, SAND_NEXT=4, MINES=5.
const boards = new Map();

/// The islands that have already set themselves up, by id.
const claims = new Set();

/// Every interval [`clock_start`] has opened, so a check can close them.
const clocks = [];

/// True the first time `id` asks and false afterwards.
///
/// An island's setup runs inside an effect, the effect re-runs whenever the
/// island's state moves, and setting up twice would stamp the board twice and
/// start a second clock, so the claim is what makes the first run the only one.
/// A number rather than a flag because three islands share this file.
export function claim(id) {
	if (claims.has(id)) return false;
	claims.add(id);
	return true;
}

/// A board of `len` cells, as the record a `&'static mut [u8]` is.
///
/// The array is made once and kept, so the record handed back on the next frame
/// is over the same cells the last frame wrote. The length is fixed by the first
/// call: a later call asking for a different one gets the board that exists,
/// because what makes this a board rather than a buffer is that it persists.
export function board(id, len) {
	let held = boards.get(id);
	if (!held) {
		held = { buf: new Array(len).fill(0), off: 0, len };
		boards.set(id, held);
	}
	return held;
}

/// The same board, for the declaration that borrows it to read.
///
/// Rust needs two `extern` declarations because one returns `&'static mut [u8]`
/// and the other `&'static [u8]`, and an `extern` name is its JavaScript name.
/// There is one board and one behaviour, so this is the same function under the
/// second name rather than a copy of it.
export const board_ro = board;

/// Exchanges what two boards hold.
///
/// A generation is read out of one board and written into the other, and then
/// the two change places. The `buf` moves and the RECORD does not, so a record
/// handed out before the exchange is over the new cells afterwards -- which is
/// what lets an island ask for its boards once per frame and not think about it.
export function board_swap(a, b) {
	const left = boards.get(a);
	const right = boards.get(b);
	const buf = left.buf;
	left.buf = right.buf;
	right.buf = buf;
}

/// Moves `sig` on by one every `ms`, for ever.
///
/// A tick is a signal write and nothing else: the island reads the signal in the
/// hole that steps its simulation, so a write is what runs the step, and the
/// step is compiled Rust. `sig[1](sig[0]() + 1)` is `answered`'s spelling of a
/// bump, because a `Sig<T>` is the runtime's own `[read, write]` pair.
export function clock_start(sig, ms) {
	clocks.push(setInterval(() => sig[1](sig[0]() + 1), ms));
}

/// The function a pointer event calls, closing over the three signals it writes.
///
/// Coordinates cannot cross as a Rust value -- an `Event` exposes
/// `target_value()` and `prevent_default()` and nothing else -- so this is how a
/// drawing surface learns where the pointer is. What lands in the signals is the
/// event's own numbers, untranslated: turning a client coordinate into a cell of
/// a canvas is arithmetic over that canvas's rectangle, which is a decision, and
/// decisions are compiled Rust.
///
/// `buttons` rather than a flag: it is 0 whenever nothing is held, which is the
/// only question an island asks of it.
///
/// The gate closes before the coordinates move and reopens after them, and the
/// order is the point. An effect reading all three signals re-runs after EVERY
/// write, not once per event, so left open through a diagonal move it would run
/// between the two coordinate writes and act on the new column of the old row.
/// Closing first makes the in-between runs read "nothing held"; the one run
/// that acts is the last, which reads a coherent event.
export function pointer_sink(x, y, down) {
	return event => {
		down[1](0);
		x[1](event.clientX);
		y[1](event.clientY);
		down[1](event.buttons);
	};
}

/// Forgets every board, claim and clock.
///
/// Only a check calls this: a module is loaded once per page and a page
/// hydrates its islands once, so nothing in a browser ever needs it. And an
/// open interval keeps node alive, so without this `smoke/check.mjs` runs its
/// checks and then never exits.
export function rt_reset() {
	boards.clear();
	claims.clear();
	for (const clock of clocks) clearInterval(clock);
	clocks.length = 0;
}

// ------------------------------------------------------------ the benchmark
//
// The js-framework-benchmark island keeps its rows here, and the reason is the
// same one the boards above have plus one more.
//
// WHY THE ROWS ARE HERE. The island crate is `#![no_std]` with no heap and no
// mutable statics, so a list that survives from one click to the next is not a
// thing it can declare. An array of `{ id, label }` objects made here is one it
// can read a field at a time.
//
// WHY THE LABELS ARE BUILT HERE. A label is three words joined by spaces, and
// joining strings needs a heap. So the island picks the three words -- which is
// the decision, and the draw that picks them is compiled Rust down to the
// `Math.round(Math.random() * 1000) % max` the benchmark specifies -- and hands
// them over as three `&str`. Nothing here chooses anything.
//
// WHY THE ID COUNTER IS HERE. The benchmark requires ids to start at 1 and never
// restart, not even after clearing the table, so the counter outlives every row
// it numbered, and there is nowhere over there to keep it.
//
// WHY SELECTION IS ONE ID. There is one selected row, so there is one id, and a
// row's class is derived from it when the row is read. A per-row flag would be
// two places to change on a selection and a second thing that can be wrong.

/// The rows, in the order the table draws them.
let benchRows = [];

/// The id the next row will carry.
///
/// Ids start at 1 and never restart: clearing the table drops the rows and keeps
/// the counter, which is what the benchmark asks for and what makes an id worth
/// looking at in a trace.
let benchNextId = 1;

/// The id of the selected row, or 0 for none. Zero is safe as "none" because the
/// first id handed out is 1.
let benchSelected = 0;

/// How many rows the table has.
export function bench_len() {
	return benchRows.length;
}

/// The id of the row at `index`.
export function bench_id(index) {
	return benchRows[index].id;
}

/// Its label.
export function bench_label(index) {
	return benchRows[index].label;
}

/// The class its `<tr>` carries: the benchmark's `danger` on the selected row and
/// nothing on every other one.
export function bench_class(index) {
	return benchRows[index].id === benchSelected ? "danger" : "";
}

/// Puts a row on the end, labelled with the three words the island picked.
export function bench_push(adjective, colour, noun) {
	benchRows.push({ id: benchNextId++, label: `${adjective} ${colour} ${noun}` });
}

/// Marks the label of the row at `index`, which is the whole of what the
/// benchmark's update does to a row.
export function bench_bang(index) {
	benchRows[index].label += " !!!";
}

/// Exchanges the rows at two positions.
export function bench_swap(a, b) {
	const held = benchRows[a];
	benchRows[a] = benchRows[b];
	benchRows[b] = held;
}

/// Takes the row at `index` out.
export function bench_remove(index) {
	benchRows.splice(index, 1);
}

/// Makes the row at `index` the selected one.
export function bench_select(index) {
	benchSelected = benchRows[index].id;
}

/// Empties the table and forgets the selection, keeping the id counter.
export function bench_clear() {
	benchRows = [];
	benchSelected = 0;
}

/// Forgets everything, the id counter included.
///
/// Only a check calls this: a page loads this module once and a table is never
/// asked to start its ids again, so nothing in a browser needs it. It is
/// `rt_reset`'s counterpart for this island.
export function bench_reset() {
	benchRows = [];
	benchNextId = 1;
	benchSelected = 0;
}

// ------------------------------------------------- what the shim would supply
//
// The backend emits `__rt.<name>(...)` for operations with no short inline
// spelling, and `__rt` is whatever module the build named. The islands are built
// with NO SHIM, because a shim is a global-scope script and these are modules,
// so this file is `__rt` and every helper the program reaches for has to be
// here.
//
// To find out which: read the emitted chunks.
//
//     grep -oh "__rt\.[a-zA-Z_0-9]*" target/.../chunks/*.js | sort -u
//
// A missing one is not a compile error. It is a `TypeError` the first time that
// line runs, which is why the list is checked against the chunks rather than
// remembered.
//
// Every one below is `runtime/shim.js`'s implementation, kept faithful to it
// rather than reinvented: a divergence here would be a difference between an
// island and every other compiled program, which is the worst place for one.
// Where one is carried over in part, the omission is named and argued in its own
// doc comment.

/// A float to integer cast, which Rust defines as saturating: past the
/// destination's range clamps to its end, and NaN is zero. The bounds are
/// literals the backend passes. Clamping before truncating is what keeps
/// `Infinity` off the bitwise operators.
export function f2i(x, min, max) {
	if (Number.isNaN(x)) return 0;
	if (x <= min) return min;
	if (x >= max) return max;
	return Math.trunc(x);
}

/// The UTF-8 bytes of a string, as the `{ buf, off, len }` record a `&[u8]` is.
///
/// The four entry memo ring is the shim's: a repeated call returns the SAME
/// record, so comparing a byte slice with itself takes the fast path. Nothing
/// depends on it, and a fifth distinct string evicts the first.
const encoder = new TextEncoder();
const strBytes = [];
let strBytesNext = 0;

export function str_bytes(s) {
	for (let i = 0; i < strBytes.length; i++) {
		if (strBytes[i].s === s) return strBytes[i].r;
	}
	const bytes = Array.prototype.slice.call(encoder.encode(s));
	const record = { buf: bytes, off: 0, len: bytes.length };
	strBytes[strBytesNext] = { s, r: record };
	strBytesNext = (strBytesNext + 1) % 4;
	return record;
}

// The two below are what `for cell in board.iter()` reaches for, which is how a
// board-shaped island walks its cells. Neither has anything to do with a board:
// they are `core::slice::Iter`'s own pointer arithmetic, and the boards are
// simply the first slices an island has iterated.

/// `ptr::eq`: whether two pointers name the same place.
///
/// The iterator's stop condition -- it walks a slot record along the buffer and
/// stops when it reaches the end record. `===` answers first, so anything that
/// is not a slot record costs nothing. `len` is deliberately not compared, which
/// makes comparing a fat pointer with the thin pointer to its start free.
///
/// Not carried over from the shim's: the `_named` assertion on each side, which
/// reports a pointer offset from an address with no provenance. Only
/// `__rt.erase` and integer-to-pointer casts make one of those, and an island's
/// emission mentions neither.
export function ptr_eq(a, b) {
	if (a === b) return true;
	if (a === null || b === null || typeof a !== "object" || typeof b !== "object") return false;
	// A reference to an aggregate IS that aggregate, and two of those are equal
	// exactly when `===` said so. Without this test two unrelated records
	// compare `undefined` against `undefined` three times and every pair of them
	// is equal.
	if (!("buf" in a) || !("buf" in b)) return false;
	return a.buf === b.buf && a.off === b.off && a.sc === b.sc;
}

/// A pointer's synthetic address: a per-buffer base plus its offset in bytes.
///
/// Exported for a line NOTHING RUNS. `slice::Iter::next` has a zero-sized-element
/// half that counts down a length instead of walking a pointer, the emitter
/// resolves the choice at compile time and writes the unreachable half inside
/// `if (false)`, and `__rt.addr` is in there. jsc-build's shim check reads the
/// emitted text rather than the reachable text, and it is right to: a check that
/// tried to tell live code from dead would be a second compiler.
///
/// So this is here to be the same arithmetic the shim would have done, and a
/// caller reaching it is a bug somewhere else. Not carried over: the dangling
/// pointer alignment table, which `__rt.dangling` fills and no island calls, and
/// the `es` erased element size, which `__rt.erase` writes and no island calls.
const addrs = new WeakMap();
let addrNext = 0;

export function addr(p, size) {
	if (typeof p === "number") return p;
	if (p === null || p === undefined) return 0;
	const key = typeof p.buf === "object" && p.buf !== null ? p.buf : p;
	let base = addrs.get(key);
	if (base === undefined) {
		base = 4096 * ++addrNext;
		addrs.set(key, base);
	}
	// A slot keyed by a property name -- a field of an aggregate -- has no
	// address of its own and reports the address of the object it is a field of.
	const off = typeof p.off === "number" ? p.off : 0;
	if (p.sc !== undefined) return base + off;
	return base + off * size;
}

// NOT here, and worth knowing: a keyed `for` compiles to `__rt.keyed_row(...)`,
// which the shim also supplies (`runtime/shim.js`, `keyed_row`). No island uses
// a key today, so it is not carried here; the first one that does will find a
// `TypeError` and this note.
