// Drives the compiled falling sand island's whole loop without a browser.
//
//   node smoke/sand.mjs
//
// `check.mjs` runs the same checks, so this is part of the one smoke command.
//
// The real chunk, the real `src/island-rt.mjs`, and a recording canvas from
// `sand-runtime.mjs`, which draws nothing and writes down every operation
// performed on it. So what the island painted is a value this file asserts
// rather than a picture somebody has to look at.
//
// Stubbed: the DOM runtime, and the two browser globals the island reaches at
// global scope. `document` is stubbed HERE rather than inside the runtime stub
// because it is not a runtime name: the compiled code reads it exactly as a page
// would, and standing it up at global scope is what proves that is where it
// reached it. `setInterval` is stubbed for the same reason and one more: a clock
// that records instead of scheduling lets the check RUN the frames, so a frame
// that happened is a frame `clock_start` asked for, and nothing is left ticking
// when the process wants to exit.
//
// What is established: the browser's first render is the markup the server sent,
// down to both holes; the pointer reaches compiled Rust and the cell it lands in
// is arithmetic the island did; a frame paints exactly the cells whose value
// changed, in the colour they changed to, and nothing else; grains fall, pile,
// settle and are conserved; walls do not fall and the brush picker changes what
// is deposited; the two holes' subscriptions really are disjoint; and the whole
// simulation is a function of its inputs, because replaying it from a reset
// gives the same board cell for cell.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { compiled } from "./compiled.mjs";
import {
	asked,
	calls,
	clocks,
	documentOf,
	listeners,
	painted,
	place,
	recordInterval,
	renders,
	reset,
	TEMPLATE_HTML,
	textOf,
} from "./sand-runtime.mjs";
import { stage } from "./stage.mjs";

/// The island's own numbers, read out of its source.
///
/// Read rather than repeated, for `dashboard.mjs`'s reason: a check carrying its
/// own copy of the grid size is a check that can disagree with the island about
/// what a cell is. A shape this cannot read is an error, not a default.
function tables() {
	const source = readFileSync(join(import.meta.dirname, "..", "island", "sand.rs"), "utf8");

	const number = (what, pattern) => {
		const found = source.match(pattern);
		if (!found) throw new Error(`island/sand.rs no longer declares ${what}`);
		return Number(found[1]);
	};
	const strings = (what, pattern) => {
		const found = source.match(pattern);
		if (!found) throw new Error(`island/sand.rs no longer declares ${what}`);
		return [...found[1].matchAll(/"([^"]*)"/g)].map(match => match[1]);
	};

	return {
		cols: number("COLS", /const COLS: usize = (\d+);/),
		rows: number("ROWS", /const ROWS: usize = (\d+);/),
		scale: number("SCALE", /const SCALE: f64 = ([\d.]+);/),
		reach: number("REACH", /const REACH: i32 = (\d+);/),
		cur: number("CUR", /const CUR: f64 = ([\d.]+);/),
		frameMs: number("FRAME_MS", /const FRAME_MS: f64 = ([\d.]+);/),
		colours: strings("COLOURS", /COLOURS: \[&str; \d+\] = \[([^\]]*)\]/),
		brushes: strings("BRUSHES", /BRUSHES: \[&str; \d+\] = \[([^\]]*)\]/),
		pointer: strings("POINTER", /POINTER: \[&str; \d+\] = \[([^\]]*)\]/),
		// The two numbers the server's halves answer with, which are what the
		// browser's first render has to reach as well.
		servedGrains: number("its server-side grain count", /fn frame\([\s\S]*?Grains \{\s*count: ([\d.]+)/),
		startBrush: number("the brush signal's initial value", /signal brush = ([\d.]+);/),
	};
}

/// Where the canvas sits in the viewport during the checks.
///
/// Not the origin, so a coordinate that was never translated lands in the wrong
/// cell instead of accidentally in the right one.
const LEFT = 100;
const TOP = 50;

export async function checkSand(check) {
	const t = tables();
	const cells = t.cols * t.rows;
	const { path, source } = compiled("sand");
	console.log(`# ${path}\n`);

	const runtime = pathToFileURL(join(import.meta.dirname, "sand-runtime.mjs")).href;
	const host = pathToFileURL(join(import.meta.dirname, "..", "src", "island-rt.mjs")).href;
	const staged = stage({ "topcoat-dom": runtime, "topcoat-island-rt": host });
	const rt = await import(host);
	const { __island_sand } = await import(pathToFileURL(staged.get("sand")).href);

	// ------------------------------------------------------------- the helpers

	/// The board the island keeps between frames, as it stands now.
	///
	/// The host's own array, not a copy the check maintains: `board` hands back
	/// the record the compiled Rust indexes, so this is the cells themselves.
	const board = () => rt.board(t.cur, cells).buf.slice();

	/// The client coordinate of the middle of a cell.
	const at = (col, row) => ({
		clientX: LEFT + col * t.scale + t.scale / 2,
		clientY: TOP + row * t.scale + t.scale / 2,
	});

	/// The cells one stamp of the brush covers, as indices.
	const disc = (col, row) => {
		const out = [];
		for (let down = -t.reach; down <= t.reach; down++) {
			for (let across = -t.reach; across <= t.reach; across++) {
				if (down * down + across * across > t.reach * t.reach) continue;
				out.push((row + down) * t.cols + (col + across));
			}
		}
		return out.sort((left, right) => left - right);
	};

	/// The indices `before` and `after` disagree about.
	const changed = (before, after) => {
		const out = [];
		for (let index = 0; index < cells; index++) {
			if (before[index] !== after[index]) out.push(index);
		}
		return out;
	};

	/// The cells a run of painting covers, as indices, in the order painted.
	const paintedCells = from =>
		painted.slice(from).filter(op => op.op === "fillRect").map(op => (op.y / t.scale) * t.cols + op.x / t.scale);

	/// Stands a fresh island up over a fresh page, with the two globals it
	/// reaches installed only for as long as it takes.
	const boot = () => {
		rt.rt_reset();
		place(LEFT, TOP);
		const page = reset(String(t.servedGrains), t.brushes[t.startBrush]);
		const served = { grains: textOf(page.grains), brush: textOf(page.brush) };
		globalThis.document = documentOf(page);

		const scheduled = globalThis.setInterval;
		globalThis.setInterval = recordInterval;
		try {
			__island_sand();
		} finally {
			globalThis.setInterval = scheduled;
		}
		return { page, served };
	};

	// ------------------------------------------------------ the markup declared

	// One template, declared at module scope. Counted by matching rather than by
	// length: the shared chunk declares templates too, and which of them a chunk
	// happens to share is not this island's business.
	const templates = calls.filter(call => call.op === "template");
	check(
		"the chunk declares the markup the server sends, once and with both holes left open",
		templates.filter(entry => entry.html === TEMPLATE_HTML).length,
		1,
	);
	check("nothing in the chunk asks for a random number", source.includes("Math.random"), false);

	// ---------------------------------------------------------- the first render

	const { page, served } = boot();
	const drive = () => clocks[0].run();

	/// One pointer event, through the sink the island registered.
	let last = { col: 0, row: 0 };
	const pointer = (col, row, buttons) => {
		last = { col, row };
		listeners[0].handler({ ...at(col, row), buttons });
	};

	/// Lets go where the pointer already is, which is what a `pointerup` on the
	/// canvas is. Without it the next move is a DRAG, which deposits all the way
	/// along -- correct for a drawing surface, and not what most of the checks
	/// below are about.
	const release = () => pointer(last.col, last.row, 0);

	check("the server's node is claimed, not cloned", calls.some(call => call.op === "getNextElement"), true);
	check("the brush picker's clicks are delegated", (calls.find(call => call.op === "delegateEvents")?.events ?? []).join(), "click");

	// The parity that matters: the server rendered these two strings, and the
	// browser's first run of both holes has to produce them or the markup it
	// adopted is not the markup it would have built.
	//
	// Each hole is two claims, not one. Comparing the text alone would pass on an
	// island that never ran that hole at all, because the text it would be reading
	// is the server's own; so the render is asserted first, and only then what it
	// wrote. `renders` records the parent every insertion went through, which is
	// how a hole that ran is told apart from a hole that was left as found.
	check("the server rendered a grain count", served.grains, String(t.servedGrains));
	check("the browser ran that hole", renders.filter(parent => parent === page.grains).length > 0, true);
	check("and its first render agrees with it", textOf(page.grains), served.grains);
	check("the server rendered the starting brush", served.brush, t.brushes[t.startBrush]);
	check("the browser ran that hole too", renders.filter(parent => parent === page.brush).length > 0, true);
	check("and its first render agrees with it too", textOf(page.brush), served.brush);
	check("an empty board is a blank canvas: the first frame painted nothing", painted.length, 0);

	check("the canvas was found by selector", asked.filter(ask => ask.op === "querySelector").map(ask => ask.selector).join(), ".sand-canvas");
	check("and asked for a 2d context", asked.find(ask => ask.op === "getContext")?.kind, "2d");
	check("the island listens for every pointer event it declares", listeners.map(entry => entry.kind).join(), t.pointer.join());
	check("through one sink, not four", new Set(listeners.map(entry => entry.handler)).size, 1);
	check("one clock was opened", clocks.length, 1);
	check("at the island's own frame length", clocks[0]?.ms, t.frameMs);

	// ------------------------------------------------------------- the pointer

	// A pointer down in the middle of the grid. What crosses is a client
	// coordinate; which cell that is, is the island's own arithmetic against the
	// canvas's rectangle, which is why the canvas is not at the origin.
	const stampCol = 10;
	const stampRow = 4;
	let mark = painted.length;
	let framesBefore = renders.filter(parent => parent === page.grains).length;
	pointer(stampCol, stampRow, 1);

	const stamped = board();
	check(
		"a pointer down deposits the brush in the cell it names",
		stamped.map((value, index) => (value ? index : -1)).filter(index => index >= 0).join(),
		disc(stampCol, stampRow).join(),
	);
	check("every deposited cell is sand", new Set(disc(stampCol, stampRow).map(index => stamped[index])).size, 1);
	check("the deposit painted the cells it changed", paintedCells(mark).sort((a, b) => a - b).join(), disc(stampCol, stampRow).join());
	check(
		"in the sand colour, set once for the whole stamp",
		painted.slice(mark).filter(op => op.op === "fillStyle").map(op => op.colour).join(),
		t.colours[1],
	);
	check("the canvas rectangle was read to place it", asked.some(ask => ask.op === "getContext"), true);

	// The two holes subscribe to disjoint signals, and this is the half of that
	// claim nothing else can see.
	check(
		"a pointer move does not run the physics",
		renders.filter(parent => parent === page.grains).length,
		framesBefore,
	);

	// ---------------------------------------------------------------- a frame

	release();
	mark = painted.length;
	let brushesBefore = renders.filter(parent => parent === page.brush).length;
	const before = board();
	drive();
	const after = board();

	check("a clock tick moves every grain down one row", changed(before, after).length > 0, true);
	check(
		"a frame paints exactly the cells whose value changed",
		paintedCells(mark).sort((a, b) => a - b).join(),
		changed(before, after).join(),
	);
	check(
		"each in the colour its cell now holds",
		painted.slice(mark).filter(op => op.op === "fillRect").every(op => op.colour === t.colours[after[(op.y / t.scale) * t.cols + op.x / t.scale]]),
		true,
	);
	check(
		"the blob fell as a body, keeping its shape",
		after.map((value, index) => (value ? index : -1)).filter(index => index >= 0).join(),
		disc(stampCol, stampRow + 1).join(),
	);
	check("the grain count follows the board", textOf(page.grains), String(disc(stampCol, stampRow).length));
	check("a frame does not re-stamp", renders.filter(parent => parent === page.brush).length, brushesBefore);

	// -------------------------------------------------------------- the pile

	/// Runs frames until one paints nothing, which is the board having settled.
	const settle = limit => {
		for (let frames = 1; frames <= limit; frames++) {
			const from = painted.length;
			drive();
			if (painted.length === from) return frames;
		}
		return -1;
	};

	const grains = disc(stampCol, stampRow).length;
	const frames = settle(t.rows * 4);
	check("the pile settles rather than running for ever", frames > 0, true);
	check("nothing left the grid on the way down", board().filter(value => value === 1).length, grains);
	check("and the count the island rendered agrees", textOf(page.grains), String(grains));
	check("the pile rests on the bottom row", board().slice((t.rows - 1) * t.cols).some(value => value === 1), true);

	const settled = board();
	mark = painted.length;
	drive();
	check("a settled board paints nothing at all", painted.length, mark);
	check("and does not move", board().join(), settled.join());

	// ------------------------------------------------- a pointer that deposits nothing

	mark = painted.length;
	const untouched = board();
	pointer(30, 20, 0);
	check("a pointer with nothing held deposits nothing", board().join(), untouched.join());
	check("and paints nothing", painted.length, mark);

	// Far to the left of the canvas, so every cell the brush would cover is off
	// the grid. The clipping is compiled Rust, and this is it not being a crash.
	pointer(-10, 20, 1);
	check("a pointer off the grid deposits nothing", board().join(), untouched.join());
	check("and paints nothing", painted.length, mark);
	release();

	// ----------------------------------------------------------- the brush picker

	// Chosen with the pointer STILL HELD, which is the state a release outside
	// the canvas leaves the island in: nothing reports it, so `pdown` stays down.
	// The hole that stamps is the hole that reads the brush, so a picker that did
	// not let go first would stamp once under the pointer on every choice.
	pointer(20, 10, 1);
	const beforePick = board();
	mark = painted.length;
	framesBefore = renders.filter(parent => parent === page.grains).length;
	page.buttons[1].$$click({});
	check("the picker names the brush it chose", textOf(page.brush), t.brushes[1]);
	check("choosing a brush lets go of the pointer rather than stamping under it", board().join(), beforePick.join());
	check("so it paints nothing", painted.length, mark);
	check("and does not run the physics", renders.filter(parent => parent === page.grains).length, framesBefore);

	const wallCol = 40;
	const wallRow = 20;
	mark = painted.length;
	pointer(wallCol, wallRow, 1);
	const walled = board();
	check("the wall brush deposits walls", new Set(disc(wallCol, wallRow).map(index => walled[index])).size, 1);
	check("which are not sand", walled[disc(wallCol, wallRow)[0]] === 1, false);
	check(
		"painted in the wall colour",
		painted.slice(mark).filter(op => op.op === "fillStyle").map(op => op.colour).join(),
		t.colours[2],
	);

	drive();
	check("a wall does not fall", disc(wallCol, wallRow).map(index => board()[index]).join(), disc(wallCol, wallRow).map(() => walled[disc(wallCol, wallRow)[0]]).join());

	page.buttons[2].$$click({});
	check("the picker names the eraser too", textOf(page.brush), t.brushes[2]);
	mark = painted.length;
	pointer(wallCol, wallRow, 1);
	check("erasing empties the cells", disc(wallCol, wallRow).map(index => board()[index]).join(), disc(wallCol, wallRow).map(() => 0).join());
	check(
		"painted in the empty colour",
		painted.slice(mark).filter(op => op.op === "fillStyle").map(op => op.colour).join(),
		t.colours[0],
	);

	// ------------------------------------------------------------- determinism
	//
	// The side a blocked grain leans is a hash of its own coordinates and the
	// frame number, and the island has nowhere to keep a seed even if it wanted
	// one. So the whole simulation is a function of its inputs, and replaying it
	// from a reset has to reach the same board cell for cell. This is what says
	// the physics is compiled Rust rather than `Math.random` behind a wrapper.

	// The same moves in the same order, which is what makes the two comparable:
	// one stamp, one frame, then frames until it settles.
	const replay = boot();
	pointer(stampCol, stampRow, 1);
	release();
	drive();
	const replayFrames = settle(t.rows * 4);
	check("a replay from a reset takes the same number of frames to settle", replayFrames, frames);
	check("and reaches the same board, cell for cell", board().join(), settled.join());
	check("and renders the same count", textOf(replay.page.grains), String(grains));

	// ------------------------------------------------------------------ a drag
	//
	// One pointer event is three signal writes, and the deposit hole re-runs
	// after every one of them, not once per event. The sink closes the `down`
	// gate before it moves the coordinates, so the runs between the writes
	// deposit nothing; without that, a diagonal move stamps at the new column
	// of the OLD row on the write between the two coordinates. The board after
	// a drag is exactly the discs the pointer named, and nothing in between.
	boot();
	const sink = listeners.at(-1).handler;
	sink({ ...at(10, 4), buttons: 1 });
	sink({ ...at(14, 8), buttons: 1 });
	check(
		"a diagonal drag deposits only where the pointer was",
		board().map((value, index) => (value ? index : -1)).filter(index => index >= 0).join(),
		[...new Set([...disc(10, 4), ...disc(14, 8)])].sort((left, right) => left - right).join(),
	);

	// An open interval would keep node alive, and a claimed island would refuse
	// to set itself up in the next process that loads this module.
	rt.rt_reset();
}

if (import.meta.filename === process.argv[1]) {
	const failures = [];
	await checkSand((what, actual, expected) => {
		const ok = actual === expected;
		console.log(`${ok ? "ok  " : "FAIL"}  ${what}: ${JSON.stringify(actual)}`);
		if (!ok) failures.push(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
	});
	console.log("");
	if (failures.length) {
		for (const failure of failures) console.log(failure);
		console.log(`${failures.length} failed`);
		process.exit(1);
	}
	console.log("every sand check passed");
}
