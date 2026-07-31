// A document the loader can be run against, and nothing more.
//
// There is no DOM implementation in the tree, and the loader needs very little
// of one: it finds its options on a script tag, finds the islands, reads their
// data attributes, and asks whether the nodes the server wrote are still where
// it left them. That is what this implements. A real DOM is what the parity
// harness runs the same loader against; what this suite answers is which islands
// the loader hydrates and when, which needs no layout, no parser and no events.

/// One element: a tag, its attributes, and its children.
export class Element {
	constructor(tag, attributes = {}) {
		this.tag = tag;
		this.parent = null;
		this.children = [];
		this.attributes = new Set(Object.keys(attributes));
		this.dataset = {};
		for (const [name, value] of Object.entries(attributes)) {
			if (!name.startsWith("data-")) continue;
			this.dataset[camel(name.slice("data-".length))] = value;
		}
	}

	/// Adds `child` and answers with it, so a tree reads as a tree.
	append(child) {
		child.parent = this;
		this.children.push(child);
		return child;
	}

	/// Every element below this one, in document order.
	get descendants() {
		return this.children.flatMap(child => [child, ...child.descendants]);
	}

	/// Whether `node` is this element or below it.
	contains(node) {
		for (let at = node; at; at = at.parent) {
			if (at === this) return true;
		}
		return false;
	}

	hasAttribute(name) {
		return this.attributes.has(name);
	}

	/// The selectors the loader uses, and only those.
	querySelectorAll(selector) {
		const all = this.descendants;
		switch (selector) {
			case "[data-hk]":
				return all.filter(element => "hk" in element.dataset);
			case "topcoat-island[data-ti]":
				return all.filter(element => element.tag === "topcoat-island" && "ti" in element.dataset);
			case "script[data-tl-loader]":
				return all.filter(element => element.hasAttribute("data-tl-loader"));
			default:
				throw new Error(`the page stub does not implement the selector ${JSON.stringify(selector)}`);
		}
	}

	querySelector(selector) {
		return this.querySelectorAll(selector)[0] ?? null;
	}
}

/// `data-tl-eager` reads back as `dataset.tlEager`, the way a browser spells it.
function camel(name) {
	return name.replace(/-([a-z])/g, (_, letter) => letter.toUpperCase());
}

/// An `IntersectionObserver` whose scrolling is a function call.
///
/// The loader is what decides when an island is hydrated; this is what decides
/// when the loader is told an island is visible, so a check can look at the page
/// in between.
export class Viewport {
	constructor() {
		this.observed = new Set();
		this.callback = null;
		const viewport = this;
		this.IntersectionObserver = class {
			constructor(callback) {
				viewport.callback = callback;
			}

			observe(target) {
				viewport.observed.add(target);
			}

			unobserve(target) {
				viewport.observed.delete(target);
			}
		};
	}

	/// Tells the loader that `targets` are now on screen.
	show(...targets) {
		this.callback(targets.map(target => ({ target, isIntersecting: true })));
	}
}

/// A page with a loader script tag and one mount per island.
///
/// `islands` are `{ name, key, seeds, keys }`: the island's name, the prefix its
/// hydration keys start with, the seeds the server serialized, and the keys the
/// server wrote inside it.
export function page({ eager = "", dev = false, islands = [] }) {
	const document = new Element("#document");
	const attributes = { "data-tl-loader": "" };
	if (eager) attributes["data-tl-eager"] = eager;
	if (dev) attributes["data-tl-dev"] = "";
	document.append(new Element("script", attributes));

	const mounts = new Map();
	for (const { name, key, seeds = [], keys = [], tl } of islands) {
		const island = {
			"data-ti": name,
			"data-tk": key,
			"data-ts": JSON.stringify(seeds),
		};
		if (tl) island["data-tl"] = tl;
		const mount = document.append(new Element("topcoat-island", island));
		for (const hk of keys) mount.append(new Element("div", { "data-hk": hk }));
		mounts.set(name, mount);
	}
	return { document, mounts };
}
