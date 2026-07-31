// Puts the compiled chunks somewhere node can import them.
//
// A chunk imports two kinds of specifier: the bare `topcoat-dom`, which the
// page's import map resolves and node does not, and `./<name>.js` for the shared
// chunk beside it, which node resolves the same way a browser would. So staging
// is a copy of the whole directory with the bare specifier rewritten, which
// leaves the relative one working because the copies are siblings too.

import { copyFileSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

import { chunkDir } from "./compiled.mjs";

/// Copies every chunk into a temporary directory, resolving each bare specifier
/// in `imports` to the URL it names.
///
/// Returns the staged path of each chunk, by name.
export function stage(imports) {
	const from = chunkDir();
	const to = mkdtempSync(join(tmpdir(), "topcoat-island-smoke-"));
	const staged = new Map();
	for (const file of readdirSync(from)) {
		const target = join(to, file);
		if (!file.endsWith(".js")) {
			copyFileSync(join(from, file), target);
			continue;
		}
		let source = readFileSync(join(from, file), "utf8");
		for (const [specifier, url] of Object.entries(imports)) {
			source = source.replaceAll(JSON.stringify(specifier), JSON.stringify(url));
		}
		writeFileSync(target, source);
		staged.set(basename(file, ".js"), target);
	}
	return staged;
}
