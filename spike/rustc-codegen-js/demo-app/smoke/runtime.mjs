// Checks that the runtime the page serves exports everything the compiled
// islands import from it.
//
//   cargo run &            # the app has to be up
//   node smoke/runtime.mjs [http://127.0.0.1:3000]
//
// In the browser a compiled chunk imports the bare specifier `topcoat-dom`,
// which an import map resolves to the served runtime. That runtime is one
// self-contained file, so linking the same graph under node is just fetching it
// and importing it: node has no import maps, but there is nothing left to
// resolve. A name missing here is a name the browser would fail to import.
//
// The names are read out of every chunk the page's import map names, so an
// island that reaches for a runtime name the others do not is covered too. Only
// the ones a chunk imports FROM the runtime count: an island may declare a
// JavaScript library of its own with `#[js_extern]`, and the names it imports
// from that are the library's business rather than the runtime's.

import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const origin = process.argv[2] ?? "http://127.0.0.1:3000";

/// The specifier a chunk imports its DOM operations by.
const RUNTIME = "topcoat-dom";

/// The names the compiled chunks import from the runtime, read out of the
/// chunks themselves rather than listed here, so this cannot drift from what the
/// emitter calls.
async function imported(origin) {
	const map = JSON.parse(await get(`${origin}/demo/chunks.importmap.json`));
	const names = new Set();
	for (const [specifier, url] of Object.entries(map.imports)) {
		if (!specifier.startsWith("topcoat-island/")) continue;
		const source = await get(`${origin}${url}`);
		for (const [, clause, from] of source.matchAll(/^import \{([^}]*)\} from "([^"]+)";/gm)) {
			if (from !== RUNTIME) continue;
			for (const [, name] of clause.matchAll(/(\w+) as \w+/g)) names.add(name);
		}
	}
	if (!names.size) throw new Error("the chunks import nothing: are they ES modules?");
	return [...names];
}

async function get(url) {
	const response = await fetch(url);
	if (!response.ok) throw new Error(`${url} answered ${response.status}`);
	return response.text();
}

// Solid's builds touch the document as they initialize, the same way the vendored
// bundle's own export check does.
globalThis.document ??= { createElement: () => ({ style: {} }), createTextNode: () => ({}) };
globalThis.window ??= globalThis;

const dir = mkdtempSync(join(tmpdir(), "topcoat-runtime-smoke-"));
const source = await get(`${origin}/demo/topcoat-dom.js`);

// The artifact is only the artifact if the page really needs no resolver for it.
const leaked = /(?:^|[;\s])(?:import|export)[^;]*?from\s*["']([^"'.][^"']*)["']/.exec(source);
if (leaked) {
	console.log(`FAIL  the served runtime still imports ${JSON.stringify(leaked[1])}`);
	process.exit(1);
}
console.log("ok    the served runtime resolves nothing at load time");

writeFileSync(join(dir, "topcoat-dom.js"), source);
const runtime = await import(pathToFileURL(join(dir, "topcoat-dom.js")).href);
const names = await imported(origin);

const missing = names.filter(name => typeof runtime[name] !== "function");
for (const name of names) {
	console.log(`${missing.includes(name) ? "FAIL" : "ok  "}  the runtime exports ${name}`);
}
console.log(`ok    the loader's own import: hydrate is ${typeof runtime.hydrate}`);

// The reason there is one file rather than two: an island's state has to be able
// to notify the effects that render it, which only holds inside one graph.
const [read, write] = runtime.createSignal(1);
const observed = [];
runtime.effect(() => observed.push(read()));
write(2);
const oneGraph = observed.at(-1) === 2;
console.log(`${oneGraph ? "ok  " : "FAIL"}  a signal from the runtime drives the runtime's effects`);

console.log("");
if (missing.length || typeof runtime.hydrate !== "function" || !oneGraph) {
	if (missing.length) console.log(`${missing.length} of ${names.length} imported names are missing`);
	if (!oneGraph) console.log(`the signal did not re-run the effect: saw ${JSON.stringify(observed)}`);
	process.exit(1);
}
console.log(`all ${names.length} imported names resolve, in one reactive graph`);
