// Size budgets for the compiled island chunks.
//
//   node smoke/budgets.mjs             # measure and check against the baselines
//   node smoke/budgets.mjs --update    # record what it measures as the baselines
//
// `check.mjs` runs the same checks, so the budgets are part of the one smoke
// command as well.
//
// # What a number means
//
// Each island is compiled into a file of its own, so a budget is a file size and
// nothing has to be attributed. Two kinds are recorded:
//
// - `chunk:<name>`, one emitted file.
// - `page:<island>`, what a page carrying only that island downloads: its own
//   chunk plus the shared one. This is the number chunking exists to move, and
//   it is the one to read when deciding whether an island got expensive.
// - `page:showcase`, the one page in the app that carries more than one island.
//   A per-island number says nothing about it, and it is where the app's JavaScript
//   is at its most expensive, so it gets a ceiling of its own.
//
// A chunk is bigger than the same island's share of one combined module, because
// it carries its own import declarations and compresses on its own. That cost is
// paid once per island and saved on every island a page does not have.

import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gzipSync } from "node:zlib";

import { ISLANDS, SHARED, chunks } from "./compiled.mjs";

const BASELINES = join(import.meta.dirname, "budgets.json");

/// The pages that carry more than one island, as the islands they carry.
///
/// Every other page in the app is one island, so `page:<island>` already is its
/// number. The showcase is three, which is the case chunking is really an answer
/// to: it pays for life, sand and mines once each and for the shared chunk once,
/// and every other page pays for none of them.
const PAGES = { showcase: ["life", "sand", "mines"] };

/// The raw and gzipped size of one piece of text.
function size(text) {
	return { raw: Buffer.byteLength(text), gzip: gzipSync(text, { level: 9 }).length };
}

/// Every chunk's size, and what each island costs a page that carries it.
export function measure() {
	const emitted = chunks();
	const measured = {};
	for (const [name, chunk] of emitted) {
		measured[`chunk:${name}`] = size(chunk.source);
	}
	const shared = emitted.get(SHARED).source;
	for (const island of ISLANDS) {
		measured[`page:${island}`] = size(emitted.get(island).source + shared);
	}
	for (const [page, carried] of Object.entries(PAGES)) {
		measured[`page:${page}`] = size(carried.map(island => emitted.get(island).source).join("") + shared);
	}
	return { path: emitted.get(ISLANDS[0]).path, measured };
}

/// The recorded baselines.
function baselines() {
	return JSON.parse(readFileSync(BASELINES, "utf8"));
}

/// Records what is measured now as the baselines.
function update() {
	const { measured } = measure();
	const recorded = baselines();
	writeFileSync(
		BASELINES,
		`${JSON.stringify({ ...recorded, measured: new Date().toISOString().slice(0, 10), sizes: measured }, null, "\t")}\n`,
	);
	for (const [name, size] of Object.entries(measured)) {
		console.log(`recorded  ${name}: ${size.raw} raw, ${size.gzip} gzip`);
	}
}

/// Runs every budget check through `check(what, actual, expected)`.
///
/// A size is checked as a ceiling with a tolerance band rather than for equality:
/// the emitter's output moves for reasons that are nobody's regression, and what
/// a budget is for is catching the growth that is.
export function checkBudgets(check) {
	const recorded = baselines();
	const { measured } = measure();
	for (const [name, baseline] of Object.entries(recorded.sizes)) {
		const actual = measured[name];
		if (!actual) {
			check(`${name} is still emitted`, false, true);
			continue;
		}
		const ceiling = Math.ceil(baseline.gzip * (1 + recorded.tolerance));
		check(
			`${name} is within its gzip budget of ${ceiling} (baseline ${baseline.gzip})`,
			actual.gzip <= ceiling,
			true,
		);
	}
	// A baseline names what was measured last time. This names what has to be
	// measurable now, so an island whose chunk stopped being emitted fails here
	// rather than passing by not being looked at.
	for (const island of ISLANDS) {
		check(`chunk:${island} was emitted`, `chunk:${island}` in measured, true);
		check(`page:${island} was measured`, `page:${island}` in measured, true);
	}
	check(`chunk:${SHARED} was emitted`, `chunk:${SHARED}` in measured, true);
	for (const page of Object.keys(PAGES)) {
		check(`page:${page} was measured`, `page:${page}` in measured, true);
	}
}

if (import.meta.filename === process.argv[1]) {
	if (process.argv.includes("--update")) {
		update();
	} else {
		const failures = [];
		checkBudgets((what, actual, expected) => {
			const ok = actual === expected;
			console.log(`${ok ? "ok  " : "FAIL"}  ${what}`);
			if (!ok) failures.push(what);
		});
		const { measured } = measure();
		for (const [name, size] of Object.entries(measured)) {
			console.log(`      ${name}: ${size.raw} raw, ${size.gzip} gzip`);
		}
		if (failures.length) {
			console.log(`\n${failures.length} over budget`);
			process.exit(1);
		}
		console.log("\nevery chunk is within its budget");
	}
}
