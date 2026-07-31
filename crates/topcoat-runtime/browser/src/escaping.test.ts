// @vitest-environment jsdom

import { expect, it } from "vitest";

// Topcoat escapes `>` in text, solid does not. The concern is that a server
// rendered `&gt;` and a client rendered `>` would disagree during hydration and
// leave the page in a broken state.
//
// They cannot, for two independent reasons.
//
// 1. Escaping is a property of the bytes, not of the tree. The HTML parser
//    resolves `&gt;` back to `>` while building the document, so the server's
//    output and solid's output parse to the same text node with the same data
//    and the same siblings. Everything downstream of the parser, which is
//    everything the client runtime sees, is identical. That is what the tests
//    below demonstrate.
//
// 2. Hydration adopts nodes instead of rewriting them. In
//    `dom-expressions/src/client.js`, `insertExpression` opens the string case
//    with `if (hydrating) return current;`, so a text position is never written
//    during the hydration pass; the server's node is kept as it is. Nothing
//    compares serialized text at any point.
//
// After hydration, a signal update assigns `node.data = value` with a plain
// JavaScript string. No escaper runs on the client, so the character written is
// `>`, which is exactly the character the parser produced from `&gt;`.
//
// Verdict: Topcoat's wider text escaping is safe, and `escape.rs` needs no
// change. It costs three bytes per `>` in the served markup and buys nothing on
// hydration, but it is never wrong.

function parse(html: string): ChildNode[] {
	const host = document.createElement("div");
	host.innerHTML = html;
	return Array.from(host.childNodes);
}

it("an escaped and a bare angle bracket parse to the same text node", () => {
	const escaped = parse("a&gt;b");
	const bare = parse("a>b");

	expect(escaped).toHaveLength(1);
	expect(bare).toHaveLength(1);
	expect(escaped[0]?.nodeType).toBe(Node.TEXT_NODE);
	expect(bare[0]?.nodeType).toBe(Node.TEXT_NODE);
	expect((escaped[0] as Text).data).toBe("a>b");
	expect((bare[0] as Text).data).toBe((escaped[0] as Text).data);
});

it("an entity does not split the text into more nodes", () => {
	// A sibling walk counts nodes, so an entity that split its text in two
	// would move every following `nextSibling` step off by one.
	expect(parse("a&gt;b&gt;c")).toHaveLength(1);
	expect(parse("a>b>c")).toHaveLength(1);
});

it("escaped text keeps the siblings around it in the same positions", () => {
	const escaped = parse("<i></i>a&gt;b<!--m--><b></b>");
	const bare = parse("<i></i>a>b<!--m--><b></b>");

	expect(escaped.map((node) => node.nodeType)).toEqual(
		bare.map((node) => node.nodeType),
	);
	expect(escaped).toHaveLength(4);
});

it("writing the character back is what the parser already produced", () => {
	// This is what a post-hydration signal update does: a plain assignment of
	// an unescaped JavaScript string.
	const [node] = parse("a&gt;b");
	const text = node as Text;
	const before = text.data;

	text.data = "a>b";

	expect(text.data).toBe(before);
});
