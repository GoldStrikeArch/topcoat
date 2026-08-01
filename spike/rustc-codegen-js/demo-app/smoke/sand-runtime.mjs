// The runtime names the compiled sand island calls, and the canvas it draws on.
//
// Its own stub rather than a shared one, for two reasons. The node graph is a
// shape of its own -- the island's two holes are spans the compiled code
// reaches by walking `firstChild` and `nextSibling` past two text nodes --
// and, more to the point, this island DRAWS. A canvas cannot be pretended at:
// the whole claim being made is that the picture is decided by compiled Rust, so
// the surface has to be a RECORDER. It draws nothing and writes down every
// operation performed on it, which turns "what did the island paint" into a
// value a check can assert instead of a picture somebody has to look at. That is
// `contract/fixtures/js-extern/chart-lib.mjs`'s arrangement, for a canvas.
//
// `template(html)` PARSES the html it is given rather than pretending: it is
// the emitter's own declaration, so the graph the compiled code walks is the graph the emitter
// said it would walk, and a stub cannot quietly agree with a shape the compiler
// does not actually emit.
//
// WHAT THIS DOES NOT MEASURE. Hydration. `insert` replaces a parent's children
// rather than adopting the server's, so the number of DOM mutations a hydrate
// performs is not observable here; that is the parity harness's question and it
// asks it against the served page. What this measures is the LOOP: what the two
// holes render, what the canvas was asked to paint, and what the boards hold.

/// The effect currently running, so reading a signal subscribes it.
let listener = null;

export function createSignal(initial) {
	let value = initial;
	const subscribers = new Set();
	return [
		() => {
			if (listener) subscribers.add(listener);
			return value;
		},
		next => {
			if (next === value) return;
			value = next;
			for (const run of [...subscribers]) run();
		},
	];
}

export function effect(fn) {
	const run = () => {
		const previous = listener;
		listener = run;
		try {
			return fn();
		} finally {
			listener = previous;
		}
	};
	return run();
}

// ---------------------------------------------------------------- the graph

let nextId = 0;

function node(kind, extra = {}) {
	return { id: nextId++, kind, firstChild: null, nextSibling: null, parentNode: null, ...extra };
}

/// Links `children` into `parent` as a sibling chain, the way a parsed document
/// has them.
///
/// Every link is cleared before any is made: relinking a node that is already
/// in the chain without clearing first leaves a stale `nextSibling` pointing at
/// a node now earlier in the list, which is a cycle.
function adopt(parent, children) {
	parent.firstChild = null;
	for (const child of children) child.nextSibling = null;

	let previous = null;
	for (const child of children) {
		child.parentNode = parent;
		if (previous) previous.nextSibling = child;
		else parent.firstChild = child;
		previous = child;
	}
	parent.children = children;
	return parent;
}

export const element = (tag, attributes = {}, children = []) =>
	adopt(node("element", { tag, nodeName: tag, attributes, text: null, rendered: null }), children);

export const text = data => node("text", { data });

export const comment = value => node("comment", { nodeType: 8, nodeValue: value, data: value });

/// The node graph one template string describes.
///
/// Only what a `view!` emits: elements with quoted attributes, text, and the
/// `<!$>` / `<!/>` hole markers. An unknown shape throws rather than being
/// skipped, so a template this cannot read is a failure and not a silent empty
/// island.
export function parse(html) {
	const stack = [{ tag: "#fragment", children: [] }];
	let at = 0;

	while (at < html.length) {
		if (html[at] !== "<") {
			const end = html.indexOf("<", at);
			const stop = end === -1 ? html.length : end;
			stack.at(-1).children.push(text(html.slice(at, stop)));
			at = stop;
			continue;
		}

		if (html.startsWith("<!", at)) {
			const end = html.indexOf(">", at);
			if (end === -1) throw new Error(`unterminated marker in ${html}`);
			stack.at(-1).children.push(comment(html.slice(at + 2, end)));
			at = end + 1;
			continue;
		}

		if (html.startsWith("</", at)) {
			const end = html.indexOf(">", at);
			const open = stack.pop();
			if (stack.length === 0 || open.tag !== html.slice(at + 2, end)) {
				throw new Error(`mismatched </${html.slice(at + 2, end)}> in ${html}`);
			}
			stack.at(-1).children.push(element(open.tag, open.attributes, open.children));
			at = end + 1;
			continue;
		}

		const end = html.indexOf(">", at);
		if (end === -1) throw new Error(`unterminated tag in ${html}`);
		const inner = html.slice(at + 1, end);
		const [tag, ...rest] = inner.split(/\s+/);
		const attributes = {};
		for (const [, name, value] of rest.join(" ").matchAll(/([\w-]+)="([^"]*)"/g)) {
			attributes[name] = value;
		}
		at = end + 1;

		// A `view!` writes every element it closes, so anything without a closing
		// tag is void and has no children.
		if (VOID.has(tag)) {
			stack.at(-1).children.push(element(tag, attributes, []));
			continue;
		}
		stack.push({ tag, attributes, children: [] });
	}

	if (stack.length !== 1) throw new Error(`unclosed <${stack.at(-1).tag}> in ${html}`);
	const roots = stack[0].children;
	if (roots.length !== 1) throw new Error(`expected one root in ${html}`);
	return roots[0];
}

const VOID = new Set(["input", "img", "br", "hr", "meta", "link", "source"]);

// ------------------------------------------------- the markup the server sent
//
// ONE written-down shape, used twice. `TEMPLATE_HTML` is what the chunk declares
// and `served` is what the server renders, and the second is built out of the
// first by filling the two holes with the values the server's halves answer. So
// the check's idea of the server's markup cannot drift from its idea of the
// client's: there is one string, and the substitution is the only difference
// between them. A substitution that finds nothing throws, which is what makes an
// edit to the view a failure here rather than a check that quietly stops
// checking anything.

/// The markup the chunk declares, hole for hole.
export const TEMPLATE_HTML =
	'<div class="island sand">'
	+ '<canvas class="sand-canvas" width="360" height="240"></canvas>'
	+ '<p class="sand-count">grains <span class="sand-grains"></span>'
	+ ' brush <span class="sand-brush"></span></p>'
	+ '<div class="island-controls">'
	+ '<button class="island-step" type="button">sand</button>'
	+ '<button class="island-step" type="button">wall</button>'
	+ '<button class="island-step" type="button">erase</button>'
	+ "</div></div>";

/// The same markup with the two holes filled, which is what the server sends.
export function servedHtml(grains, brush) {
	return fill(fill(TEMPLATE_HTML, "sand-grains", grains), "sand-brush", brush);
}

function fill(html, hole, value) {
	const empty = `<span class="${hole}"></span>`;
	if (!html.includes(empty)) throw new Error(`the sand template no longer has a ${hole} hole`);
	return html.replace(empty, `<span class="${hole}">${value}</span>`);
}

// --------------------------------------------------------- the drawing surface
//
// The canvas is the node the template already has rather than a second object
// beside it, so `document.querySelector` hands back the element the island is
// also walking. A canvas the check invented would be a canvas that cannot be
// the wrong one.

/// Every operation the island performed on the context, in order.
export const painted = [];

/// Every listener the island registered on the canvas.
export const listeners = [];

/// Every lookup the island made through `document` or on the canvas.
export const asked = [];

/// Every clock the island opened, as `{ ms, run }`; `run` is the function the
/// interval would have called.
export const clocks = [];

/// Where the canvas sits in the viewport, which is what turns a client
/// coordinate into a cell.
let rectangle = { left: 0, top: 0 };

/// Puts the canvas at `left`, `top`.
export function place(left, top) {
	rectangle = { left, top };
}

/// Gives `canvas` the surface the island expects to find on it.
function surface(canvas) {
	let colour = null;
	const context = {
		set fillStyle(next) {
			colour = next;
			painted.push({ op: "fillStyle", colour: next });
		},
		get fillStyle() {
			return colour;
		},
		fillRect(x, y, width, height) {
			painted.push({ op: "fillRect", x, y, width, height, colour });
		},
	};

	canvas.getContext = kind => {
		asked.push({ op: "getContext", kind });
		return context;
	};
	canvas.addEventListener = (kind, handler) => {
		listeners.push({ kind, handler });
	};
	canvas.getBoundingClientRect = () => ({
		left: rectangle.left,
		top: rectangle.top,
		right: rectangle.left + 360,
		bottom: rectangle.top + 240,
		width: 360,
		height: 240,
	});
	return canvas;
}

/// The document the island reaches at global scope.
///
/// Handed back rather than installed: the compiled code reads `document` as a
/// global, exactly as a page would, and standing it up at global scope is the
/// caller's job so that the reach is visible in the check rather than hidden in
/// a stub. A declaration that had quietly imported it instead would find nothing
/// there.
export function documentOf(page) {
	return {
		querySelector(selector) {
			asked.push({ op: "querySelector", selector });
			return selector === ".sand-canvas" ? page.canvas : null;
		},
	};
}

/// A clock that records instead of scheduling, for `globalThis.setInterval`.
///
/// The island's frames are then driven by the check calling what the interval
/// would have called, which measures the wiring as well as the physics: a frame
/// that ran because the check wrote the signal would prove nothing about
/// `clock_start`. Nothing real is scheduled, so nothing is left running either.
export function recordInterval(run, ms) {
	clocks.push({ ms, run });
	return 0;
}

// ------------------------------------------- the runtime names the emitter calls

export const calls = [];

/// The parent of every hole render, in order, so a check can say which hole ran
/// and which did not.
///
/// The two holes subscribe to disjoint sets of signals, which is a claim about
/// what does NOT happen: a pointer move must not run the physics and a frame
/// must not re-stamp. Nothing else here can see a hole that stayed still.
export const renders = [];

let page = null;
let claimed = false;

/// Puts a fresh page under the island and forgets every recording.
///
/// `calls` is not cleared: a template is declared once, when the module is
/// loaded, so clearing it would throw away the only record of that.
export function reset(grains, brush) {
	const root = parse(servedHtml(grains, brush));
	const canvas = find(root, "canvas");
	page = {
		root,
		canvas: surface(canvas),
		grains: find(root, "span", "sand-grains"),
		brush: find(root, "span", "sand-brush"),
		buttons: [...walk(root)].filter(entry => entry.tag === "button"),
	};
	claimed = false;
	painted.length = 0;
	listeners.length = 0;
	asked.length = 0;
	clocks.length = 0;
	renders.length = 0;
	return page;
}

/// Every element below `root`, in document order.
function* walk(root) {
	for (let child = root.firstChild; child; child = child.nextSibling) {
		if (child.kind !== "element") continue;
		yield child;
		yield* walk(child);
	}
}

/// The one element with this tag and class. Two would be ambiguous and none
/// would be a template that no longer says what this file thinks it says, so
/// both throw.
function find(root, tag, className = null) {
	const hits = [...walk(root)].filter(
		entry => entry.tag === tag && (className === null || entry.attributes.class === className),
	);
	if (hits.length !== 1) throw new Error(`expected one <${tag} class="${className}">, found ${hits.length}`);
	return hits[0];
}

export function template(html) {
	calls.push({ op: "template", html });
	const cloner = () => parse(html);
	cloner.html = html;
	return cloner;
}

export function getNextElement(cloner) {
	calls.push({ op: "getNextElement", html: cloner.html });
	if (claimed) return cloner();
	claimed = true;
	return page.root;
}

export function getNextMarker(start) {
	return [start, []];
}

export function insert(parent, accessor) {
	calls.push({ op: "insert", parent: describe(parent) });
	if (typeof accessor !== "function") {
		parent.rendered = String(accessor);
		return;
	}
	effect(() => {
		renders.push(parent);
		const value = accessor();
		// A `for` hole hands back the rows it built, which for a row that is one
		// interpolated value is the value itself rather than a node.
		if (Array.isArray(value)) adopt(parent, value.map(entry => (entry?.kind ? entry : text(String(entry)))));
		else parent.rendered = String(value);
	});
}

export function delegateEvents(events) {
	calls.push({ op: "delegateEvents", events });
}

export const setAttribute = (el, name, value) => {
	el.attributes[name] = value;
};
export const setBoolAttribute = setAttribute;
export const className = (el, value) => setAttribute(el, "class", value);
export const setProperty = (el, name, value) => {
	el[name] = value;
};
export const classList = () => {};
export const style = () => {};
export const spread = () => {};
export const assign = () => {};
export const use = () => {};
export const addEventListener = () => {};
export const dynamicProperty = () => {};
export const innerHTML = () => {};
export const memo = fn => fn;
export const untrack = fn => fn();
export const getOwner = () => null;
export const createComponent = (fn, props) => fn(props);
export const mergeProps = (...parts) => Object.assign({}, ...parts);
export const getNextMatch = el => el;
export const runHydrationEvents = () => {};
export const getHydrationKey = () => "";
export const NoHydration = props => props.children;
export const hydrate = code => code();
export const render = code => code();

// --------------------------------------------------------------- reading back

/// Everything a node renders, in document order.
///
/// A value written by `insert` counts as the node's text, which is how a hole
/// filled after the graph was built is read back.
export function textOf(target) {
	if (!target) return "";
	if (target.kind === "text") return target.data;
	if (target.kind === "comment") return "";
	let out = target.rendered === null ? "" : target.rendered;
	for (let child = target.firstChild; child; child = child.nextSibling) {
		out += textOf(child);
	}
	return out;
}

function describe(target) {
	if (!target) return "null";
	if (target.kind === "element") return `<${target.tag}>`;
	if (target.kind === "text") return `"${target.data}"`;
	return `<!--${target.nodeValue}-->`;
}
