// Drives the compiled dashboard island's whole loop without a browser.
//
// The real chunk, the real `src/island-rt.mjs`, and the real chart library from
// `contract/fixtures/js-extern`, which is a recorder: it draws nothing and writes
// down every operation performed on it. So what the chart was asked to do is a
// value this file can assert, rather than a picture somebody has to look at.
//
// Stubbed: the DOM runtime (`dashboard-runtime.mjs`) and the two browser globals
// the island declares with `#[js_extern]`, `EventSource` and `document`. Those
// two are stubbed HERE rather than in the runtime stub because they are not
// runtime names: the compiled code reaches them at global scope, exactly as a
// page would, and standing them up at global scope is what proves that is where
// it reached them. A declaration that had quietly imported them instead would
// find nothing here.
//
// What is established: the island constructs its chart with a configuration
// built in Rust; it subscribes to the feed once however many times its list
// re-runs; a tick arriving writes one price and re-ranks the whole list, with
// every row reading the price its own signal holds; and every update carries the
// whole series, as an object, keyed by symbol.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { compiled } from "./compiled.mjs";
import { reset, rows } from "./dashboard-runtime.mjs";
import { stage } from "./stage.mjs";

/// Where the fake chart library lives, and the specifier the island reaches it by.
const CHART_LIB = join(
	import.meta.dirname,
	"..",
	"..",
	"contract",
	"fixtures",
	"js-extern",
	"chart-lib.mjs",
);

/// The island's own tables, read out of its source.
///
/// Read rather than repeated: the feed and the island already share one symbol
/// table, and a check carrying a third copy is a check that can agree with
/// neither. A shape this cannot read is an error, not a default.
function tables() {
	const source = readFileSync(join(import.meta.dirname, "..", "island", "dashboard.rs"), "utf8");
	const symbols = source.match(/SYMBOLS: \[&str; \d+\] = \[([^\]]*)\]/);
	const prices = source.match(/START_CENTS: \[i32; [^\]]*\] = \[([^\]]*)\]/);
	if (!symbols || !prices) throw new Error("island/dashboard.rs no longer declares its tables");
	return {
		symbols: [...symbols[1].matchAll(/"([^"]+)"/g)].map(match => match[1]),
		prices: prices[1].split(",").map(cell => Number(cell.trim().replaceAll("_", ""))),
	};
}

/// The three ticks the feed is made to deliver.
///
/// Written here rather than taken from the generator: what is being measured is
/// what the island does with a tick, so the moves are chosen to force the
/// ranking to change three times and to put a fall above a rise.
const TICKS = [
	{ slot: 3, symbol: "DYAD", price: 4460, delta: 150 },
	{ slot: 1, symbol: "BOLT", price: 7875, delta: -200 },
	{ slot: 4, symbol: "ECHO", price: 15695, delta: 75 },
];

export async function checkDashboard(check) {
	const { symbols, prices } = tables();
	const { path } = compiled("dashboard");
	console.log(`# ${path}\n`);

	const lib = await import(pathToFileURL(CHART_LIB).href);
	const runtime = pathToFileURL(join(import.meta.dirname, "dashboard-runtime.mjs")).href;
	const host = pathToFileURL(join(import.meta.dirname, "..", "src", "island-rt.mjs")).href;
	const staged = stage({
		"topcoat-dom": runtime,
		"topcoat-island-rt": host,
		"topcoat-chart": pathToFileURL(CHART_LIB).href,
	});

	// The island holds the chart and the ranking across calls, so a second run of
	// this file in one process would start where the first left off.
	const rt = await import(host);
	rt.dash_reset();

	// The two globals the island declares. The listener is captured rather than
	// dispatched, because what the feed does is call it.
	let listeners = [];
	let opened = [];
	globalThis.EventSource = class EventSource {
		constructor(url) {
			opened.push(url);
		}
		addEventListener(kind, handler) {
			listeners.push({ kind, handler });
		}
	};

	const page = reset(symbols, prices);
	let asked = [];
	globalThis.document = {
		querySelector(selector) {
			asked.push(selector);
			return selector === ".dash-chart" ? page.canvas : null;
		},
	};

	lib.__reset();
	const { __island_dashboard } = await import(pathToFileURL(staged.get("dashboard")).href);
	__island_dashboard();

	// ------------------------------------------------------------ setting up

	check("the island opened exactly one feed", opened.length, 1);
	check("it is the app's own tick endpoint", opened[0], "/demo/ticks");
	check("one listener, for the event the feed names", listeners.length, 1);
	check("the listener is on `tick`", listeners[0]?.kind, "tick");
	check("the chart's canvas was found by selector", asked.join(), ".dash-chart");

	const built = lib.__trace().filter(record => record.op === "new");
	check("the chart was constructed once", built.length, 1);
	check("through the library's named export", built[0]?.via, "named");
	check("with `new`, not as a call", built[0]?.ctor, "Chart");
	// The configuration is a `#[repr(C)]` struct in the island's Rust, and this is
	// it having crossed as an object: the field names are the Rust field names.
	check(
		"the configuration is the Rust struct, as an object",
		JSON.stringify(built[0]?.args?.[1]),
		JSON.stringify({ kind: "line", span: symbols.length }),
	);
	check("the canvas it was handed is the one on the page", built[0]?.args?.[0], "node(canvas)");

	// ------------------------------------------------------ the opening render

	const opening = rows(page.list);
	check("the list opens with one row per symbol", opening.length, symbols.length);
	check("in slot order, because nothing has moved", opening.map(row => row.symbol).join(), symbols.join());
	check(
		"at the opening prices, in whole cents",
		opening.map(row => row.price).join(),
		prices.join(),
	);
	check("with no row marked as having moved", opening.map(row => row.class).join(), symbols.map(() => "mover").join());

	const sent = () => lib.__trace().filter(record => record.op === "send" && record.method === "update");
	check("the chart was updated once on the way up", sent().length, 1);
	check(
		"with the series the signals hold",
		JSON.stringify(sent()[0]?.args?.[0]),
		JSON.stringify(series(symbols, prices)),
	);

	// The node each symbol started in, taken before anything moves, so the check
	// at the end can say whether a re-render moved rows or rebuilt them.
	const before = new Map(opening.map(row => [row.symbol, row.node]));

	// ------------------------------------------------------------- three ticks

	const deliver = tick => listeners[0].handler({ data: JSON.stringify(tick) });
	const live = [...prices];

	/// What the rendered rows should read, in the order they are actually in.
	///
	/// The rows are in RANK order and `live` is in SLOT order, so the expectation
	/// is built by looking each row's symbol back up rather than by zipping.
	const expected = rendered =>
		rendered.map(row => String(live[symbols.indexOf(row.symbol)])).join();

	deliver(TICKS[0]);
	live[TICKS[0].slot] = TICKS[0].price;
	let now = rows(page.list);
	check("a tick moves its symbol to the top", now[0]?.symbol, TICKS[0].symbol);
	check("carrying the price the tick named", now[0]?.price, String(TICKS[0].price));
	check("and the move it made", now[0]?.delta, String(TICKS[0].delta));
	check("a rise is marked as one", now[0]?.class, "mover up");
	check("the rest keep slot order", now.slice(1).map(row => row.symbol).join(), "ACME,BOLT,CRUX,ECHO");
	check("only the symbol that moved changed price", now.map(row => row.price).join(), expected(now));

	deliver(TICKS[1]);
	live[TICKS[1].slot] = TICKS[1].price;
	now = rows(page.list);
	check("a bigger fall outranks a smaller rise", now[0]?.symbol, TICKS[1].symbol);
	check("a fall is marked as one", now[0]?.class, "mover down");
	check("the previous mover holds second", now[1]?.symbol, TICKS[0].symbol);

	deliver(TICKS[2]);
	live[TICKS[2].slot] = TICKS[2].price;
	now = rows(page.list);
	check("three moves rank by size, biggest first", now.map(row => row.symbol).join(), "BOLT,DYAD,ECHO,ACME,CRUX");
	check("the symbols that never moved sit below, in slot order", now.slice(3).map(row => row.symbol).join(), "ACME,CRUX");
	check("every price is the one its signal holds", now.map(row => row.price).join(), expected(now));

	// ------------------------------------------- why the loop is not keyed

	// This pins the reason rather than the preference. `view_abi::push_keyed`
	// hands back the node a key contributed last time and discards the row just
	// built, text included, so a keyed version of this list reorders correctly
	// and then shows its first render's numbers for ever. Measured: keyed, the
	// row above read 4310 after a tick that set it to 4460.
	//
	// So the rows are rebuilt, and that is what makes the prices right. If this
	// check ever starts failing, the reconcile has gained per-row reactivity and
	// the loop should be keyed again.
	const rebuilt = now.filter(row => before.get(row.symbol) !== row.node);
	check("a row is rebuilt rather than moved, which is what refreshes it", rebuilt.length, symbols.length);

	// ---------------------------------------------------------- the chart, live

	const updates = sent();
	check("one update per tick, plus the one on the way up", updates.length, TICKS.length + 1);
	check(
		"the last update carries every current price, by symbol",
		JSON.stringify(updates.at(-1)?.args?.[0]),
		JSON.stringify(series(symbols, live)),
	);
	check("every update sends one argument", updates.every(update => update.argc === 1), true);
	check(
		"every update goes to the chart that was built",
		updates.every(update => update.target === built[0]?.result),
		true,
	);

	// A second connection would mean the setup ran again, which the list re-running
	// three times is exactly the way to cause.
	check("the feed was never opened a second time", opened.length, 1);
}

/// The series an update carries: one field per symbol, lower cased, in slot order.
function series(symbols, prices) {
	return Object.fromEntries(symbols.map((symbol, slot) => [symbol.toLowerCase(), prices[slot]]));
}
