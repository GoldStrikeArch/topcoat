// The runtime names the compiled dashboard island calls, over a node graph built
// from the emitter's own template strings.
//
// Its own stub rather than `stub.mjs`'s or `search-runtime.mjs`'s, and for a
// reason neither of those had: this island's rows are STRUCTURED. A row is a
// `<li>` of three spans and the compiled code reaches each span by walking
// `firstChild` and `nextSibling` from the row's root, so a stub that hands back a
// featureless object cannot run it. The other two islands render one value into
// one slot and never walk anything.
//
// So `template(html)` here PARSES the html it is given rather than pretending. It
// is the emitter's own declaration, so the graph the compiled code walks is the
// graph the emitter said it would walk, and a stub cannot quietly agree with a
// shape the compiler does not actually emit.
//
// WHAT THIS DOES NOT MEASURE. Hydration. `insert` replaces a parent's children
// rather than adopting the server's, so the number of DOM mutations a hydrate
// performs is not observable here; that is the parity harness's question and it
// asks it against the served page. What this measures is the LOOP: which rows
// exist, in what order, carrying what text, and whether a row that moves keeps
// the node it had.

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
/// Every link is cleared before any is made. A re-render may hand back nodes
/// that are already in the chain, and relinking those without clearing first
/// leaves a stale `nextSibling` pointing at a node now earlier in the list,
/// which is a cycle: walking it never ends.
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
/// row.
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

/// The dashboard as the server renders it: a chart canvas and five rows at their
/// opening prices, in slot order, with nothing having moved.
export function served(symbols, prices) {
	const rows = symbols.map((symbol, slot) =>
		element("li", { class: "mover" }, [
			element("span", { class: "mover-symbol" }, [text(symbol)]),
			element("span", { class: "mover-price" }, [text(String(prices[slot]))]),
			element("span", { class: "mover-delta" }, [text("0")]),
		]),
	);
	const canvas = element("canvas", { class: "dash-chart" }, []);
	const list = element("ul", { class: "movers" }, rows);
	return { root: element("div", { class: "island dashboard" }, [canvas, list]), canvas, list };
}

let page = null;
let claimed = false;

export const calls = [];

/// Puts a fresh page under the island. `calls` is not cleared: a template is
/// declared once, when the module is loaded.
export function reset(symbols, prices) {
	page = served(symbols, prices);
	claimed = false;
	nextId = 0;
	return page;
}

// ------------------------------------------- the runtime names the emitter calls

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

export function insert(parent, accessor, marker, initial) {
	calls.push({ op: "insert", parent: describe(parent) });
	if (typeof accessor !== "function") {
		parent.rendered = String(accessor);
		return;
	}
	effect(() => {
		const value = accessor();
		if (Array.isArray(value)) adopt(parent, value);
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
	const own = target.rendered === null ? "" : target.rendered;
	let out = own;
	for (let child = target.firstChild; child; child = child.nextSibling) {
		out += textOf(child);
	}
	return out;
}

/// The rows of the movers list, as `{ node, symbol, price, delta, class }`.
///
/// `node` is the identity a keyed loop is supposed to preserve, so it is handed
/// back rather than described.
export function rows(list) {
	const out = [];
	for (let row = list.firstChild; row; row = row.nextSibling) {
		const cells = [];
		for (let cell = row.firstChild; cell; cell = cell.nextSibling) cells.push(textOf(cell));
		out.push({
			node: row,
			symbol: cells[0] ?? "",
			price: cells[1] ?? "",
			delta: cells[2] ?? "",
			class: row.attributes.class ?? "",
		});
	}
	return out;
}

function describe(target) {
	if (!target) return "null";
	if (target.kind === "element") return `<${target.tag}>`;
	if (target.kind === "text") return `"${target.data}"`;
	return `<!--${target.nodeValue}-->`;
}
