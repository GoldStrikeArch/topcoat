// The runtime names the compiled Minesweeper island calls, over a node graph
// built from the chunk's own templates.
//
// Its own stub rather than `stub.mjs`: this island's rows have children of
// their own -- a note is a span
// with two spans in it, and the emitted code walks `firstChild`/`nextSibling`
// through them -- so a cloner that makes a bare element is not enough.
//
// WHY THE GRAPH IS PARSED RATHER THAN WRITTEN OUT
//
// `template(html)` is handed the exact markup the server sends for that part of
// the island, so parsing it is how the graph this hydrates into keeps the shape
// the server wrote without anybody holding a second copy in sync. A graph spelled
// out by hand would go on passing after the view changed under it.
//
// Only what these templates contain is implemented: open tags with double quoted
// attributes, close tags, and text. Everything a real parser worries about --
// void elements, entities, self closing tags, case folding -- is absent from the
// templates and absent from here, and an unparsable template is an error rather
// than a shrug.
//
// WHAT IS NOT RECORDED. `calls` holds the operations that happen once -- the
// template declarations and the delegated event registration. The per-node ones
// are left out on purpose: this island rebuilds 120 cells on every render and a
// win takes a hundred clicks, so recording them would be a hundred thousand
// entries nothing reads. That the server's own node was claimed rather than
// cloned is asserted directly instead, on the node the entry point returns.

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

/// One element of the graph.
function element(tag, attributes) {
	return { tag, attributes, children: [], text: null, firstChild: null, nextSibling: null };
}

/// Links `children` under `parent` the way the emitted code walks them.
function adopt(parent, children) {
	parent.children = children;
	parent.firstChild = children[0] ?? null;
	for (let index = 0; index < children.length; index++) {
		children[index].nextSibling = children[index + 1] ?? null;
	}
	return parent;
}

/// The node graph a template's markup describes.
///
/// Text is kept as the element's `text` rather than as a node of its own, which
/// is all these templates need: the only text in them is the restart button's
/// label, and nothing walks past it.
export function parse(html) {
	const stack = [];
	const roots = [];
	let at = 0;

	while (at < html.length) {
		if (html[at] !== "<") {
			const end = html.indexOf("<", at);
			const text = html.slice(at, end === -1 ? html.length : end);
			if (!stack.length) throw new Error(`mines-runtime: text outside an element in ${html}`);
			if (text.trim()) stack[stack.length - 1].text = text;
			at = end === -1 ? html.length : end;
			continue;
		}

		const end = html.indexOf(">", at);
		if (end === -1) throw new Error(`mines-runtime: unterminated tag in ${html}`);
		const inner = html.slice(at + 1, end);
		at = end + 1;

		if (inner.startsWith("/")) {
			const done = stack.pop();
			if (!done) throw new Error(`mines-runtime: stray close tag in ${html}`);
			adopt(done, done.children);
			continue;
		}

		const [tag, ...rest] = inner.split(/\s+/);
		const attributes = {};
		for (const [, name, value] of rest.join(" ").matchAll(/([a-zA-Z-]+)="([^"]*)"/g)) {
			attributes[name] = value;
		}
		const node = element(tag, attributes);
		if (stack.length) stack[stack.length - 1].children.push(node);
		else roots.push(node);
		stack.push(node);
	}

	if (stack.length) throw new Error(`mines-runtime: unclosed tag in ${html}`);
	if (roots.length !== 1) throw new Error(`mines-runtime: ${roots.length} roots in ${html}`);
	return roots[0];
}

/// A fresh copy of a parsed shape, which is what cloning a template is.
function clone(shape) {
	const copy = element(shape.tag, { ...shape.attributes });
	copy.text = shape.text;
	return adopt(copy, shape.children.map(clone));
}

/// The markup the server sent, once the island's own template has been seen.
let page = null;

/// Whether the island's root has been claimed.
let claimed = false;

/// The operations that happen once. See the note at the top of the file.
export const calls = [];

/// Puts a fresh page under the island, so a run starts from the server's own
/// markup.
///
/// `html` is the island's own template, which is the markup the server sends with
/// the reactive holes left empty. `calls` is not cleared: a template is declared
/// when the module is loaded, so clearing would throw away the record of it.
export function reset(html) {
	page = parse(html);
	claimed = false;
	return page;
}

/// The island's root, as it stands.
export function root() {
	return page;
}

/// The markup a node stands for, with its attributes in name order.
///
/// Canonical rather than literal: the client sets `class` and `value` after
/// cloning the template, so their order is the emitter's and the server's is the
/// view's. Nothing downstream of either can tell the difference, and sorting is
/// what lets the two be compared at all.
export function html(node) {
	const attributes = Object.keys(node.attributes)
		.sort()
		.map(name => ` ${name}="${node.attributes[name]}"`)
		.join("");
	const inside = node.children.length ? node.children.map(html).join("") : (node.text ?? "");
	return `<${node.tag}${attributes}>${inside}</${node.tag}>`;
}

// ------------------------------------- the runtime names the emitter calls

export function template(markup) {
	calls.push({ op: "template", html: markup });
	const shape = parse(markup);
	const cloner = () => clone(shape);
	cloner.html = markup;
	return cloner;
}

export function getNextElement(cloner) {
	if (claimed) return cloner();
	claimed = true;
	return page;
}

export function insert(parent, accessor) {
	if (typeof accessor !== "function") {
		parent.text = String(accessor);
		return;
	}
	effect(() => adopt(parent, accessor()));
}

export function setAttribute(el, name, value) {
	el.attributes[name] = String(value);
}

export function delegateEvents(events) {
	calls.push({ op: "delegateEvents", events });
}

export const setBoolAttribute = setAttribute;
export const setProperty = (el, name, value) => {
	el[name] = value;
};
export const className = (el, value) => setAttribute(el, "class", value);
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
