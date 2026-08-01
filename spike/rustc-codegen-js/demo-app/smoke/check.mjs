// Checks the compiled counter island without a browser.
//
//   node smoke/check.mjs
//
// There is no DOM implementation in the tree, so this drives the compiled module
// against `stub.mjs`, which implements the runtime names the emitter calls over a
// node graph shaped like the HTML the server sends. What it establishes is that
// the module claims the server's node instead of cloning a template, that its
// signal is seeded from the argument the loader passes, and that the whole
// event-to-signal-to-DOM loop is the compiled Rust and works.
//
// What it cannot establish is that the real runtime finds the node at all: that
// needs `data-hk` and a registry, so it stays in the browser checklist in
// README.md.

import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { checkBench } from "./bench.mjs";
import { checkBudgets } from "./budgets.mjs";
import { compiled } from "./compiled.mjs";
import { checkLife } from "./life.mjs";
import { checkLoader } from "./loader.mjs";
import { checkMines } from "./mines.mjs";
import { checkSand } from "./sand.mjs";
import { stage } from "./stage.mjs";
import { calls, decrement, display, increment } from "./stub.mjs";

// A chunk imports its runtime by the specifier an import map resolves in the
// browser, and imports the shared chunk beside it by a relative path. Node has
// no import map, so the copies that run here name the stub instead; the relative
// import works unchanged because the copies are siblings too.
//
// `topcoat-island-rt` is mapped even though the counter itself borrows nothing
// from the host: the shared chunk does. Once more than one island reaches
// `str::as_bytes`, the panic path that lowers to `__rt.str_bytes` is hoisted into
// `shared.js`, and every chunk's graph carries that import whether or not the
// island at the top of it uses the host for anything. A specifier in the graph is
// a specifier node has to resolve, so it is staged here rather than left out.
const { path } = compiled("counter");
const staged = stage({
	"topcoat-dom": pathToFileURL(join(import.meta.dirname, "stub.mjs")).href,
	"topcoat-island-rt": pathToFileURL(join(import.meta.dirname, "..", "src", "island-rt.mjs")).href,
});
const { __island_counter } = await import(pathToFileURL(staged.get("counter")).href);

console.log(`# ${path}\n`);

__island_counter(5);

const failures = [];
const check = (what, actual, expected) => {
	const ok = actual === expected;
	console.log(`${ok ? "ok  " : "FAIL"}  ${what}: ${JSON.stringify(actual)}`);
	if (!ok) {
		failures.push(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
	}
};

const ops = calls.map(call => call.op);
console.log(`${JSON.stringify(calls, null, 1)}\n`);

/// The first call of one runtime operation.
///
/// Looked up by name rather than by position: what this suite is about is which
/// operations the compiled module performs, not the order the emitter happens to
/// lay two independent ones out in.
const call = op => calls.find(call => call.op === op) ?? {};

/// The markup the server sends for the counter, with the value's place left open.
const COUNTER_HTML = '<div class="island"><p class="island-count">count <!$><!/></p>'
	+ '<div class="island-controls">'
	+ '<button class="island-step" type="button">-1</button>'
	+ '<button class="island-step" type="button">+1</button>'
	+ "</div></div>";

// The counter has a chunk of its own, so its chunk declares its markup and no
// other island's. What the check is about is that the markup is declared once
// and at module scope, rather than rebuilt wherever it is instantiated.
const templates = calls.filter(call => call.op === "template");
check("the chunk declares one template, at module scope", templates.length, 1);
check(
	"the counter's template is the markup the server sends, with the value's place left open",
	templates.filter(template => template.html === COUNTER_HTML).length,
	1,
);
check("the server's node is claimed, not cloned", ops.includes("getNextElement"), true);
check("the delegated handler is registered", (call("delegateEvents").events ?? []).join(), "click");
check("the dynamic child is inserted into its element", call("insert").parent, "<p>");

check("the count renders the seed the loader passed", display.rendered, "5");
check("the bound property starts false", decrement.disabled, false);

increment.$$click({});
check("+1 updates the count", display.rendered, "6");

decrement.$$click({});
decrement.$$click({});
check("-1 twice updates the count", display.rendered, "4");

for (let index = 0; index < 4; index++) decrement.$$click({});
check("the count reaches zero", display.rendered, "0");
check("the bound property tracked the signal down to zero", decrement.disabled, true);

increment.$$click({});
check("the bound property tracked the signal back up", decrement.disabled, false);

// The nodes `getNextMarker` claims are the range the first insert replaces. It
// has to be the value alone: claiming the opening marker as well is what makes
// the first insert build a fresh text node and delete the server's, which is a
// visible mutation on a hydrate that should have none.
check("the insert claims the server's text node and nothing else", (call("getNextMarker").claimed ?? []).join(" "), '"5"');

// The showcase's three, each in a module of its own so that three islands can be
// written at once without three authors editing this file. All three drive the
// compiled chunk against the real `src/island-rt.mjs`, so the boards are the
// host's own arrays; sand additionally stands up a recording canvas and the two
// browser globals it reaches. See life.mjs, sand.mjs and mines.mjs.
console.log("");
await checkLife(check);

console.log("");
await checkSand(check);

console.log("");
await checkMines(check);

// The js-framework-benchmark island: the six operations, the row contract the
// harness reads, and what one signal over a whole table costs. See bench.mjs.
console.log("");
await checkBench(check);

// Which islands the loader hydrates, when it fetches their code, and what it
// says when a hydration was lost. See loader.mjs.
console.log("");
await checkLoader(check);

// Every chunk's size against its recorded baseline. See budgets.mjs.
console.log("");
checkBudgets(check);

console.log("");
if (failures.length) {
	console.log(`${failures.length} failed`);
	process.exit(1);
}
console.log("all checks passed");
