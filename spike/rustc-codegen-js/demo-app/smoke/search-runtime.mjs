// The runtime names the compiled search island calls, over a node graph shaped
// like the HTML the server sends for it.
//
// Its own stub rather than `stub.mjs`'s: that one is the counter's graph and its
// `insert` renders one value into a text slot, while this island inserts a LIST
// of created nodes into an element the server sent empty. Two islands, two
// shapes, and folding them together would make each check less clear about what
// it is measuring.

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

/// One element of the graph.
function element(tag, attributes = {}) {
	return { tag, attributes, children: [], text: null, firstChild: null, nextSibling: null };
}

/// The markup the server sends: an empty box and an empty list.
export function served() {
	const input = element("input", { class: "search-input" });
	const list = element("ul", { class: "search-results" });
	const root = element("div", { class: "island" });
	root.children = [input, list];
	root.firstChild = input;
	input.nextSibling = list;
	return { root, input, list };
}

let page = served();
export const calls = [];

/// Puts a fresh page under the island, so a run starts from the server's own
/// markup.
///
/// `calls` is not cleared: a template is declared once, when the module is
/// loaded, so clearing would throw away the record of it.
export function reset() {
	page = served();
	claimed = false;
	return page;
}

/// The island's own root is the server's node; anything else is created,
/// because the server never wrote it.
let claimed = false;

export function template(html) {
	calls.push({ op: "template", html });
	const cloner = () => element(html.slice(1, html.indexOf(" ")));
	cloner.html = html;
	return cloner;
}

export function getNextElement(cloner) {
	calls.push({ op: "getNextElement", html: cloner.html });
	if (claimed) return cloner();
	claimed = true;
	return page.root;
}

export function insert(parent, accessor) {
	calls.push({ op: "insert" });
	if (typeof accessor !== "function") {
		parent.text = String(accessor);
		return;
	}
	effect(() => {
		parent.children = accessor();
	});
}

export function delegateEvents(events) {
	calls.push({ op: "delegateEvents", events });
}

export const setAttribute = () => {};
export const setBoolAttribute = () => {};
export const setProperty = () => {};
export const className = () => {};
export const classList = () => {};
export const style = () => {};
export const spread = () => {};
export const assign = () => {};
export const use = () => {};
export const addEventListener = () => {};
export const dynamicProperty = () => {};
export const memo = fn => fn;
export const untrack = fn => fn();
export const getOwner = () => null;
export const createComponent = (fn, props) => fn(props);
export const mergeProps = (...parts) => Object.assign({}, ...parts);
export const getNextMarker = start => [start, []];
export const getNextMatch = el => el;
export const runHydrationEvents = () => {};
export const getHydrationKey = () => "";
export const hydrate = code => code();
export const render = code => code();
