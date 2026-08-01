// Snapshot the app into a static site.
//
// Starts the server, fetches every page and the files they reference, and lays
// them out in a directory a static host serves as-is. Built for a GitHub Pages
// project site, which serves the repository under `/<repo>/`: the app is
// compiled with `TOPCOAT_BASE_URL=/<repo>` so every URL in the pages carries
// the prefix, while the local server keeps serving unprefixed routes; this
// script bridges the two by fetching the unprefixed route and writing the file
// where the prefixed URL will look for it.
//
// Every page renders complete; the showcase page's three islands are pure
// client programs and stay fully interactive in a snapshot.
//
//   TOPCOAT_BASE_URL=/topcoat cargo build
//   TOPCOAT_BASE_URL=/topcoat node scripts/snapshot.mjs dist
//
// The base URL must match the one the binary was built with; the script checks
// the pages it saves actually carry it.

import { spawn } from "node:child_process";
import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, join } from "node:path";

const BASE = process.env.TOPCOAT_BASE_URL ?? "";
const OUT = process.argv[2] ?? "dist";
const PORT = process.env.PORT ?? "3111";
const SERVER = `http://127.0.0.1:${PORT}`;

/// Every page in the app. A new page is a new entry.
const PAGES = ["/", "/island", "/island/nested", "/island/showcase", "/bench"];

/// The stable-URL artifacts pages load directly. The chunks are not listed:
/// they come out of the served import map, so an island added to `build.rs`
/// is picked up here without a change.
const STATIC_FILES = [
	"/demo/shim.js",
	"/demo/app.js",
	"/demo/app.js.map",
	"/demo/glue.js",
	"/demo/topcoat-dom.js",
	"/demo/island-rt.js",
	"/demo/island-loader.js",
	"/demo/chunks.importmap.json",
];

/// Fetch a route from the local server and write it under the output root.
async function save(route, to) {
	const reply = await fetch(SERVER + route);
	if (!reply.ok) {
		throw new Error(`${route} answered ${reply.status}`);
	}
	const body = Buffer.from(await reply.arrayBuffer());
	const path = join(OUT, to);
	await mkdir(dirname(path), { recursive: true });
	await writeFile(path, body);
	return body;
}

/// Wait for the server to answer, then snapshot everything.
async function snapshot() {
	for (let tries = 0; ; tries++) {
		try {
			await fetch(SERVER + "/");
			break;
		} catch {
			if (tries === 120) {
				throw new Error(`no server at ${SERVER} after two minutes`);
			}
			await new Promise((settle) => setTimeout(settle, 1000));
		}
	}

	for (const page of PAGES) {
		const to = page === "/" ? "index.html" : join(page.slice(1), "index.html");
		const html = (await save(page, to)).toString();
		// A page without the prefix means the binary was built without the
		// base URL this snapshot is being taken for, and every link on the
		// deployed site would step outside it.
		if (BASE !== "" && !html.includes(`"${BASE}/demo/`)) {
			throw new Error(`${page} does not carry the base URL ${BASE}`);
		}
	}

	for (const route of STATIC_FILES) {
		await save(route, route.slice(1));
	}

	// The chunks, from the import map the pages serve: strip the base URL back
	// off to reach the local route, and take each chunk's source map with it.
	const map = JSON.parse(
		(await save("/demo/chunks.importmap.json", "demo/chunks.importmap.json")).toString(),
	);
	for (const url of Object.values(map.imports)) {
		const route = url.slice(BASE.length);
		await save(route, route.slice(1));
		try {
			await save(route + ".map", route.slice(1) + ".map");
		} catch {
			console.warn(`no source map beside ${route}`);
		}
	}

	// The content hashed assets (the stylesheet, the runtime script). Under a
	// base URL the server renders them as externally hosted and serves no
	// route, so they come from the bundle directory instead.
	if (!existsSync("target/assets")) {
		throw new Error("target/assets is missing: run `topcoat asset bundle` first");
	}
	await cp("target/assets", join(OUT, "_topcoat/assets"), {
		recursive: true,
		filter: (from) => !from.endsWith("manifest.toml"),
	});

	// The benchmark page's stylesheet. A real entry leaves it to the harness;
	// a standalone snapshot links `<base>/css/currentStyle.css` and carries
	// the copy itself.
	if (process.env.TOPCOAT_BENCH_STANDALONE) {
		const path = join(OUT, "css", "currentStyle.css");
		await mkdir(dirname(path), { recursive: true });
		await writeFile(path, await benchStyle());
	}

	// GitHub Pages runs Jekyll by default, and Jekyll drops underscore
	// prefixed paths like `/_topcoat/`. This file turns Jekyll off.
	await writeFile(join(OUT, ".nojekyll"), "");
}

/// The harness's stylesheet as one file.
///
/// `currentStyle.css` is a pair of root-absolute `@import`s, so copied
/// verbatim under a base both would point outside it; the imported sheets are
/// concatenated instead. They come from the local krausest clone when
/// `bench/setup.sh` has made one, and otherwise from the pinned commit on
/// GitHub (the Pages workflow has no clone).
async function benchStyle() {
	const parts = ["css/bootstrap/dist/css/bootstrap.min.css", "css/main.css"];
	const bench = join(import.meta.dirname, "..", "..", "bench");

	const clone = join(bench, "krausest");
	if (existsSync(clone)) {
		const sheets = await Promise.all(parts.map((part) => readFile(join(clone, part), "utf8")));
		return sheets.join("\n");
	}

	const pin = (await readFile(join(bench, "KRAUSEST_PIN"), "utf8")).trim();
	const sheets = [];
	for (const part of parts) {
		const url = `https://raw.githubusercontent.com/krausest/js-framework-benchmark/${pin}/${part}`;
		const reply = await fetch(url);
		if (!reply.ok) {
			throw new Error(`${url} answered ${reply.status}`);
		}
		sheets.push(await reply.text());
	}
	return sheets.join("\n");
}

await rm(OUT, { recursive: true, force: true });

const server = spawn("cargo", ["run"], {
	stdio: ["ignore", "inherit", "inherit"],
	env: { ...process.env, HOST: "127.0.0.1", PORT },
});

try {
	await snapshot();
	console.log(`snapshot of ${PAGES.length} pages written to ${OUT}/`);
} finally {
	server.kill();
}
