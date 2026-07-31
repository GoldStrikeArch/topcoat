// Checks the island loader: which islands it hydrates, when it fetches their
// code, and what it says when hydration was lost.
//
//   node smoke/loader.mjs
//
// `check.mjs` runs the same checks, so this is part of the one smoke command.
//
// The loader under test is the one the app serves: its source is read out of
// `src/dom.rs`, so a check here is a check on the bytes the page gets. What is
// replaced is only what a browser would have supplied: the runtime it imports
// `hydrate` from, the chunks it imports islands from, the document, and the
// observer that says when an island came into view. Replacing the observer with
// a function call is what makes "not yet, and now" a thing a check can measure.

import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

import { Viewport, page } from "./page.mjs";

/// The loader the app serves, read out of the route that serves it.
export function loaderSource() {
	const dom = join(import.meta.dirname, "..", "src", "dom.rs");
	const source = readFileSync(dom, "utf8");
	const opening = 'const LOADER: &str = r#"';
	const start = source.indexOf(opening);
	const end = source.indexOf('"#;', start);
	if (start < 0 || end < 0) throw new Error(`no LOADER string in ${dom}`);
	return source.slice(start + opening.length, end);
}

/// Runs the loader against one page, with the browser's parts replaced.
///
/// Answers with what the run did: the chunks that were imported, in order, the
/// islands that were hydrated, and everything the loader logged.
async function run({ eager = "", dev = false, islands = [], hydrate, show = [] }) {
	const { document, mounts } = page({ eager, dev, islands });
	const viewport = new Viewport();
	const dir = mkdtempSync(join(tmpdir(), "topcoat-loader-smoke-"));

	const imported = [];
	const hydrated = [];
	const logged = [];

	// The chunks the loader imports by name. Each one records that it was
	// fetched, which is the whole of what laziness is measured by.
	for (const { name } of islands) {
		writeFileSync(
			join(dir, `${name}.js`),
			`globalThis.__loaderImported.push(${JSON.stringify(name)});\n`
				+ `export function __island_${name}(...seeds) { return { island: ${JSON.stringify(name)}, seeds }; }\n`,
		);
	}
	writeFileSync(
		join(dir, "runtime.js"),
		"export function hydrate(code, mount, options) { return globalThis.__loaderHydrate(code, mount, options); }\n",
	);

	globalThis.__loaderImported = imported;
	globalThis.__loaderHydrate = (code, mount, options) => {
		hydrated.push({ island: mount.dataset.ti, options, value: code() });
		hydrate?.(mount);
	};
	globalThis.document = document;
	globalThis.IntersectionObserver = viewport.IntersectionObserver;

	const console_ = { ...console };
	const capture = level => (...args) => logged.push({ level, message: args.join(" ") });
	console.warn = capture("warn");
	console.error = capture("error");

	const staged = join(dir, "loader.mjs");
	writeFileSync(
		staged,
		loaderSource()
			.replace('"topcoat-dom"', JSON.stringify(pathToFileURL(join(dir, "runtime.js")).href))
			.replace(
				'const CHUNK = "topcoat-island/"',
				`const CHUNK = ${JSON.stringify(`${pathToFileURL(dir).href}/`)}`,
			)
			.replaceAll("CHUNK + name", "CHUNK + name + `.js`"),
	);

	try {
		await import(pathToFileURL(staged).href);
		await settle();
		if (show.length) {
			viewport.show(...show.map(name => mounts.get(name)));
			await settle();
		}
	} finally {
		console.warn = console_.warn;
		console.error = console_.error;
	}
	return { imported, hydrated, logged, mounts, viewport };
}

/// Lets everything the loader queued run.
async function settle() {
	for (let turn = 0; turn < 5; turn++) await new Promise(resolve => setImmediate(resolve));
}

/// The two islands the checks below use, with one server-written key each.
const ISLANDS = [
	{ name: "counter", key: "i0", seeds: [5], keys: ["i0.0"] },
	{ name: "nested", key: "i1", seeds: [5], keys: ["i1.0"] },
];

/// Detaches the island's server nodes, which is what a key the server never
/// wrote makes the runtime do: it builds the node itself and the server's is
/// replaced.
function rebuild(mount) {
	for (const node of mount.children) node.parent = null;
	mount.children = [];
}

/// Runs every loader check through `check(what, actual, expected)`.
export async function checkLoader(check) {
	// An eager island is hydrated as soon as the loader runs; a lazy one is not
	// fetched at all until it is on screen. Both islands are on the page, so the
	// difference measured is the loader's and not the page's.
	const lazy = await run({ eager: "counter", islands: ISLANDS });
	check("the eager island's chunk is fetched at once", lazy.imported.includes("counter"), true);
	check("the eager island is hydrated at once", lazy.hydrated.length, 1);
	check("a lazy island's chunk is not fetched before it is seen", lazy.imported.includes("nested"), false);
	check("a lazy island is still waiting to be seen", lazy.viewport.observed.size, 1);

	const seen = await run({ eager: "counter", islands: ISLANDS, show: ["nested"] });
	check("the lazy island's chunk is fetched once it is seen", seen.imported.includes("nested"), true);
	check("both islands end up hydrated", seen.hydrated.map(one => one.island).sort().join(), "counter,nested");
	check("an island is not observed again after it hydrates", seen.viewport.observed.size, 0);
	check(
		"each island hydrates against its own key prefix",
		seen.hydrated.map(one => one.options.renderId).sort().join(),
		"i0.,i1.",
	);
	check("the seeds the server serialized reach the entry point", seen.hydrated[0].value.seeds.join(), "5");

	// The other opt-out: the attribute on the mount itself, which is what an
	// island declares for itself once it can.
	const marked = await run({ islands: [{ ...ISLANDS[0], tl: "eager" }, ISLANDS[1]] });
	check("an island marked eager on its own mount is hydrated at once", marked.hydrated.length, 1);
	check("and the unmarked one is not", marked.imported.includes("nested"), false);

	// The hydration check. A key the client asks for and the server never wrote
	// costs the page nothing visible, which is exactly why it needs saying.
	const quiet = await run({ eager: "counter", dev: true, islands: [ISLANDS[0]] });
	check("a clean hydrate says nothing", quiet.logged.length, 0);

	const lost = await run({ eager: "counter", dev: true, islands: [ISLANDS[0]], hydrate: rebuild });
	check("a lost hydration is reported", lost.logged.length, 1);
	check("the report is a warning", lost.logged[0]?.level, "warn");
	check("it names the island", lost.logged[0]?.message.includes("`counter`"), true);
	check("and the key of the node that was replaced", lost.logged[0]?.message.includes("`i0.0`"), true);

	// Outside a dev build the check does not run at all, so it costs a query per
	// island in the one build where the answer is worth having.
	const shipped = await run({ eager: "counter", islands: [ISLANDS[0]], hydrate: rebuild });
	check("a build that is not a dev build stays silent", shipped.logged.length, 0);
}

if (import.meta.filename === process.argv[1]) {
	const failures = [];
	await checkLoader((what, actual, expected) => {
		const ok = actual === expected;
		console.log(`${ok ? "ok  " : "FAIL"}  ${what}: ${JSON.stringify(actual)}`);
		if (!ok) failures.push(what);
	});
	console.log("");
	if (failures.length) {
		console.log(`${failures.length} failed`);
		process.exit(1);
	}
	console.log("every loader check passed");
}
