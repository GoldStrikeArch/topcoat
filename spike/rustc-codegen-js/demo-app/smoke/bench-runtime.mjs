// The runtime names the compiled benchmark island calls, over a node graph
// PARSED out of the two templates the chunk declares.
//
// Life's stub and this one are the same idea and deliberately not the same file.
// The reason is `calls`: a stub records every runtime operation it was asked to
// perform, and a table of ten thousand rows performs a hundred thousand of them.
// Sharing the recorder with another island's checks would mean each of them
// searching the other's, and the first symptom would be a check that got slower
// and then found the wrong template.
//
// What is modelled is what the emitted chunk reaches for: a template, the walk
// down it, an insert, an attribute, and a handler assigned as a property. The
// graph is parsed from the template string the chunk itself declares rather than
// written out by hand, for the reason life's stub gives: a hand written graph is
// a second copy of the markup that somebody has to keep in step, and a mistake in
// it looks exactly like a bug in the island.
//
// What is deliberately NOT modelled: layout, event dispatch and the hydration
// registry. A handler is a property the emitter assigns, so a check calls it.

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

// ------------------------------------------------------------------ the parser

/// The elements that never have a closing tag.
const VOID = new Set(["input", "br", "hr", "img", "meta", "link"]);

/// One element of the graph. `childNodes` is the markup's own structure, which
/// the emitter's walk follows; `children` and `text` are what an insert put
/// there.
function element(tag) {
	return {
		tag,
		attributes: {},
		childNodes: [],
		children: [],
		text: null,
		firstChild: null,
		nextSibling: null,
	};
}

/// A text node, which exists so that the walk counts it.
function text(data) {
	return { tag: "#text", data, childNodes: [], firstChild: null, nextSibling: null };
}

/// The `<!$>` and `<!/>` a hole is bracketed by, and the `<!>` that keeps two
/// text nodes apart.
function comment(data) {
	return { tag: "#comment", data, childNodes: [], firstChild: null, nextSibling: null };
}

/// The attributes of one start tag, as the template writes them: `name="value"`,
/// double quoted, or a bare name.
function attributes(rest) {
	const found = {};
	const pattern = /([a-zA-Z][a-zA-Z0-9-]*)(?:="([^"]*)")?/g;
	for (const [, name, value] of rest.matchAll(pattern)) {
		found[name] = value ?? "";
	}
	return found;
}

/// Links `firstChild` and `nextSibling` down the tree, which is the only way the
/// emitted code ever moves through it.
function link(node) {
	node.firstChild = node.childNodes[0] ?? null;
	for (let at = 0; at < node.childNodes.length; at++) {
		node.childNodes[at].nextSibling = node.childNodes[at + 1] ?? null;
		link(node.childNodes[at]);
	}
	return node;
}

/// The node graph `html` describes.
export function parse(html) {
	const root = element("#root");
	const open = [root];
	let at = 0;

	while (at < html.length) {
		const lt = html.indexOf("<", at);
		if (lt < 0) {
			open.at(-1).childNodes.push(text(html.slice(at)));
			break;
		}
		if (lt > at) open.at(-1).childNodes.push(text(html.slice(at, lt)));

		const gt = html.indexOf(">", lt);
		if (gt < 0) throw new Error(`unterminated tag in template: ${html.slice(lt, lt + 40)}`);
		const raw = html.slice(lt + 1, gt);
		at = gt + 1;

		if (raw.startsWith("!")) {
			open.at(-1).childNodes.push(comment(raw));
			continue;
		}
		if (raw.startsWith("/")) {
			open.pop();
			continue;
		}

		const space = raw.search(/[\s/]/);
		const tag = (space < 0 ? raw : raw.slice(0, space)).toLowerCase();
		if (!/^[a-z][a-z0-9-]*$/.test(tag)) {
			throw new Error(`the bench stub does not parse the tag ${JSON.stringify(tag)}`);
		}
		const node = element(tag);
		node.attributes = space < 0 ? {} : attributes(raw.slice(space));
		open.at(-1).childNodes.push(node);
		if (!VOID.has(tag) && !raw.endsWith("/")) open.push(node);
	}

	if (open.length !== 1) throw new Error("unbalanced tags in template");
	return link(root).childNodes[0];
}

/// The first element below `node` carrying `name` in its class attribute.
///
/// By class rather than by position, so a check names what it is asserting about
/// instead of counting siblings.
export function find(node, name) {
	if (node.attributes && (node.attributes.class ?? "").split(" ").includes(name)) return node;
	for (const child of node.childNodes) {
		const found = find(child, name);
		if (found) return found;
	}
	return null;
}

/// The first element below `node` whose id attribute is `id`.
///
/// The benchmark's six buttons are named by id and nothing else, because that is
/// how the harness presses them. A check that found them by position would pass
/// with the ids wrong, which is the one way this island can be broken and look
/// fine.
export function byId(node, id) {
	if (node.attributes && node.attributes.id === id) return node;
	for (const child of node.childNodes) {
		const found = byId(child, id);
		if (found) return found;
	}
	return null;
}

// ---------------------------------------------------------- the runtime names

export const calls = [];

/// Whether the island's own root has been claimed.
///
/// The island's root is the server's node; every row the table's loop builds is
/// created, because the server sent an empty table.
let claimed = false;

/// How many nodes have been instantiated from a template.
///
/// A count rather than a record, because the number this island is about is in
/// the thousands: one write to the island's signal rebuilds every row, and this
/// is what lets a check say so instead of taking it on trust.
export const built = { nodes: 0 };

/// Puts a fresh page under the island, so a run starts from the server's markup.
///
/// `calls` is not cleared: a template is declared once, when the module is
/// loaded, and clearing would throw away the record of it.
export function reset() {
	claimed = false;
	built.nodes = 0;
}

export function template(html) {
	calls.push({ op: "template", html });
	const cloner = () => parse(html);
	cloner.html = html;
	return cloner;
}

export function getNextElement(cloner) {
	built.nodes++;
	if (claimed) return cloner();
	claimed = true;
	return cloner();
}

export function insert(parent, accessor) {
	if (typeof accessor !== "function") {
		fill(parent, accessor);
		return;
	}
	effect(() => fill(parent, accessor()));
}

/// What an insert leaves behind: a list of rows, or one value as text.
function fill(parent, value) {
	if (Array.isArray(value)) {
		parent.children = value;
		return;
	}
	parent.text = String(value);
}

export function setAttribute(node, name, value) {
	node.attributes[name] = String(value);
}

export function delegateEvents(events) {
	calls.push({ op: "delegateEvents", events });
}

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
