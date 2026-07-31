// @vitest-environment jsdom

import { expect, it } from "vitest";

import { Runtime } from "./runtime";
import { ISLAND_TAG, scan } from "./scan";

// An island hydrates itself from the hydration keys the server wrote for it.
// This runtime walks the whole document looking for its own markers, and the
// two must not both bind the same nodes, so the walk has to stop at an island
// boundary rather than merely ignore what it finds inside one.

function markup(html: string): Element {
	const root = document.createElement("div");
	root.innerHTML = html;
	return root;
}

/** A signal marker, which `scan` registers when it walks over one. */
function signal(id: string, value: unknown): string {
	const payload = JSON.stringify({ t: "signal", id, v: value });
	return `<!-- ::topcoat::signal(${payload}) -->`;
}

function run(root: Element): Runtime {
	const runtime = new Runtime();
	scan(root, null, null, runtime.rootScope);
	return runtime;
}

it("registers the signals outside an island", () => {
	const runtime = run(markup(signal("outside", 1)));

	expect(runtime.registry.has("outside")).toBe(true);
});

it("skips every marker inside an island", () => {
	const runtime = run(
		markup(
			`${signal("outside", 1)}` +
				`<${ISLAND_TAG}>${signal("inside", 2)}</${ISLAND_TAG}>`,
		),
	);

	expect(runtime.registry.has("outside")).toBe(true);
	expect(runtime.registry.has("inside")).toBe(false);
});

it("skips markers nested deep inside an island", () => {
	const runtime = run(
		markup(
			`<${ISLAND_TAG}><section><p>${signal("deep", 1)}</p></section></${ISLAND_TAG}>`,
		),
	);

	expect(runtime.registry.has("deep")).toBe(false);
});

it("leaves bindings inside an island alone", () => {
	const root = markup(
		`<${ISLAND_TAG}><input data-topcoat-bind:value="cx.signal('a')"></${ISLAND_TAG}>`,
	);
	const input = root.querySelector("input");
	expect(input).not.toBeNull();

	// A bound input would have had its value written by the effect the binding
	// sets up; an untouched one keeps the empty value it parsed with.
	run(root);

	expect((input as HTMLInputElement).value).toBe("");
});

it("resumes scanning after an island ends", () => {
	const runtime = run(
		markup(
			`<${ISLAND_TAG}>${signal("inside", 1)}</${ISLAND_TAG}>` +
				`${signal("after", 2)}`,
		),
	);

	expect(runtime.registry.has("inside")).toBe(false);
	expect(runtime.registry.has("after")).toBe(true);
});

it("scans nothing when an island is the root", () => {
	const root = markup(`<${ISLAND_TAG}>${signal("root", 1)}</${ISLAND_TAG}>`);
	const island = root.firstElementChild;
	expect(island).not.toBeNull();

	const runtime = new Runtime();
	scan(island as Element, null, null, runtime.rootScope);

	expect(runtime.registry.has("root")).toBe(false);
});
