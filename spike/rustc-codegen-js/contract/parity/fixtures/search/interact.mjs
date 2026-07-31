// What the search island has to do once it is alive.
//
// LIVE against demo-app's /island/search. Written two waves before the island
// existed, against the shape `fixture.json` declared and the wire its `wire` block
// names, and flipped without weakening an assertion: every claim below held the
// first time it ran against the real island. The island's SOURCE shape changed
// under it (see `fixture.json`'s `$landed`) and none of these moved, because they
// are about the DOM and the bytes on the socket rather than about how the island
// holds its state.
//
// WHAT THIS FIXTURE CLAIMS THAT THE OTHER TWO CANNOT
// -------------------------------------------------
// The counter proves a template claims the server's nodes and a click updates one
// text node. The nested fixture proves claiming survives a component boundary. Both
// are entirely SYNCHRONOUS and entirely local: nothing leaves the page.
//
// This one is the first that crosses the boundary. Three surfaces the other two
// cannot reach:
//
//   1. A DELEGATED `input` LISTENER. The counter delegates `click` only, so nothing
//      yet proves the bootstrap's event list is derived from the island's handlers.
//      Typing here goes through a real dispatched, BUBBLING `input` event, because
//      solid's delegation puts one listener on the document and a non-bubbling
//      event would never reach it -- an island that appeared to work under a direct
//      handler call would be dead in a browser.
//   2. TIME. A debounce is a claim about what does NOT happen yet, and the only way
//      to measure that is to look before the window closes and find nothing. Every
//      other check in this harness is a claim about a state; this is a claim about
//      an interval.
//   3. THE WIRE. What the island sends is a contract with the server, and
//      `contract/fixtures/procedure-wire.json` is where that contract is written
//      down. The stub is built FROM it (lib/wire.mjs), not alongside it.
//
// AND THE NEGATIVE, which is the half that decides whether the island is usable: a
// procedure that answers 400 must not leave the island inert. An island that stops
// searching after one failed request is worse than one that never worked, because
// the failure is invisible until a user is already using it.

import { fetchStub, wireParity } from "../../lib/wire.mjs";

/**
 * Real elapsed time, plus a drain of the microtask queue.
 *
 * jsdom runs real timers, so a debounce is measured by actually waiting. Awaiting a
 * `setTimeout` also lets every already-resolved promise continue, which is what
 * makes "the reply has been applied to the DOM" true by the time the next
 * assertion reads it: the stub resolves a promise, the island's continuation writes
 * a signal, and the effect runs synchronously from there.
 */
const after = ms => new Promise(resolve => setTimeout(resolve, ms));

/**
 * Type into an element the way a browser does.
 *
 * `bubbles: true` is load-bearing and not defensive: solid's `delegateEvents` adds
 * ONE listener on the document (`client.js`), so a non-bubbling event never reaches
 * the handler. Setting `.value` first and dispatching second is the order a real
 * keystroke produces, and it is the order a `:value` bind reads.
 */
function type(window, element, value) {
	element.value = value;
	element.dispatchEvent(new window.Event("input", { bubbles: true }));
}

/** The rendered result list, as text. */
const rendered = mount => [...mount.querySelectorAll(".search-result")].map(item => item.textContent);

/**
 * Install a stub as the page's `fetch`, in both realms.
 *
 * In a browser there is one `fetch` and `window === globalThis`, so this would
 * be a single assignment. This harness has a seam a browser does not: the
 * document is jsdom's and the page's modules are imported by node, so the host
 * module's unqualified `fetch` resolves to NODE's global while a fixture
 * stubbing only `window.fetch` would leave it untouched -- and it did, on the
 * first run, which is how this got written. The seam is an artifact of the
 * harness and not a property of the island, so both are set rather than one
 * being declared correct.
 *
 * Installed at call time and not captured at module evaluation, which is what
 * makes it stubbable at all: `island-rt.mjs` calls `fetch(...)` rather than
 * holding a reference to it. `lib/dom.mjs` saves and restores the node global,
 * so a stub cannot outlive its page.
 */
function serve(window, stub) {
	window.fetch = stub;
	globalThis.fetch = stub;
}

/**
 * @param {object} context
 * @param {Element} context.mount the `<topcoat-island>` element
 * @param {Window} context.window
 * @param {import("../../lib/check.mjs").Checks} context.checks
 * @param {() => object[]} context.drain the mutations since the last drain
 * @param {object} context.expected the fixture's `expected` block
 * @param {object} context.wire the fixture's `wire` block
 * @param {{level: string, text: string}[]} context.console the page's console output
 */
export async function interact({ mount, window, checks, drain, expected, wire, console: logged }) {
	const input = mount.querySelector(".search-input");
	const list = mount.querySelector(".search-results");

	// ---- 0. the island the server sent ----

	checks.ok("the search input survived hydration", !!input);
	checks.ok("and the result list did too", !!list);
	checks.is("the list starts empty, because the server ran no query", rendered(mount), []);
	checks.is("the input starts empty", input?.value, "");

	// The stub is built from the CHECKED wire block, so the bodies it accepts and
	// the bodies it returns are procedure-wire.json's. run.mjs has already asserted
	// the block agrees with the spec; this call is what turns that agreement into a
	// running stub rather than a comment.
	const parity = wireParity(wire);
	checks.is("the fixture's wire block still agrees with procedure-wire.json", parity.differences, []);

	const server = fetchStub(wire, [
		{ status: 200, contentType: wire.responseContentType, body: JSON.stringify(expected.results) },
		{ status: 200, contentType: wire.responseContentType, body: JSON.stringify(expected.secondResults) },
		// The error reply, spelled from the vector rather than written out. See
		// `wire.$errorBody`: the middle of the message is serde_json's own wording and
		// asserting it would break on a patch release.
		{ status: wire.errorStatus, contentType: `${wire.errorContentType}; charset=utf-8`, body: `${wire.errorBodyStartsWith} invalid type: integer \`7\`, expected a string ${wire.errorBodyEndsWith}` },
		{ status: 200, contentType: wire.responseContentType, body: JSON.stringify(expected.results) },
	]);
	serve(window, server.fetch);

	// ---- 1. THE DEBOUNCE: what does not happen yet ----
	//
	// Three keystrokes in a row, then a look BEFORE the window closes. Three rather
	// than one because the claim is coalescing, not just delay: an island that fired
	// one call per keystroke after a 150ms delay would pass a one-keystroke test and
	// be exactly the thing a debounce exists to prevent.

	type(window, input, expected.query.slice(0, 1));
	type(window, input, expected.query);
	type(window, input, expected.query);

	checks.is("typing makes no request synchronously", server.calls.length, 0);
	await after(Math.round(expected.debounceMs * 0.4));
	checks.is(
		`and none partway through the ${expected.debounceMs}ms window either`,
		server.calls.length,
		0,
	);

	await after(expected.debounceMs);
	checks.is("exactly one request once the window closes, so three keystrokes coalesced into one call", server.calls.length, 1);

	// ---- 2. THE WIRE: what it sent ----

	const call = server.calls[0];
	// `onTarget`, not `isProcedureRoute`. The island calls demo-app's written-down
	// `POST /demo/search` rather than the procedure route, because a procedure's id is
	// a uuid minted at macro-expansion time and an island's file is expanded once per
	// crate, so the two expansions mint different ids and a shared-file procedure
	// cannot address itself from the client (`wire.callTarget.$why`). The wire is
	// identical either way, which is the point: the route moved and nothing this
	// fixture measures did.
	checks.ok(
		`the call went where the island's client half calls: ${wire.callTarget.kind === "exactPath" ? wire.callTarget.path : `${wire.routePrefix}/<id>`}`,
		call?.onTarget,
		`got ${JSON.stringify(call?.url)}`,
	);
	checks.note(
		`the procedure route ${wire.routePrefix}/<id> also exists for this procedure and is NOT what was called`
		+ `${call?.isProcedureRoute ? " -- but this call DID match its shape, which means the island found a stable id and that is a finding worth chasing" : "; the stub reports isProcedureRoute separately so a change of mind here is visible"}.`,
	);
	checks.is("by POST", call?.method, wire.method);
	checks.is("with the serde wire's media type", call?.contentType?.split(";")[0].trim(), wire.requestContentType);
	checks.is(
		"and the argument as a JSON ARRAY, not a bare value -- the tuple is (T,)",
		call?.args,
		[expected.query],
	);
	checks.ok(
		"never `null`, which is the SURROGATE wire's zero-argument spelling and is rejected by this one",
		call?.body !== "null",
		`procedure-wire.json vector ${wire.vectors.zeroArgumentsRejectsNull}: each wire rejects the other's zero-argument body, which is what makes the asymmetry safe`,
	);

	// ---- 3. the reply rendered ----

	drain();
	checks.is("the reply renders one result per element", rendered(mount), expected.results);
	checks.ok("inside the list the server sent, which is still the same element", list === mount.querySelector(".search-results"));

	// ---- 4. and a second query REPLACES the list rather than appending to it ----
	//
	// The two result sets overlap on one item on purpose (`expected.$results`): a
	// list that appended would read as four items, and a disjoint pair would make an
	// append look like a replace to anyone reading only the last item.

	type(window, input, expected.secondQuery);
	await after(expected.debounceMs * 1.5);
	checks.is("a second settled query makes a second call", server.calls.length, 2);
	checks.is("with the new argument", server.calls[1]?.args, [expected.secondQuery]);
	checks.is("and the list is REPLACED, not appended to", rendered(mount), expected.secondResults);

	const mutations = drain();
	checks.ok(
		"the update touched only the result list",
		mutations.length > 0 && mutations.every(record => record.where.includes("ul") || record.where.includes("li")),
		`mutations landed outside the list: ${JSON.stringify(mutations.map(record => record.where))}`,
	);

	// ---- 5. THE NEGATIVE: a procedure error must not kill the island ----

	const before = logged.length;
	type(window, input, "boom");
	await after(expected.debounceMs * 1.5);
	checks.is("the failing query was sent", server.calls.length, 3);

	// What the client DOES with a 400 is the framework agent's design choice, and
	// this fixture deliberately does not dictate it: it may log, it may render an
	// error, it may do nothing visible. What it may not do is throw out of the page
	// or stop working. So the assertion is about survival, and what the error did is
	// RECORDED rather than required.
	const during = logged.slice(before);
	checks.note(
		`ON THE 400: the page logged ${during.length} message(s) during the failing call`
		+ `${during.length ? `: ${JSON.stringify(during.map(message => `${message.level}: ${message.text}`))}` : ""}.`,
	);
	// Promoted from a note to a claim, because it was MEASURED and it is the good
	// answer. PROCEDURES.md records a defect in the existing browser client: it
	// throws `Procedure call failed: ${status} ${statusText}` and DISCARDS the
	// response body, so the argument index the server computed never reaches the
	// developer. This client does not inherit it -- it logs the body -- and that
	// is worth holding rather than rediscovering.
	//
	// The asserted fragment is the argument path, which comes from the spec's
	// $pathRule rather than from serde_json's wording, so a serde patch release
	// cannot break this (see `wire.$errorBody`).
	checks.ok(
		"the failure reaches the developer WITH the server's diagnosis, not just a status code",
		during.some(message => message.text.includes(wire.errorBodyEndsWith)),
		`no logged message carried ${JSON.stringify(wire.errorBodyEndsWith)}, the argument index the server computed. `
			+ "PROCEDURES.md records the surrogate client discarding the body; this would be that defect inherited.",
	);
	checks.is(
		"the last good results are still on screen: a failed query did not blank the list",
		rendered(mount),
		expected.secondResults,
	);

	type(window, input, expected.query);
	await after(expected.debounceMs * 1.5);
	checks.is("the island still searches after the failure", server.calls.length, 4);
	checks.is("and renders the recovery query's results", rendered(mount), expected.results);

	// A stub with replies left over means the island made fewer calls than the
	// fixture planned, which every count above would already have caught -- but
	// asserting it makes the fixture's own bookkeeping checkable rather than implied.
	checks.is("the fixture queued exactly as many replies as the island asked for", server.pending, 0);
}

/**
 * What the search island looks like when its hydration key did not match.
 *
 * Run by the negative control. These checks PASS -- they assert the failure mode --
 * and the control's own pass condition is that the named parity check in run.mjs
 * failed.
 *
 * WHAT THIS MEASURES, and it is the opposite of what this probe was scaffolded to
 * expect. It was written for a loader that handed the runtime the mount's existing
 * child nodes, where a key miss left the island inert and the symptom was that NO
 * REQUEST WAS EVER MADE -- the loudest signal of the three fixtures. That loader is
 * gone. The current one calls `hydrate(() => entry(...seeds), mount, { renderId })`,
 * so upstream's insert semantics apply: on a key miss the runtime BUILDS the island
 * itself and inserts it, replacing the server's markup.
 *
 * The island then WORKS. It searches, it renders, the results are right. What was
 * lost is hydration, and nothing about the running page says so: same markup, no
 * throw, no console output. That is the finding, and it is why this probe now
 * asserts the working symptom rather than the dead one -- a probe still asserting
 * silence would be asserting something no longer true, and would have gone on
 * passing only because nothing ran it.
 *
 * The one thing that WOULD report it is the loader's `reportLoss`, and it is off
 * here: it is written only under `topcoat dev`, and these captures carry no
 * `data-tl-dev`. So in a production page this remains entirely silent, and this
 * harness is still the only thing that sees it.
 *
 * @param {object} context same shape as `interact`
 */
export async function dead({ mount, window, checks, expected, wire }) {
	const server = fetchStub(wire, [
		{ status: 200, contentType: wire.responseContentType, body: JSON.stringify(expected.results) },
	]);
	serve(window, server.fetch);

	// Queried AFTER hydration, deliberately: on a key miss the element the server
	// sent has been replaced, so this is the client's own input and not the one the
	// capture contained. That substitution is the whole degradation.
	const input = mount.querySelector(".search-input");
	checks.ok("there is still an input, because the client rebuilt one", !!input);
	checks.is("and it looks exactly like the server's", input?.value, "");

	type(window, input, expected.query);
	await after(expected.debounceMs * 2);

	checks.is("the island STILL searches with a broken key: hydration was lost, function was not", server.calls.length, 1);
	checks.is("and still renders the results", rendered(mount), expected.results);
	checks.is("the stub's queue is consumed, so the call above was a real one", server.pending, 0);
	checks.note(
		"Every functional signal here says the island is fine, and it is: it fetches, it renders, the markup is\n"
		+ "        what it should be. Only `_$HY.completed` shows that the server's nodes were thrown away, which is what\n"
		+ "        CLAIMED reads and why that is the check this control has to trip.",
	);
}
