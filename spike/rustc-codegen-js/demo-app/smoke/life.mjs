// Drives the compiled Game of Life island without a browser.
//
//   node smoke/life.mjs
//
// `check.mjs` runs the same checks, so this is part of the one smoke command.
//
// WHAT IS REAL HERE
//
// The compiled chunk and `src/island-rt.mjs` are the ones the app serves, so the
// board really is a JavaScript array the host keeps and the rule really is
// compiled Rust reading and writing it. What is replaced is the DOM runtime
// (`life-runtime.mjs`, a node graph parsed from the chunk's own template) and
// `setInterval`, which is captured rather than allowed to run: a check that waited
// out real 50ms ticks could say that the board advanced but not that it advanced
// on the ticks the speed asked for, which is most of what the driver does.
//
// SERVER AND CLIENT AGREEING ON THE FIRST RENDER
//
// The classic silent failure for an island is a first render that does not match
// the markup it is adopting, because nothing goes wrong: the runtime rebuilds the
// DOM and the page looks right. Here the shape is one `view!` body compiled twice
// and cannot differ; what is computed twice is WHICH CELLS ARE ALIVE, once by the
// server mapping `initial_alive` over `0..640` and once by the client stamping the
// same function into the board.
//
// So the opening board is written down once, as `OPENING_BOARD` in
// `island/life.rs`, and asserted at both ends: the test at the foot of that file
// measures the server half against it, and this file reads the same literal out of
// the source and measures the compiled client against it. A change to
// `initial_alive` that reached one half and not the other fails one of the two.
// The markup the server has to send is pinned here as `ISLAND_HTML`, which is the
// template the chunk declares -- the client's own statement of what it expects to
// find already in the page.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { calls, find, reset } from "./life-runtime.mjs";
import { stage } from "./stage.mjs";

/// The markup the server sends for this island, with every hole left open.
///
/// The chunk declares this string, so it is what the client will walk; the server
/// renders the same view body, so it is what the server has to have written. A
/// change to the view that moves a node changes this line, which is the point:
/// the walk that reaches the grid counts the text nodes on the way.
const ISLAND_HTML =
	'<div class="island life">'
	+ '<p class="life-readout">generation <span class="life-generation"></span>'
	+ ' · population <span class="life-population"></span></p>'
	+ '<div class="life-grid"></div>'
	+ '<div class="island-controls">'
	+ '<button class="island-step" type="button">play / pause</button>'
	+ '<button class="island-step" type="button">step</button>'
	+ '<button class="island-step" type="button">reset</button>'
	+ '<button class="island-step" type="button">clear</button>'
	+ '<button class="island-step" type="button">random</button>'
	+ "</div>"
	+ '<label class="life-speed">speed '
	+ '<input class="life-rate" type="range" min="1" max="20" value="10">'
	+ "</label>"
	+ "</div>";

/// How wide the board is, so a check can talk in cells rather than in indices.
const WIDTH = 32;

/// How many cells there are.
const CELLS = 640;

/// The opening board, read out of the island's own source rather than repeated.
///
/// Repeating it would give this file a second opinion about what the server
/// renders, and two opinions cannot disagree usefully. Read from the one literal
/// the Rust test also measures, a failure here means the CLIENT drifted.
function openingBoard() {
	const source = readFileSync(join(import.meta.dirname, "..", "island", "life.rs"), "utf8");
	const found = source.match(/const OPENING_BOARD: &str = "([\d,]*)";/);
	if (!found) throw new Error("island/life.rs no longer writes down OPENING_BOARD");
	return found[1];
}

/// The cells the grid is currently showing as alive, by index.
const alive = grid =>
	grid.children.filter(cell => cell.attributes.class === "cell on").map(cell => Number(cell.attributes.value));

/// Those of them in the board's top left corner, where the glider is and where
/// nothing else reaches within the few generations this file steps.
const corner = indices => indices.filter(index => index % WIDTH < 10 && Math.floor(index / WIDTH) < 10);

export async function checkLife(check) {
	const host = pathToFileURL(join(import.meta.dirname, "..", "src", "island-rt.mjs")).href;
	const staged = stage({
		"topcoat-dom": pathToFileURL(join(import.meta.dirname, "life-runtime.mjs")).href,
		"topcoat-island-rt": host,
	});

	// The boards and the claim outlive a call, so a second island in this process
	// would find this one's. Cleared going in as well as coming out.
	const rt = await import(host);
	rt.rt_reset();
	reset();

	const { __island_life } = await import(pathToFileURL(staged.get("life")).href);

	// The clock, captured rather than started. `clock_start` reaches `setInterval`
	// at global scope, which is what makes this possible and what a browser does
	// too; the globals go back immediately afterwards so that the checks that run
	// after this one find node's own.
	const timers = globalThis.setInterval;
	const clears = globalThis.clearInterval;
	let clock = null;
	let period = null;
	globalThis.setInterval = (fn, ms) => {
		clock = fn;
		period = ms;
		return "life-clock";
	};
	globalThis.clearInterval = () => {};

	let root;
	try {
		root = __island_life();
	} finally {
		globalThis.setInterval = timers;
		globalThis.clearInterval = clears;
	}

	const grid = find(root, "life-grid");
	const generation = find(root, "life-generation");
	const population = find(root, "life-population");
	const [play, stepOnce, restore, clear] = find(root, "island-controls").childNodes;
	const rate = find(root, "life-rate");

	/// One tick of the clock the island started.
	const tick = () => clock();

	// ------------------------------------------------------ what the chunk is

	const template = calls.find(call => call.op === "template" && call.html.includes("life-grid"));
	check("the chunk declares the island's markup", template?.html, ISLAND_HTML);
	check(
		"the page has to capture both the events the island uses",
		(calls.find(call => call.op === "delegateEvents")?.events ?? []).join(),
		"click,input",
	);
	check("the island started one clock", period !== null, true);
	// Fixed, with the speed picking a stride over it: the page lends a clock that
	// can be started and nothing that stops one.
	check("at a fixed 20 Hz", period, 50);

	// ---------------------------------------------------- the opening render

	check("the grid is one button per cell", grid.children.length, CELLS);
	check(
		"every cell carries its own index, so one handler can tell them apart",
		grid.children.every((cell, at) => cell.attributes.value === String(at)),
		true,
	);
	check("the cells alive are the ones the server rendered", alive(grid).join(), openingBoard());
	check("nothing has stepped", generation.text, "0");
	check("and the readout counts the board", population.text, String(openingBoard().split(",").length));

	// --------------------------------------------- a click names its own cell

	// Pattern B, the shape a grid uses: one handler on the container, each cell a
	// `<button value=...>`, and the handler reading `event.target.value`. See
	// `examples/dom-tests/23_for_row_handlers.rs` for the per-row alternative,
	// which works and costs one closure per cell.
	grid.$$click({ target: { value: "33" } });
	check("a click turns its own cell on", alive(grid).includes(33), true);
	check("and leaves every other cell alone", alive(grid).length, 11);
	check("a hand edit is not a generation", generation.text, "0");
	// The readout is the driver's hole and the driver does not read `edits`, so it
	// is one tick behind a hand edit. That is the price of the two subscriptions
	// being separate, and it is asserted rather than hidden.
	check("the readout has not re-run yet", population.text, "10");

	tick();
	check("a tick that is not due steps nothing", generation.text, "0");
	check("but the readout re-runs and counts the edit", population.text, "11");

	grid.$$click({ target: { value: "33" } });
	check("clicking the same cell again turns it off", alive(grid).join(), openingBoard());

	// A click on the gap between cells reports the grid, which has no value.
	// `target_value` reads that back as the empty string, which is why "no cell"
	// is a case the island has rather than an index it invents.
	grid.$$click({ target: {} });
	check("a click on no cell changes nothing", alive(grid).join(), openingBoard());

	// ------------------------------------------------------------- the clock

	// The board the clock is about to step, which the two checks above have just
	// put back to the one the server sent.
	const opening = alive(grid).join();

	tick();
	check("the second tick is the one the speed asks for", generation.text, "1");
	check("the readout is the population of the board it just wrote", population.text, String(alive(grid).length));
	// A counter that moves is not a board that moved. The generation above says a
	// step was ATTEMPTED; this says the grid the page shows is no longer the one it
	// was, so the tick reached the rule and the swap and the loop, not just the
	// signal. What it stepped TO is settled further down, against the step button.
	check("and the grid it shows is not the board it started from", alive(grid).join() === opening, false);

	/// The board one clock tick reached from the opening board.
	const ticked = alive(grid).join();

	rate.$$input({ target: { value: "20" } });
	// The driver reads `speed`, so moving the slider re-runs it mid-tick. It does
	// not step again: `stepped` holds the tick the last step happened at, and the
	// re-run finds it equal. This is the whole of why writing `generation` from
	// inside an effect that reads it terminates.
	check("moving the slider does not step the board", generation.text, "1");
	rate.$$input({ target: { value: "not a number" } });
	check("nor does a slider value that is not one", generation.text, "1");

	tick();
	check("at twenty steps a second every tick is due", generation.text, "2");

	play.$$click({});
	check("pausing does not step the board", generation.text, "2");
	tick();
	tick();
	tick();
	check("and a paused board ignores the clock", generation.text, "2");

	play.$$click({});
	// Resuming steps at once rather than waiting for the next due tick: the
	// driver's condition is about which tick it is, not about how the tick was
	// reached. Asserted because it is visible, not because it is wanted.
	check("resuming steps at once", generation.text, "3");
	play.$$click({});

	// ------------------------------------------------------- the simulation

	restore.$$click({});
	check("reset puts the opening board back", alive(grid).join(), openingBoard());
	check("without pretending a generation happened", generation.text, "3");

	const before = corner(alive(grid));
	stepOnce.$$click({});
	// The clock and the step button are two ways into one rule, and both start
	// here: `ticked` is where a tick left the opening board, and this is where the
	// button leaves it. A clock that advanced the counter without stepping the
	// board would have left `ticked` as the opening board, and this would be the
	// check that named it.
	check("a clock tick reached the same board a step button does", alive(grid).join(), ticked);
	for (let at = 0; at < 3; at++) stepOnce.$$click({});
	check("four steps are four generations", generation.text, "7");
	// The one property of Life that says the rule is right rather than merely
	// busy: a glider is back in its own shape after four generations, one cell
	// down and one cell to the right of where it started. Nothing else on the
	// board reaches this corner in four generations.
	check(
		"a glider has moved one cell diagonally",
		corner(alive(grid)).join(),
		before.map(index => index + WIDTH + 1).join(),
	);

	clear.$$click({});
	check("clear kills every cell", alive(grid).length, 0);
	tick();
	check("and the readout follows on the next tick", population.text, "0");

	restore.$$click({});
	check("reset works from an empty board too", alive(grid).join(), openingBoard());

	// The boards, the claim and the clock the host is holding. Without this a
	// second island in this process would find this one's board, and in a browser
	// nothing would ever call it.
	rt.rt_reset();
}

if (import.meta.filename === process.argv[1]) {
	const failures = [];
	await checkLife((what, actual, expected) => {
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
	console.log("every life check passed");
}
