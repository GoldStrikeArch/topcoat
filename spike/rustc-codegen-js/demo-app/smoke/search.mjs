// Drives the compiled search island's whole loop without a browser.
//
//   node smoke/search.mjs
//
// `check.mjs` runs the same checks, so this is part of the one smoke command.
//
// WHAT IS REAL HERE
//
// The compiled chunk and `src/island-rt.mjs` are the ones the app serves. What is
// replaced is the DOM runtime (`search-runtime.mjs`, a node graph shaped like the
// server's markup) and `fetch`, which answers from a queue and records what it
// was asked. So the debounce, the request body, the media type and the rendering
// of the reply are all the shipped code's.
//
// The numbers are the parity fixture's: a 150ms window, `ru` then `st`, and the
// two answers the server really gives for them, which overlap on `rust` and
// `trust` so that a list rendered by appending cannot pass as one that replaced.

import { join } from "node:path";
import { pathToFileURL } from "node:url";

import * as runtime from "./search-runtime.mjs";
import { stage } from "./stage.mjs";

/// The window the island waits out, and a sample inside it.
const DEBOUNCE_MS = 150;

/// What the server answers for each query, measured against the running app.
const ANSWERS = {
	ru: ["rust", "ruby", "trust", "crust"],
	st: ["rust", "trust", "crust"],
};

/// Every request the island made.
const requests = [];

globalThis.fetch = async (url, options) => {
	const query = JSON.parse(options.body)[0];
	requests.push({ url, ...options, query });
	const results = ANSWERS[query] ?? [];
	return { ok: true, status: 200, json: async () => results };
};

const staged = stage({
	"topcoat-dom": pathToFileURL(join(import.meta.dirname, "search-runtime.mjs")).href,
	"topcoat-island-rt": pathToFileURL(join(import.meta.dirname, "..", "src", "island-rt.mjs")).href,
});
const { __island_search } = await import(pathToFileURL(staged.get("search")).href);

/// Waits `ms` of real time, because the debounce is a real timer.
const after = ms => new Promise(resolve => setTimeout(resolve, ms));

/// The text of each row currently rendered into the list.
const rendered = list => list.children.map(row => row.text);

/// Runs every search check through `check(what, actual, expected)`.
export async function checkSearch(check) {
	const page = runtime.reset();
	__island_search();

	const templates = runtime.calls.filter(call => call.op === "template");
	check("the chunk declares the island's template and the row's", templates.length, 2);
	check(
		"the delegated handler is the one the island's page captures",
		(runtime.calls.find(call => call.op === "delegateEvents")?.events ?? []).join(),
		"input",
	);
	check("the server's list is empty and stays empty until an answer", rendered(page.list).join(), "");
	check("no request was made by rendering", requests.length, 0);

	// Three keystrokes in a row. Three, because the claim is that they COALESCE:
	// one request per keystroke after a delay would pass a one-keystroke test and
	// is the exact thing a debounce prevents.
	for (const typed of ["r", "ru", "ru "]) page.input.$$input({ target: { value: typed } });
	page.input.$$input({ target: { value: "ru" } });
	check("typing makes no request of its own", requests.length, 0);

	// Sampled inside the window, so the assertion is not a race with its own timer.
	await after(DEBOUNCE_MS * 0.4);
	check("and none part way through the window", requests.length, 0);

	await after(DEBOUNCE_MS);
	check("one request once the typing settles", requests.length, 1);

	const [first] = requests;
	check("it is a POST", first.method, "POST");
	check("to the route the island was given", first.url, "/demo/search");
	check("on the serde wire", first.headers["content-type"], "application/topcoat+json");
	// A JSON ARRAY even for one argument, and never the surrogate wire's `null`.
	check("carrying the argument as an array", first.body, '["ru"]');

	check("the answer is rendered as one row each", rendered(page.list).join(), ANSWERS.ru.join());

	// A second settled query REPLACES the list. The two answers overlap, so an
	// append would read as seven rows rather than three.
	page.input.$$input({ target: { value: "st" } });
	await after(DEBOUNCE_MS * 1.4);
	check("a second query is one more request", requests.length, 2);
	check("and its answer replaces the list", rendered(page.list).join(), ANSWERS.st.join());
	check("rather than appending to it", page.list.children.length, ANSWERS.st.length);

	// Emptying the box answers nothing, and asks nothing to do it.
	page.input.$$input({ target: { value: "" } });
	await after(DEBOUNCE_MS * 1.4);
	check("an empty box empties the list", rendered(page.list).join(), "");
	check("without asking the server", requests.length, 2);
}

if (import.meta.filename === process.argv[1]) {
	const failures = [];
	await checkSearch((what, actual, expected) => {
		const ok = actual === expected;
		console.log(`${ok ? "ok  " : "FAIL"}  ${what}: ${JSON.stringify(actual)}`);
		if (!ok) failures.push(what);
	});
	console.log("");
	if (failures.length) {
		console.log(`${failures.length} failed`);
		process.exit(1);
	}
	console.log("every search check passed");
}
