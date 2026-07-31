// Drives the compiled benchmark island without a browser.
//
//   node smoke/bench.mjs
//
// `check.mjs` runs the same checks, so this is part of the one smoke command.
//
// WHAT IS REAL HERE
//
// The compiled chunk and `src/island-rt.mjs` are the ones the app serves, so the
// rows really are a JavaScript array the host keeps and every rule -- which words
// a label is made of, which rows a bang lands on, when a swap is legal -- really
// is compiled Rust. What is replaced is the DOM runtime (`bench-runtime.mjs`, a
// node graph parsed from the chunk's own templates) and `Math.random`, which is
// seeded: the benchmark's draw is `Math.round(Math.random() * 1000) % max`, so a
// known sequence of draws makes the labels a thing a check can write down rather
// than describe.
//
// WHY THE TEMPLATES ARE PINNED IN FULL
//
// This island's markup is a CONTRACT. The js-framework-benchmark harness presses
// six buttons by id, reads the rows out of `tbody`, clicks the second cell's
// anchor to select and the third cell's anchor to delete, and decides whether an
// implementation is keyed by looking at the rows it finds. An island that works
// perfectly with the wrong class on a cell is an entry the harness rejects or,
// worse, measures wrongly. So both templates are written out below exactly, and a
// change to either has to be made here too, on purpose.
//
// WHAT THE ROW COUNT MEANS
//
// `built.nodes` counts template instantiations, and the checks assert it after a
// selection. One signal drives the whole table, so selecting a row rebuilds every
// row: a thousand nodes for a class change on one of them. That is this entry's
// honest cost and it is asserted rather than mentioned.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { built, byId, calls, find, reset } from "./bench-runtime.mjs";
import { stage } from "./stage.mjs";

/// The markup the server sends for this island, with the table left empty.
///
/// The chunk declares this string, so it is what the client walks; the server
/// renders the same view body, so it is what the server has to have written. It
/// is also the benchmark's page: six ids, the table's three classes, and the
/// preload icon that gets the glyphicon font fetched before a row needs it.
const ISLAND_HTML =
	'<div class="container"><div class="jumbotron"><div class="row">'
	+ '<div class="col-md-6"><h1>Topcoat-island</h1></div>'
	+ '<div class="col-md-6"><div class="row">'
	+ '<div class="col-sm-6 smallpad">'
	+ '<button type="button" class="btn btn-primary btn-block" id="run">Create 1,000 rows</button>'
	+ "</div>"
	+ '<div class="col-sm-6 smallpad">'
	+ '<button type="button" class="btn btn-primary btn-block" id="runlots">Create 10,000 rows</button>'
	+ "</div>"
	+ '<div class="col-sm-6 smallpad">'
	+ '<button type="button" class="btn btn-primary btn-block" id="add">Append 1,000 rows</button>'
	+ "</div>"
	+ '<div class="col-sm-6 smallpad">'
	+ '<button type="button" class="btn btn-primary btn-block" id="update">Update every 10th row</button>'
	+ "</div>"
	+ '<div class="col-sm-6 smallpad">'
	+ '<button type="button" class="btn btn-primary btn-block" id="clear">Clear</button>'
	+ "</div>"
	+ '<div class="col-sm-6 smallpad">'
	+ '<button type="button" class="btn btn-primary btn-block" id="swaprows">Swap Rows</button>'
	+ "</div>"
	+ "</div></div></div></div>"
	+ '<table class="table table-hover table-striped test-data"><tbody></tbody></table>'
	+ '<span class="preloadicon glyphicon glyphicon-remove" aria-hidden="true"></span>'
	+ "</div>";

/// One row, which is the part of the contract the harness reads most closely.
const ROW_HTML =
	"<tr>"
	+ '<td class="col-md-1"></td>'
	+ '<td class="col-md-4"><a></a></td>'
	+ '<td class="col-md-1"><a><span class="glyphicon glyphicon-remove" aria-hidden="true"></span></a></td>'
	+ '<td class="col-md-6"></td>'
	+ "</tr>";

/// The three word tables, read out of the island's own source rather than
/// repeated.
///
/// Repeating them would give this file a second opinion about what a label is,
/// and two opinions cannot disagree usefully. Read from the arrays the compiled
/// code draws from, a failure below means the DRAW changed.
function words() {
	const source = readFileSync(join(import.meta.dirname, "..", "island", "bench.rs"), "utf8");
	const table = name => {
		const found = source.match(new RegExp(`const ${name}: \\[&str; \\d+\\] = \\[([^\\]]*)\\];`));
		if (!found) throw new Error(`island/bench.rs no longer writes down ${name}`);
		return [...found[1].matchAll(/"([^"]*)"/g)].map(([, word]) => word);
	};
	return { adjectives: table("ADJECTIVES"), colours: table("COLOURS"), nouns: table("NOUNS") };
}

/// The seeded generator, which hands out 0.000, 0.001, 0.002 and so on.
///
/// Chosen so that `Math.round(random() * 1000)` is the draw's own number: the
/// n-th draw of a batch answers `n % 1000`, and the word it picks is that modulo
/// the table's length. That makes an expected label something this file computes
/// from the island's own tables rather than a magic string.
const draws = { at: 0 };

/// The label the row at `index` of a batch gets, given the tables.
function expectedLabel({ adjectives, colours, nouns }, index) {
	const pick = (table, draw) => table[(draw % 1000) % table.length];
	return `${pick(adjectives, index * 3)} ${pick(colours, index * 3 + 1)} ${pick(nouns, index * 3 + 2)}`;
}

/// The rows the table is showing.
const shown = table => table.childNodes[0].children;

/// What one rendered row says.
const cellId = row => row.childNodes[0].text;
const cellLabel = row => row.childNodes[1].childNodes[0].text;
const rowClass = row => row.attributes.class ?? "";

/// The two anchors a row is worked by: the label selects, the icon deletes.
const selectAnchor = row => row.childNodes[1].childNodes[0];
const removeAnchor = row => row.childNodes[2].childNodes[0];

/// Whether every rendered row says what the store says.
///
/// The whole table rather than a sample: the one failure this island could have
/// that nothing else would catch is a row drawn from the wrong index, and a
/// sample of three would miss it.
function matchesStore(rt, table) {
	const rows = shown(table);
	if (rows.length !== rt.bench_len()) return false;
	return rows.every(
		(row, at) =>
			cellId(row) === String(rt.bench_id(at))
			&& cellLabel(row) === rt.bench_label(at)
			&& rowClass(row) === rt.bench_class(at),
	);
}

export async function checkBench(check) {
	const host = pathToFileURL(join(import.meta.dirname, "..", "src", "island-rt.mjs")).href;
	const staged = stage({
		"topcoat-dom": pathToFileURL(join(import.meta.dirname, "bench-runtime.mjs")).href,
		"topcoat-island-rt": host,
	});

	// The rows and the id counter outlive a call, so a second run in this process
	// would find this one's. Cleared going in as well as coming out.
	const rt = await import(host);
	rt.bench_reset();
	reset();

	const { __island_bench } = await import(pathToFileURL(staged.get("bench")).href);

	// The benchmark's own draw, made repeatable. Restored immediately after the
	// island has been driven so the checks that follow find node's own.
	const random = Math.random;
	Math.random = () => (draws.at++ % 1000) / 1000;

	/// Starts a batch, so the labels it makes are the ones `expectedLabel` names.
	const seed = () => {
		draws.at = 0;
	};

	try {
		const table = words();
		const root = __island_bench();
		const rows = find(root, "test-data");

		/// Presses one of the six buttons.
		const press = id => byId(root, id).$$click({});

		// ------------------------------------------------------ what the chunk is

		const declared = calls.filter(call => call.op === "template").map(call => call.html);
		check("the chunk declares the benchmark's page", declared.includes(ISLAND_HTML), true);
		check("and the row shape the harness reads", declared.includes(ROW_HTML), true);
		check(
			"the page only has to capture clicks",
			(calls.find(call => call.op === "delegateEvents")?.events ?? []).join(),
			"click",
		);

		// ---------------------------------------------------- the opening render

		check("the server sends an empty table", shown(rows).length, 0);
		check("and the store agrees", rt.bench_len(), 0);
		for (const id of ["run", "runlots", "add", "update", "clear", "swaprows"]) {
			check(`the ${id} button is there and answers a click`, typeof byId(root, id)?.$$click, "function");
		}

		// --------------------------------------------------------- creating rows

		seed();
		press("run");
		check("run makes a thousand rows", rt.bench_len(), 1000);
		check("and draws them all", shown(rows).length, 1000);
		check("ids start at one", cellId(shown(rows)[0]), "1");
		check("and run to a thousand", cellId(shown(rows)[999]), "1000");
		// The draw order is adjective, colour, noun, and the words come from the
		// island's own tables. Written out as well as computed, because the point
		// of the seeded generator is that the answer can be read.
		check("a label is three words in the benchmark's order", cellLabel(shown(rows)[0]), "pretty yellow house");
		check("computed from the island's own tables", cellLabel(shown(rows)[0]), expectedLabel(table, 0));
		check("and the next row draws the next three", cellLabel(shown(rows)[1]), expectedLabel(table, 1));
		check("every row shows what the store holds", matchesStore(rt, rows), true);
		check("nothing is selected yet", shown(rows).filter(row => rowClass(row) === "danger").length, 0);

		// ------------------------------------------------------------- selecting

		built.nodes = 0;
		selectAnchor(shown(rows)[2]).$$click({});
		check("selecting a row marks it", rowClass(shown(rows)[2]), "danger");
		check("and marks only it", shown(rows).filter(row => rowClass(row) === "danger").length, 1);
		check("the store holds the id rather than a flag", rt.bench_class(2), "danger");
		// The honest cost of one signal over the whole table: a class change on one
		// row rebuilt every row. See the header.
		check("and the table was rebuilt row by row to say so", built.nodes, 1000);

		selectAnchor(shown(rows)[5]).$$click({});
		check("selecting another row moves the mark", rowClass(shown(rows)[5]), "danger");
		check("and takes it off the first", rowClass(shown(rows)[2]), "");

		// -------------------------------------------------------------- deleting

		const second = cellId(shown(rows)[1]);
		removeAnchor(shown(rows)[0]).$$click({});
		check("deleting a row takes it out of the store", rt.bench_len(), 999);
		check("and out of the table", shown(rows).length, 999);
		check("the row after it moves up", cellId(shown(rows)[0]), second);
		// The selection is an id, so it stays on the row it was on rather than on
		// the position that row happened to have.
		check("the selected row keeps its mark under its new index", rowClass(shown(rows)[4]), "danger");
		check("every row still shows what the store holds", matchesStore(rt, rows), true);

		// ------------------------------------------------------------ appending

		press("add");
		check("append adds a thousand to what is there", rt.bench_len(), 1999);
		check("ids carry on rather than restarting", cellId(shown(rows)[1998]), "2000");
		check("and the table drew them", shown(rows).length, 1999);

		// -------------------------------------------------------------- updating

		const before = cellLabel(shown(rows)[1]);
		press("update");
		check("update marks the first row", cellLabel(shown(rows)[0]).endsWith(" !!!"), true);
		check("and every tenth after it", cellLabel(shown(rows)[10]).endsWith(" !!!"), true);
		check("and leaves the ones between alone", cellLabel(shown(rows)[1]), before);
		check(
			"which is one row in ten",
			shown(rows).filter(row => cellLabel(row).endsWith(" !!!")).length,
			200,
		);
		press("update");
		check("a second update marks the same rows again", cellLabel(shown(rows)[0]).endsWith(" !!! !!!"), true);

		// -------------------------------------------------------------- swapping

		const low = cellId(shown(rows)[1]);
		const high = cellId(shown(rows)[998]);
		const between = cellId(shown(rows)[2]);
		press("swaprows");
		check("swap exchanges the second row with the nine hundred and ninety-ninth", cellId(shown(rows)[1]), high);
		check("both ways", cellId(shown(rows)[998]), low);
		check("and touches nothing else", cellId(shown(rows)[2]), between);
		check("the table still shows what the store holds", matchesStore(rt, rows), true);

		// -------------------------------------------------------------- clearing

		press("clear");
		check("clear empties the store", rt.bench_len(), 0);
		check("and the table", shown(rows).length, 0);

		built.nodes = 0;
		press("swaprows");
		check("a swap with too few rows does nothing at all", rt.bench_len(), 0);
		check("and does not even rebuild the table", built.nodes, 0);

		press("add");
		check("the id counter survived the clearing", cellId(shown(rows)[0]), "2001");

		// ------------------------------------------------------- ten thousand rows

		seed();
		press("runlots");
		check("runlots replaces the table with ten thousand rows", rt.bench_len(), 10000);
		check("and draws all of them", shown(rows).length, 10000);
		check("labelled from the same tables", cellLabel(shown(rows)[0]), expectedLabel(table, 0));
		check("with ids that carried on again", cellId(shown(rows)[0]), "3001");

		press("clear");
		check("and clear takes ten thousand rows away", shown(rows).length, 0);
	} finally {
		Math.random = random;
		// The rows, the selection and the id counter the host is holding. Without
		// this a second run in this process would find this one's, and in a browser
		// nothing would ever call it.
		rt.bench_reset();
	}
}

if (import.meta.filename === process.argv[1]) {
	const failures = [];
	await checkBench((what, actual, expected) => {
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
	console.log("every bench check passed");
}
