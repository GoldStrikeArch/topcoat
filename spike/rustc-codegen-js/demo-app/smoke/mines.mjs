// Drives the compiled Minesweeper island's whole game without a browser.
//
//   node smoke/mines.mjs
//
// `check.mjs` runs the same checks, so this is part of the one smoke command.
//
// WHAT IS REAL HERE
//
// The compiled chunk and `src/island-rt.mjs` are the ones the app serves, so the
// placement, the flood fill, the flag toggle, the win test and the restart are
// all compiled Rust doing the work. What is replaced is the DOM runtime
// (`mines-runtime.mjs`, a node graph parsed out of the chunk's own templates).
//
// The field is the HOST'S OWN ARRAY -- `island-rt`'s `board(5, 121)` -- so this
// file can read what the compiled code wrote rather than inferring it from the
// markup. Every rule it then checks is recomputed here, in JavaScript, from that
// array: where the mines are allowed to be, what each revealed cell should say,
// and how far a flood should have spread. Nothing is compared against a recording
// of a previous run.
//
// PARITY. The island's own tables are read out of `island/mines.rs` rather than
// repeated, and the first render is compared against the whole markup the server
// sends, built from those tables. The other half of that claim is in
// `island/mines.rs` itself: `mines_island::tests` asserts the same numbers from
// the server's side, over the same shared code.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import * as runtime from "./mines-runtime.mjs";
import { stage } from "./stage.mjs";

/// The seed the field is laid from, and one that is not it.
///
/// Two, because "the mines came from the seed" is only shown by a second seed
/// putting them somewhere else. Both are written down rather than taken from a
/// clock: a check whose input changes every run cannot say what it measured.
const SEED = 20_260_730;
const OTHER_SEED = 7;

/// The cell every game below opens on. The middle of the field, so the nine
/// cells kept clear around it are all real cells.
const OPENING = 40;

/// The island's own tables, read out of its source.
///
/// Read rather than repeated, for `dashboard.mjs`'s reason: the two halves of the
/// island already share one set of tables, and a check carrying a third copy is a
/// check that can agree with neither. A shape this cannot read is an error.
function tables() {
	const source = readFileSync(join(import.meta.dirname, "..", "island", "mines.rs"), "utf8");

	const one = (name, pattern) => {
		const found = source.match(pattern);
		if (!found) throw new Error(`island/mines.rs no longer declares ${name}`);
		return found[1];
	};
	const number = name => Number(one(name, new RegExp(`const ${name}: (?:i32|u8|f64) = ([^;]+);`)));
	// Rust spells a mark as an escape so the source stays ascii; the compiled
	// chunk carries the character itself, which is what the check compares.
	const unescape = text => text.replaceAll(/\\u\{([0-9a-fA-F]+)\}/g, (_, code) =>
		String.fromCodePoint(Number.parseInt(code, 16)));
	const string = name => unescape(one(name, new RegExp(`const ${name}: &str = "([^"]*)";`)));
	const list = name =>
		[...one(name, new RegExp(`const ${name}: \\[&str; \\d+\\] = \\[([^\\]]*)\\]`))
			.matchAll(/"([^"]*)"/g)].map(match => match[1]);

	const columns = number("COLUMNS");
	const rows = number("ROWS");
	return {
		columns,
		rows,
		cells: columns * rows,
		mineCount: number("MINE_COUNT"),
		board: number("BOARD"),
		bits: { mine: number("MINE"), revealed: number("REVEALED"), flagged: number("FLAGGED") },
		counts: list("COUNTS"),
		open: list("OPEN"),
		hidden: string("HIDDEN"),
		flaggedClass: string("FLAGGED_CLASS"),
		blown: string("BLOWN"),
		blank: string("BLANK"),
		flagMark: string("FLAG_MARK"),
		mineMark: string("MINE_MARK"),
		playing: [string("PLAYING"), string("PLAYING_WORD")],
		won: [string("WON"), string("WON_WORD")],
		lost: [string("LOST"), string("LOST_WORD")],
	};
}

/// Runs every minesweeper check through `check(what, actual, expected)`.
export async function checkMines(check) {
	const t = tables();
	const host = pathToFileURL(join(import.meta.dirname, "..", "src", "island-rt.mjs")).href;
	const staged = stage({
		"topcoat-dom": pathToFileURL(join(import.meta.dirname, "mines-runtime.mjs")).href,
		"topcoat-island-rt": host,
	});

	// The host keeps the field between calls, so a second run of this file in one
	// process would start on the last one's game.
	const rt = await import(host);
	rt.rt_reset();

	const { __island_mines } = await import(pathToFileURL(staged.get("mines")).href);

	// ------------------------------------------------------------ the chunk

	const templates = runtime.calls.filter(call => call.op === "template");
	check("the chunk declares three templates: the island, a note and a cell", templates.length, 3);

	const island = templates.find(template => template.html.startsWith('<div class="island mines">'));
	check(
		"the island's template is the markup the server sends, with the reactive holes empty",
		island?.html,
		'<div class="island mines"><p class="mines-status"></p><div class="mines-grid"></div>'
			+ '<div class="island-controls">'
			+ '<button class="island-step" type="button">restart</button>'
			+ "</div></div>",
	);
	check(
		"a cell is a button carrying its own index, so one handler can serve the grid",
		templates.find(template => template.html.startsWith("<button"))?.html,
		'<button type="button"></button>',
	);
	check(
		"the status line is one span holding the count and the word",
		templates.find(template => template.html.startsWith("<span"))?.html,
		'<span><span class="mines-left"></span><span class="mines-word"></span></span>',
	);
	// ------------------------------------------------------- the first render

	let page = runtime.reset(island.html);
	check(
		"the entry point returns the server's own node, rather than a clone",
		__island_mines(SEED) === page,
		true,
	);
	// Registered by the entry point rather than at module scope, so this is asked
	// after the island has run.
	check(
		"both the left and the right click are delegated",
		(runtime.calls.find(call => call.op === "delegateEvents")?.events ?? []).join(),
		"click,contextmenu",
	);

	/// The three places the view puts things, walked the way the emitted code does.
	const status = () => page.firstChild;
	const grid = () => status().nextSibling;
	const restart = () => grid().nextSibling.firstChild;

	/// The field, as the host holds it.
	const field = () => {
		const held = rt.board(t.board, t.cells + 1);
		return held.buf.slice(held.off, held.off + t.cells);
	};

	/// The cells of the field that have a bit set.
	const having = bit => field().flatMap((cell, index) => (cell & bit ? [index] : []));

	/// The eight cells around `at`, without wrapping.
	const around = at => {
		const x = at % t.columns;
		const y = Math.floor(at / t.columns);
		const near = [];
		for (const [dx, dy] of [[-1, -1], [0, -1], [1, -1], [-1, 0], [1, 0], [-1, 1], [0, 1], [1, 1]]) {
			const nx = x + dx;
			const ny = y + dy;
			if (nx < 0 || nx >= t.columns || ny < 0 || ny >= t.rows) continue;
			near.push(ny * t.columns + nx);
		}
		return near;
	};

	/// How many mines touch `at`, recomputed here rather than read off the markup.
	const count = at => around(at).filter(near => field()[near] & t.bits.mine).length;

	/// What the grid currently draws.
	const drawn = () =>
		grid().children.map(cell => ({
			value: cell.attributes.value,
			class: cell.attributes.class,
			label: cell.text ?? "",
		}));

	/// What the status line currently reads.
	const note = () => {
		const shown = status().children[0];
		return {
			class: shown.attributes.class,
			left: shown.children[0].text,
			word: shown.children[1].text,
		};
	};

	/// The whole markup the server sends for an untouched field.
	///
	/// Built from the island's own tables, so it moves when they do. This is the
	/// parity claim: the browser's first render, before any click, is the document
	/// the server wrote -- which is what makes the hydration an adoption rather
	/// than a rebuild.
	const untouched = `<div class="island mines">`
		+ `<p class="mines-status"><span class="${t.playing[0]}">`
		+ `<span class="mines-left">${t.mineCount}</span>`
		+ `<span class="mines-word">${t.playing[1]}</span></span></p>`
		+ `<div class="mines-grid">`
		+ [...Array(t.cells).keys()]
			.map(index => `<button class="${t.hidden}" type="button" value="${index}">${t.blank}</button>`)
			.join("")
		+ `</div><div class="island-controls">`
		+ `<button class="island-step" type="button">restart</button></div></div>`;

	check("the first render is the whole document the server sends", runtime.html(page), untouched);
	check("rendering the field lays no mines", field().filter(cell => cell !== 0).length, 0);

	// --------------------------------------------------------------- clicking

	/// A left click on the grid, as a delegated event whose target is a cell.
	const click = value => grid().$$click({ target: { value: String(value) } });

	/// A right click, which also has to be stopped from opening a menu.
	let prevented = 0;
	const rightClick = value =>
		grid().$$contextmenu({
			target: { value: String(value) },
			preventDefault: () => {
				prevented += 1;
			},
		});

	// A click that lands between the buttons has no value to read. `target_value`
	// answers the empty string for it rather than undefined, which is the whole
	// reason it is spelled `|| ""`, and this is the island telling "no cell" from
	// a cell.
	grid().$$click({ target: {} });
	check("a click on the grid itself is not a click on a cell", runtime.html(page), untouched);

	// ------------------------------------------------- the first click is safe

	click(OPENING);
	const mines = having(t.bits.mine);
	check("the first reveal lays the mines", mines.length, t.mineCount);
	check(
		"none of them within one cell of the click, so an opening move cannot lose",
		mines.filter(mine => mine === OPENING || around(OPENING).includes(mine)).length,
		0,
	);
	check("which makes the opening cell a zero", count(OPENING), 0);
	check("and the game is still on", note().word, t.playing[1]);

	const revealed = having(t.bits.revealed);
	check("a zero opens a region rather than a cell", revealed.length > around(OPENING).length, true);
	check("nothing revealed is a mine", revealed.filter(at => field()[at] & t.bits.mine).length, 0);
	// The flood's own rule, recomputed: a revealed zero must have pulled in every
	// cell it touches. A fill that stopped early passes every count check and
	// fails this one.
	check(
		"and every zero it reached brought its neighbours with it",
		revealed
			.filter(at => count(at) === 0)
			.flatMap(at => around(at))
			.filter(near => !(field()[near] & t.bits.revealed)).length,
		0,
	);

	// What the grid draws, recomputed cell by cell from the host's array.
	const expected = () =>
		field().map((cell, index) => {
			if (cell & t.bits.revealed) {
				return cell & t.bits.mine
					? { value: String(index), class: t.blown, label: t.mineMark }
					: { value: String(index), class: t.open[count(index)], label: t.counts[count(index)] };
			}
			if (cell & t.bits.flagged) {
				return { value: String(index), class: t.flaggedClass, label: t.flagMark };
			}
			return { value: String(index), class: t.hidden, label: t.blank };
		});

	check(
		"every cell draws the state the field holds, with its count as its label",
		JSON.stringify(drawn()),
		JSON.stringify(expected()),
	);

	// --------------------------------------------------------------- flagging

	const toFlag = field().findIndex(cell => cell === 0);
	rightClick(toFlag);
	check("a right click is stopped from opening a menu", prevented, 1);
	check("and flags the cell instead", (field()[toFlag] & t.bits.flagged) !== 0, true);
	check("without revealing it", (field()[toFlag] & t.bits.revealed) !== 0, false);
	// ONE off the count and not two. Both loops of the view call `apply`, so a
	// missing token guard toggles the flag twice and this reads 15 again.
	check("one flag comes off the count", note().left, String(t.mineCount - 1));
	check("and the cell shows it", drawn()[toFlag].class, t.flaggedClass);
	check("with the mark the table names", drawn()[toFlag].label, t.flagMark);

	click(toFlag);
	check("a flagged cell is not revealed by a left click", (field()[toFlag] & t.bits.revealed) !== 0, false);

	rightClick(toFlag);
	check("a second right click takes the flag off", (field()[toFlag] & t.bits.flagged) !== 0, false);
	check("and gives the count back", note().left, String(t.mineCount));

	// ---------------------------------------------------------------- restart

	restart().$$click({});
	check("restart clears every cell", field().filter(cell => cell !== 0).length, 0);
	check("and puts back the document the server sent", runtime.html(page), untouched);

	// ------------------------------------------------------- the same seed again

	click(OPENING);
	check(
		"the same seed opened at the same cell lays the same mines",
		having(t.bits.mine).join(),
		mines.join(),
	);

	// ------------------------------------------------------------------ losing

	const [firstMine] = mines;
	click(firstMine);
	check("clicking a mine loses", note().word, t.lost[1]);
	check("and says so", note().class, t.lost[0]);
	check("the whole field comes up", having(t.bits.mine).every(at => field()[at] & t.bits.revealed), true);
	check("every mine drawn as one", drawn()[firstMine].class, t.blown);
	check("with the mark the table names", drawn()[firstMine].label, t.mineMark);

	// -------------------------------------------- another seed, and a won game

	rt.rt_reset();
	page = runtime.reset(island.html);
	__island_mines(OTHER_SEED);
	check("a fresh island renders the server's document again", runtime.html(page), untouched);

	click(OPENING);
	const others = having(t.bits.mine);
	check("another seed lays another field", others.join() === mines.join(), false);
	check("of the same size", others.length, t.mineCount);

	// Every cell that is not a mine, which is the only way to win.
	for (let index = 0; index < t.cells; index++) {
		if (!(field()[index] & t.bits.mine)) click(index);
	}
	check("revealing every safe cell wins", note().word, t.won[1]);
	check("and says so", note().class, t.won[0]);
	check("without any mine having gone off", having(t.bits.mine).some(at => field()[at] & t.bits.revealed), false);
	check("the mines are all that is left hidden", having(t.bits.revealed).length, t.cells - t.mineCount);

	// An open interval would keep node alive; this island opens none, and calling
	// it anyway is what keeps the next suite's field from being this one's.
	rt.rt_reset();
}

if (import.meta.filename === process.argv[1]) {
	const failures = [];
	await checkMines((what, actual, expected) => {
		const ok = actual === expected;
		console.log(`${ok ? "ok  " : "FAIL"}  ${what}: ${JSON.stringify(actual)}`);
		if (!ok) failures.push(what);
	});
	console.log("");
	if (failures.length) {
		console.log(`${failures.length} failed`);
		process.exit(1);
	}
	console.log("every minesweeper check passed");
}
