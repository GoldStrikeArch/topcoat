// The runtime names the emitter calls, implemented over a node graph shaped like
// the HTML the server sends. Driven by check.mjs.
//
// Not a DOM: only what the compiled code touches is implemented, which is
// `firstChild`, `nextSibling`, a text slot, and property assignment. The
// reactivity is the smallest thing that can be called a signal. `getNextMarker`
// follows the real runtime's depth-counting semantics, so the node range it
// returns is the range the real one would return.

// ---------- reactivity ----------

let listener = null;

export function createSignal(initial) {
	let value = initial;
	const subscribers = new Set();
	const read = () => {
		if (listener) subscribers.add(listener);
		return value;
	};
	const write = next => {
		if (next === value) return;
		value = next;
		for (const fn of [...subscribers]) fn();
	};
	return [read, write];
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

export const memo = fn => fn;
export const untrack = fn => fn();
export const getOwner = () => null;
export const createComponent = (fn, props) => fn(props);
export const mergeProps = (...parts) => Object.assign({}, ...parts);

// ---------- the node graph the server sent ----------

let nextId = 0;

function node(kind, extra = {}) {
	return {
		id: nextId++,
		kind,
		firstChild: null,
		nextSibling: null,
		parentNode: null,
		...extra,
	};
}

function element(tag, children) {
	const el = node("element", { tag, text: null });
	let previous = null;
	for (const child of children) {
		child.parentNode = el;
		if (previous) previous.nextSibling = child;
		else el.firstChild = child;
		previous = child;
	}
	return el;
}

const text = value => node("text", { data: value });
const comment = value => node("comment", { nodeType: 8, nodeValue: value, data: value });

// <div class="island"><p class="island-count">count <!--$-->5<!--/--></p>
//   <div class="island-controls"><button>-1</button><button>+1</button></div></div>
export const decrement = element("button", [text("-1")]);
export const increment = element("button", [text("+1")]);
export const display = element("p", [
	text("count "),
	comment("$"),
	text("5"),
	comment("/"),
]);
export const root = element("div", [
	display,
	element("div", [decrement, increment]),
]);

// ---------- the runtime names the emitter calls ----------

export const calls = [];

export function template(html) {
	calls.push({ op: "template", html });
	return () => {
		throw new Error("the template was cloned, so hydration did not claim the server's node");
	};
}

export function getNextElement(cloner) {
	calls.push({ op: "getNextElement" });
	return root;
}

export function getNextMarker(start) {
	let end = start;
	let count = 0;
	const current = [];
	while (end) {
		if (end.nodeType === 8) {
			if (end.nodeValue === "$") count++;
			else if (end.nodeValue === "/") {
				if (count === 0) break;
				count--;
			}
		}
		current.push(end);
		end = end.nextSibling;
	}
	calls.push({ op: "getNextMarker", claimed: current.map(describe) });
	return [end, current];
}

export function insert(parent, accessor, marker, initial) {
	calls.push({ op: "insert", parent: describe(parent) });
	effect(() => {
		parent.rendered = String(accessor());
	});
}

export function delegateEvents(events) {
	calls.push({ op: "delegateEvents", events });
}

export const setAttribute = (el, name, value) => {
	el[`attr:${name}`] = value;
};
export const setBoolAttribute = setAttribute;
export const className = (el, value) => setAttribute(el, "class", value);
export const classList = () => {};
export const style = () => {};
export const spread = () => {};
export const assign = () => {};
export const use = () => {};
export const setProperty = (el, name, value) => {
	el[name] = value;
};
export const addEventListener = () => {};
export const dynamicProperty = () => {};
export const getNextMatch = el => el;
export const runHydrationEvents = () => {};
export const getHydrationKey = () => "";
export const hydrate = (code, el) => code();
export const render = (code, el) => code();
export const NoHydration = props => props.children;
export const innerHTML = () => {};

function describe(target) {
	if (!target) return "null";
	if (target.kind === "element") return `<${target.tag}>`;
	if (target.kind === "text") return `"${target.data}"`;
	return `<!--${target.nodeValue}-->`;
}

