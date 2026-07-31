// Finds the island chunks cargo published into OUT_DIR.
//
// Its own file because three suites want them: the behaviour checks in
// `check.mjs`, the size budgets in `budgets.mjs`, and the loader checks in
// `loader.mjs`.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

/// The islands the build asks for a chunk of, plus the chunk holding what more
/// than one of them reaches.
export const ISLANDS = ["counter", "nested", "search", "dashboard", "life", "sand", "mines", "bench"];
export const SHARED = "shared";

/// The directory the build published the chunks into.
///
/// One per island plus the shared one, siblings in a directory of their own, so
/// a chunk's `./<name>.js` import resolves without any help.
///
/// Cargo gives a build script a new `OUT_DIR` whenever the script's fingerprint
/// changes and leaves the old one where it is, so `target/debug/build` can hold
/// SEVERAL directories of chunks and only one of them is the build that just
/// ran. Taking the first by name reads whichever sorts lowest, which is how a
/// stale directory once got measured and stamped as the size baseline. The
/// newest write wins instead.
export function chunkDir() {
	const builds = join(import.meta.dirname, "..", "target", "debug", "build");
	let newest;
	for (const entry of readdirSync(builds)) {
		const candidate = join(builds, entry, "out", "chunks");
		let written;
		try {
			written = Math.max(
				...readdirSync(candidate)
					.filter(file => file.endsWith(".js"))
					.map(file => statSync(join(candidate, file)).mtimeMs)
			);
		} catch {
			continue;
		}
		if (Number.isFinite(written) && (newest === undefined || written > newest.written)) {
			newest = { candidate, written };
		}
	}
	if (newest === undefined) {
		throw new Error("no chunks under target/debug/build/*/out: run `cargo build` first");
	}
	return newest.candidate;
}

/// One compiled chunk: where it is and what is in it.
export function compiled(name) {
	const path = join(chunkDir(), `${name}.js`);
	return { path, source: readFileSync(path, "utf8") };
}

/// Every chunk the build published, by name.
export function chunks() {
	return new Map([...ISLANDS, SHARED].map(name => [name, compiled(name)]));
}
