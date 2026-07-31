// What the dashboard island has to do, live.
//
// The G2 milestone island: a chart reached through five `#[js_extern]`
// declarations and a movers list driven by server-sent ticks. Every input is read
// out of `fixture.json` -- the tick frames, the expected rows, the expected chart
// series -- so the island can change a class name or a method name without an
// assertion here being rewritten, and an assertion rewritten next to a running
// island is one that agreed with it.
//
// WHAT THIS FIXTURE CLAIMS THAT THE OTHER THREE CANNOT
// ---------------------------------------------------
// The counter proves a template claims the server's nodes. The nested fixture
// proves claiming survives a component boundary. `search` proves an island can
// cross the network boundary and come back. All three are driven BY THE USER, and
// in all three the island's list starts EMPTY.
//
// Four things only this one reaches:
//
//   1. HYDRATION ADOPTING REPEATED SERVER-RENDERED ROWS. Five `<li>`s with five
//      keys, written by the server and adopted by the client. Every earlier
//      fixture's list was built by the client, so nothing until now has asserted
//      that a `for` loop can adopt rather than create.
//   2. A FOREIGN CALL SEQUENCE. The chart is not a compiled-Rust value, it is a
//      handle to somebody else's object, and what the island did to it is a
//      sequence of operations rather than a state.
//
//      Not in the STYLE of contract/fixtures/js-extern/vectors.json: in its actual
//      records. demo-app serves this suite's own chart-lib.mjs as the chart library
//      and the import map resolves `topcoat-chart` to it, so the island calls into
//      the same recorder the js-extern vectors were recorded from. Nothing about
//      the chart is stubbed or substituted here; only the stream is.
//   3. A SERVER-DRIVEN UPDATE. The island's only input is a stream, so the ranking
//      being right is a claim about the host's rule rather than about a click.
//   4. A FAILURE THE USER CANNOT CAUSE. `search` can be handed a 400 because the
//      user types. Nobody can ask a live feed for a malformed frame.
//
// WHAT THIS FIXTURE DELIBERATELY DOES NOT CLAIM
// ---------------------------------------------
// Keyed node identity across a reorder. It was scaffolded to claim exactly that,
// and the claim is WRONG for this list: see `expected.$notKeyed`. `push_keyed`
// returns the node a key contributed last time and discards the freshly built row,
// text included, so a keyed version of this list reorders correctly and shows its
// first prices for ever. Asserting identity would pass for the broken version and
// fail for the correct one. So section 4 INVERTS it and asserts that the rows are
// replaced, which is the property this list actually has to have.
//
// THE HISTORY OF THIS ASSERTION, BECAUSE IT HAS ALREADY FLIPPED TWICE
// -------------------------------------------------------------------
// Read this before flipping it a third time. Both flips were correct at the time
// and neither was a mistake being undone; the fixture is where the argument is
// settled, so the argument is written here.
//
//   FLIP 1 (wave 5, scaffold -> keyed).  Written to assert keyed node identity
//   across a reorder, because a list that re-ranks on every tick is the textbook
//   case for a keyed `for`, and the milestone island was specified that way.
//
//   FLIP 2 (wave 5, keyed -> INVERTED).  Running it showed the design is wrong for
//   THIS list. The measurement is in `fixture.json`'s section 2 comment: a row read
//   4310 after a tick that set it to 4460. The cause is above. Inverted, and this
//   is the state today.
//
//   NOT FLIPPED (wave 6).  The obvious next move is "fix `push_keyed`, then flip
//   back". It was examined and REJECTED, on three findings, the third of which
//   ends the argument (backend wave-6 report, item 1):
//
//     K1. There is no disposal bug to fix. A plain `( )` hole in a row is
//         `Fill::Once` and emits a bare `_$insert` with NO effect; a `$( )` hole is
//         an effect owned by the ISLAND, not by the row, because a Rust loop opens
//         no reactive scope. So the rule is sharper than "keyed is for static
//         rows": a keyed row's changing parts must be reactive holes OF THE ROW'S
//         OWN TEMPLATE, and a plain `( )` hole in a keyed row is written once to a
//         node that is then discarded. That is this list exactly -- a `$( )` here
//         cannot carry the free function call the price formatting needs.
//     K2. Both repair mechanisms are unsound. Re-running the fills against the
//         cached node silently DUPLICATES an anchored child hole (upstream's
//         `insert` seeds `current = []` when `initial` is undefined, so the
//         `cleanChildren` path inserts a second text node before the anchor on
//         every render). Morphing the cached node from the fresh one cannot see
//         `HoleKind::Event` or `HoleKind::Property` at all, and there is no DOM
//         implementation anywhere in this tree to verify a morph against -- the
//         three stubs that exist disagree about where a hole's value even lives.
//     K3. THE DECIDING ONE. Even a perfect fix does not move corpus family 13 to
//         MATCH. The reference is solid's `<For>`, which clones a row's template
//         once per row EVER; ours is an eager Rust loop that clones once per row
//         PER RENDER, before `push_keyed` is reached. The divergence is the loop,
//         not the reconcile -- which is what the standing rule is named after,
//         `keyed-list-vs-a-rust-loop`. Family 13 is ATTRIBUTED for a structural
//         reason that outlives any version of `push_keyed`.
//
// So a third flip needs to defeat K3, not just K1 and K2. Fixing the reconcile is
// not sufficient and, on K1, is not even the thing that would be wrong.

/**
 * The stubbed stream, shared between `setup` and `interact`.
 *
 * Module level because the two halves run at different times against the same
 * page, and the thing they share is the constructor the island already called.
 */
let stream = null;

/** Real elapsed time, plus a drain of the microtask queue. See `search`'s copy. */
const after = ms => new Promise(resolve => setTimeout(resolve, ms));

/**
 * Put a stubbed `EventSource` in the realm BEFORE the island hydrates.
 *
 * This has to be a `setup` hook and cannot live in `interact`, and the reason is
 * specific to this island. `search` stubs `fetch` from inside `interact` because
 * its call happens after a 150ms debounce, long after hydration returned. The
 * dashboard subscribes inside the list's FIRST EFFECT RUN, which is hydration
 * itself: the island's own module docs explain why that is the safe place for it
 * (an island's setup must be synchronous, and hydration's window is exactly one
 * synchronous call stack).
 *
 * And jsdom provides no `EventSource` at all -- verified, `typeof
 * window.EventSource` is `undefined`. So without this the island throws inside the
 * hydrate bracket and every assertion afterwards measures the wreckage.
 *
 * Installed in BOTH realms, for the reason `search` sets `fetch` twice: the
 * document is jsdom's but the page's modules are imported by node, so an
 * unqualified `EventSource` resolves to node's global while the island may reach
 * `window.EventSource`. The seam is an artifact of this harness rather than a
 * property of the island, so both are set rather than one being declared correct.
 *
 * A THROWING LISTENER IS CAUGHT AND RECORDED, because that is what a browser does.
 * `dispatchEvent` does not propagate a listener's exception to whoever dispatched
 * it: it is reported to the global error handler, the other listeners still run,
 * and the connection stays open. A stub that let the throw escape would be testing
 * node's call semantics instead of the island's resilience, and would make the
 * malformed-frame assertion vacuous -- it would fail in the harness before
 * reaching anything the island decides.
 */
export async function setup({ window }) {
	const opened = [];
	const errors = [];

	class StubEventSource {
		constructor(url, options) {
			this.url = String(url);
			this.withCredentials = !!options?.withCredentials;
			this.readyState = 1;
			this.onmessage = null;
			this.onerror = null;
			this.onopen = null;
			this.listeners = new Map();
			this.closed = false;
			opened.push(this);
		}

		addEventListener(type, handler) {
			if (!this.listeners.has(type)) this.listeners.set(type, []);
			this.listeners.get(type).push(handler);
		}

		removeEventListener(type, handler) {
			this.listeners.set(type, (this.listeners.get(type) ?? []).filter(entry => entry !== handler));
		}

		close() {
			this.closed = true;
			this.readyState = 2;
		}

		/**
		 * Deliver one frame, the way the browser delivers one.
		 *
		 * Both dispatch paths, because which one an island used is its own business:
		 * a named event reaches `addEventListener(name)`, and the default `message`
		 * additionally reaches the `onmessage` property.
		 */
		push(name, data) {
			const event = new window.MessageEvent(name ?? "message", { data });
			const handlers = [...(this.listeners.get(name ?? "message") ?? [])];
			if ((name ?? "message") === "message" && typeof this.onmessage === "function") handlers.push(this.onmessage);
			for (const handler of handlers) {
				try {
					handler.call(this, event);
				} catch (error) {
					errors.push(error);
				}
			}
		}
	}

	window.EventSource = StubEventSource;
	globalThis.EventSource = StubEventSource;
	stream = { opened, errors };
}

/**
 * The chart recorder, as the PAGE has it.
 *
 * There is no fake here and nothing is substituted. demo-app serves this suite's
 * own `contract/fixtures/js-extern/chart-lib.mjs` at `/demo/chart-lib.js` and the
 * page's import map resolves `topcoat-chart` to it, so the module the island calls
 * into is the recorder itself. `moduleFor` gives the staged file URL the page
 * imported, and node caches by resolved URL, so importing it here yields the SAME
 * module object -- and therefore the same records array the island wrote into.
 *
 * Importing the file by any other path would give a second recorder with an empty
 * trace, and every assertion below would then be measuring nothing while passing
 * the ones that check for absence. That is the whole reason `moduleFor` exists.
 */
const recorder = (moduleFor, url) => import(moduleFor(url));

/**
 * A record reduced to its SHAPE: which operation, on what, with how many arguments.
 *
 * The argument VALUES are asserted separately, because the shape is what the
 * `#[js_extern]` declarations decide and the values are what the island's own logic
 * decides. Mixing them would make a change of series look like a change of
 * declaration.
 */
function shapeOf(record) {
	return record.op === "new"
		? { op: "new", ctor: record.ctor, via: record.via, argc: record.argc }
		: { op: record.op, target: record.target, method: record.method, argc: record.argc };
}

/** The declared shapes, with the commentary key dropped. */
const declaredShapes = chart => chart.expectedShapes.map(({ $: _comment, ...rest }) => rest);

/** The row elements, in document order. */
const rowNodes = mount => [...mount.querySelectorAll("ul.movers > li")];

/** Every row as [symbol, price, delta], in document order. */
const rowsOf = mount => rowNodes(mount).map(row => [
	row.querySelector(".mover-symbol")?.textContent ?? "",
	row.querySelector(".mover-price")?.textContent ?? "",
	row.querySelector(".mover-delta")?.textContent ?? "",
]);

/** Every row's class attribute, in document order. */
const classesOf = mount => rowNodes(mount).map(row => row.getAttribute("class"));

/**
 * @param {object} context
 * @param {Element} context.mount
 * @param {import("../../lib/check.mjs").Checks} context.checks
 * @param {() => object[]} context.drain
 * @param {object} context.expected
 * @param {object} context.declared the whole fixture.json
 * @param {(url: string) => string} context.moduleFor
 * @param {{level: string, text: string}[]} context.console
 */
export async function interact({ mount, checks, drain, expected, declared, moduleFor, console: logged }) {
	const { stream: feed, chart } = declared;

	// ---- 0. hydration: the server's rows, adopted ----
	//
	// run.mjs has already asserted the global facts (zero mutations, the tree the
	// server sent, every key claimed off the server's own node). These are the
	// island-specific half, and the content assertion matters more here than
	// anywhere else in the suite: this is the first island whose list arrives
	// FILLED, so "the client rebuilt it identically" and "the client adopted it"
	// look the same in the markup and differ only in the mutation count.

	const list = mount.querySelector("ul.movers");
	const canvas = mount.querySelector("canvas.dash-chart");
	checks.ok("the movers list survived hydration", !!list);
	checks.ok("and the chart's canvas did too", !!canvas);
	checks.is(
		"the server's five rows are on screen before any tick",
		rowsOf(mount),
		expected.openingRows,
	);
	checks.is("hydration itself mutated nothing", drain().length, 0);

	// Held so section 4 can prove the list ELEMENT survives even though its rows do
	// not. The distinction is the whole of what "not keyed" costs: the list is
	// adopted, its children are rebuilt.
	const claimedList = list;
	const openingNodes = rowNodes(mount);

	// ---- 1. the chart, as the island already used it ----
	//
	// Taken hold of rather than installed: the recorder is the served module, and
	// the island has already constructed a chart and pushed the opening series
	// during hydration. Nothing is reset, because that construction is the first
	// record this fixture asserts.

	const chart_lib = await recorder(moduleFor, chart.delivery.url);

	// ---- 2. the subscription the island made during hydration ----

	checks.ok(
		"the stub was installed before hydration, so the island had an EventSource to construct",
		!!stream,
		"setup() did not run: run.mjs calls it before the hydrate bracket, and without it jsdom has no EventSource at all",
	);
	checks.is("the island opened exactly one subscription", stream?.opened.length, 1);
	const source = stream?.opened[0];
	if (!source) return;
	checks.is("on the url the island declares", source.url, feed.url);
	checks.ok(
		`and registered a listener for the named "${feed.eventName}" event, not for the default message event`,
		(source.listeners.get(feed.eventName) ?? []).length === 1,
		`it registered ${JSON.stringify([...source.listeners.keys()])}; the wire carries "event: ${feed.eventName}", so a listener on message alone would never fire`,
	);

	const frames = feed.ticks;
	const send = tick => source.push(feed.eventName, "malformed" in tick ? tick.malformed : JSON.stringify(tick.data));

	// ---- 3. the good frames, and the host's ranking ----
	//
	// The expected rows are DERIVED from the host's documented rule (fixture.json
	// `stream.$hostRule`) rather than observed once and written down, so a
	// disagreement is between the island and the rule instead of between the island
	// and whatever it did the first time. Tick 2 is the regression frame: keyed,
	// DYAD read 4310 here for ever.

	for (const [at, tick] of frames.entries()) {
		if (feed.expectedRows[at] === null) continue; // the malformed frame; section 6 drives it

		send(tick);
		await after(0);

		checks.is(
			`tick ${at + 1} (${tick.data.symbol} ${tick.data.delta > 0 ? "+" : ""}${tick.data.delta}) re-ranks and re-prices the list`,
			rowsOf(mount),
			feed.expectedRows[at],
		);
		checks.is(
			`and tick ${at + 1}'s classes follow each row's own delta`,
			classesOf(mount),
			feed.expectedClasses[at],
		);
	}

	// ---- 4. THE INVERTED IDENTITY CHECK ----
	//
	// Scaffolded as "every row is the same node it was, moved rather than
	// rewritten". That is the WRONG claim for this list and asserting it would pass
	// for a version that shows stale prices for ever. See `expected.$notKeyed`.
	//
	// So the claim is inverted: the rows are rebuilt, and the value of saying so is
	// that if keying silently comes back this check fails and points at the reason.
	// The list ELEMENT is asserted to survive in the same breath, because "the whole
	// island was rebuilt" and "the rows were rebuilt" are different failures and
	// only the second one is correct.
	//
	// CONFIRMED in wave 6 and given a second, stronger cause. The first cause is
	// the measured staleness above. The second is that the repair does not exist:
	// re-running write-once fills against a cached node duplicates an anchored
	// child hole, morphing cannot see the event or property sinks and has no DOM
	// in this tree to be verified against, and neither would move corpus family 13
	// to MATCH anyway, because that divergence is the eager Rust loop and not the
	// reconcile. The full chain is in this file's header under "THE HISTORY OF
	// THIS ASSERTION"; read it before flipping this a third time.

	checks.ok(
		"the list element is still the one the server sent: hydration adopted it and kept it",
		claimedList === mount.querySelector("ul.movers"),
	);
	checks.is(
		"and its ROWS were rebuilt, not moved -- this list is deliberately NOT keyed, because a keyed row keeps the node it had, text included, and would show stale prices for ever (fixture.json expected.$notKeyed)",
		rowNodes(mount).filter(node => openingNodes.includes(node)).length,
		0,
	);
	checks.ok(
		"which is what makes the prices above correct rather than merely re-ordered",
		expected.notKeyed === true,
		"fixture.json no longer says this list is unkeyed; if that changed, read the price assertions in section 3 first",
	);

	// ---- 5. the updates were surgical ----

	const mutations = drain();
	checks.ok(
		"every mutation landed inside the movers list",
		mutations.length > 0 && mutations.every(record => record.where.includes("ul") || record.where.includes("li")),
		`mutations landed outside the list: ${JSON.stringify(mutations.map(record => record.where))}`,
	);

	// ---- 6. THE MALFORMED FRAME: it must not break the island ----

	const errorAt = frames.findIndex(tick => "malformed" in tick);
	const loggedBefore = logged.length;
	const errorsBefore = stream.errors.length;
	const rowsBefore = rowsOf(mount);

	send(frames[errorAt]);
	await after(0);

	checks.ok("a malformed frame does not close the subscription", !source.closed);
	checks.is(
		"and does not change the list: the last good frame is still on screen",
		rowsOf(mount),
		rowsBefore,
	);
	checks.ok(
		"the frame was REJECTED rather than partly decoded",
		stream.errors.length > errorsBefore,
		"nothing threw, which means the island accepted a truncated frame -- a partly decoded tick is worse than a rejected one, because it writes a wrong price rather than no price",
	);
	const during = logged.slice(loggedBefore);
	checks.note(
		`ON THE MALFORMED FRAME: the listener threw ${stream.errors.length - errorsBefore} time(s)`
		+ ` (${JSON.stringify(String(stream.errors[errorsBefore] ?? "").slice(0, 90))})`
		+ ` and the page logged ${during.length} message(s)`
		+ `${during.length ? `: ${JSON.stringify(during.map(message => `${message.level}: ${message.text}`))}` : ""}.`
		+ "\n        What the island DOES with a bad frame is the framework agent's choice and is RECORDED rather than"
		+ " required; what it may not do is stop consuming the stream, which the recovery below measures.",
	);

	// ---- 7. RECOVERY ----

	const recoveryAt = frames.length - 1;
	send(frames[recoveryAt]);
	await after(0);
	checks.is(
		"a good frame after a bad one still re-ranks and re-prices: the island is still listening",
		rowsOf(mount),
		feed.expectedRows[recoveryAt],
	);
	checks.is(
		"and the classes recovered with it",
		classesOf(mount),
		feed.expectedClasses[recoveryAt],
	);

	// ---- 8. THE FOREIGN CALL TRACE ----
	//
	// Compared position by position rather than by longest-common-subsequence, for
	// the reason harness/check-js-extern.mjs's own comparator gives: two emitters
	// can order independent runtime calls differently and both be correct, but this
	// is the sequence of operations one expression performed on one object, and
	// reordering it changes what happened.

	const want = declaredShapes(chart);
	const trace = chart_lib.__trace();
	checks.is(
		"the chart received exactly the operations the fixture declares, in order",
		trace.map(shapeOf),
		want,
	);
	checks.is(
		"and no more than those -- one construction and five updates for a hydration run plus five frames, because the malformed frame must not reach the chart at all",
		trace.length,
		want.length,
	);
	checks.ok(
		"the constructor was reached through the import binding the descriptor named",
		trace[0]?.via === want[0]?.via,
		"JS-EXTERN.md's new-named and new-default differ ONLY in this field, so it is the whole record of which binding was taken",
	);

	// The series, by value. A `#[repr(C)]` struct crosses as an object keyed by its
	// RUST FIELD NAMES, so this is also the assertion that the value model spelled
	// the struct the way CONTRACT.md says it does.
	checks.is(
		"each update carried the current price of every symbol, keyed by the struct's own field names",
		trace.filter(record => record.op === "send").map(record => record.args?.[0]),
		chart.expectedSeries,
	);
}

/**
 * What the dashboard looks like when its hydration key did not match.
 *
 * Run by the negative controls. These checks PASS -- they assert the failure mode
 * -- and the control's own pass condition is that the named parity check in
 * run.mjs failed.
 *
 * A key miss is not death, it is silent loss of hydration: `getNextElement` misses
 * the registry, falls back to `template()`, and the fresh clone replaces the
 * server's node. The island then works.
 *
 * @param {object} context same shape as `interact`
 */
export async function dead({ mount, checks, expected }) {
	// Queried AFTER hydration, deliberately: on a key miss the element the server
	// sent has been replaced, so this is the client's own list.
	const list = mount.querySelector("ul.movers");
	checks.ok("there is still a movers list, because the client rebuilt one", !!list);
	checks.is(
		"and it holds exactly the rows the server's did, because the client's first run answers the same five",
		rowsOf(mount),
		expected.openingRows,
	);
	checks.note(
		"This island is the WORST case in the suite for the silent-loss failure mode, and it is worth saying so\n"
		+ "        where somebody will read it. The counter at least re-renders a value a user was looking at, and\n"
		+ "        search's list is empty until a reply arrives. Here the client's first run reproduces the server's\n"
		+ "        five rows EXACTLY -- that agreement is the island's own design goal and is what makes\n"
		+ "        hydrateMutations 0 -- so a completely unhydrated dashboard and a correctly hydrated one are\n"
		+ "        byte-identical on screen and stay that way. Only `_$HY.completed` and the per-key identity\n"
		+ "        check can tell them apart.",
	);
}
