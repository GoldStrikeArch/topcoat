// Packs the benchmark island into a js-framework-benchmark entry.
//
//   node scripts/bench-pack.mjs            # build, pack and verify
//   node scripts/bench-pack.mjs --keep     # leave the base-URL build in place
//
// The result is `dist-bench/`, a directory the harness serves as
// `frameworks/non-keyed/topcoat-island`. Copy it there and `npm run bench` finds
// it; nothing else about the app has to move.
//
// # How the URLs come out right
//
// The harness serves each entry from a directory of its own and loads
// `<dir>/index.html`, so every URL the page carries has to be reachable from
// there. `src/base.rs` is what makes that possible: the app is compiled with
// `TOPCOAT_BASE_URL` set to the directory's path and every href, script src,
// import map entry and asset URL picks the prefix up, while the routes stay
// unprefixed. `scripts/snapshot.mjs` bridges the two, fetching the unprefixed
// route and writing the file where the prefixed URL will look for it.
//
// That is also why the page can be MOVED after the snapshot: `/bench` is
// snapshotted to `bench/index.html`, and the entry needs it at the root, and
// every URL inside it is absolute so neither position changes what it resolves
// to. This script asserts that rather than assuming it.
//
// The one URL that must NOT carry the prefix is the stylesheet. The harness
// serves `/css/currentStyle.css` from its own root for every entry, so the page
// writes that path out literally rather than through `base::at`.
//
// # What is left out
//
// The snapshot is of the whole app, and an entry should be the one page. The
// other pages, the script-mode program they load and every source map are
// dropped. What stays is what the benchmark page reaches: the loader, the DOM
// runtime, the host module, the compiled chunks, and the asset bundle. The
// chunks of the other islands stay too, because the page's import map names them
// and a map that resolves to a missing file is a broken map even if nothing ever
// imports it.

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { cp, mkdir, readFile, readdir, rename, rm, stat, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { extname, join, relative } from "node:path";
import { gzipSync } from "node:zlib";

/// Where the harness serves this entry from, which is also what the app is
/// compiled with. The directory name is the entry's name and `non-keyed` is its
/// category, so this string and the directory it is copied into have to agree.
const BASE = "/frameworks/non-keyed/topcoat-island";

/// The page the entry is, as the app's own route.
const PAGE = "/bench";

/// Where the packed entry is written.
const OUT = "dist-bench";

/// The app's own directory, so the script can be run from anywhere.
const APP = join(import.meta.dirname, "..");

/// The pages the snapshot takes that an entry has no use for.
const OTHER_PAGES = ["index.html", "island"];

/// The artifacts only the script-mode demo loads.
const OTHER_FILES = ["demo/app.js", "demo/shim.js", "demo/glue.js"];

/// What the entry tells the harness about itself.
///
/// `build-prod` is what the harness runs before benchmarking, and there is
/// nothing for it to do: the directory is built by this script, from Rust, with
/// a toolchain the harness does not have. The leptos entry ships the same way.
const PACKAGE = {
	name: "js-framework-benchmark-topcoat-island",
	version: "1.0.0",
	description: "Topcoat islands: a view! body compiled to JavaScript by a rustc codegen backend",
	"js-framework-benchmark": {
		frameworkVersion: "0.1.0",
		language: "Rust",
		frameworkHomeURL: "https://github.com/GoldStrikeArch/topcoat",
	},
	scripts: { "build-prod": "exit 0" },
	license: "Apache-2.0",
};

/// The lock file `npm ci` insists on. The package has no dependencies, so this
/// is the whole of it.
const LOCK = {
	name: PACKAGE.name,
	version: PACKAGE.version,
	lockfileVersion: 3,
	requires: true,
	packages: {
		"": { name: PACKAGE.name, version: PACKAGE.version, license: PACKAGE.license },
	},
};

/// Runs a command in the app directory and resolves when it succeeds.
///
/// A variable set to `null` is removed rather than emptied. Cargo fingerprints a
/// build script by the variables it declared an interest in, and `X=""` and no
/// `X` at all are two different fingerprints even though `base.rs` reads them the
/// same way, so restoring the default build means UNSETTING the base URL.
function run(command, args, env = {}) {
	const inherited = { ...process.env, ...env };
	for (const [name, value] of Object.entries(env)) {
		if (value === null) delete inherited[name];
	}

	return new Promise((settle, fail) => {
		const child = spawn(command, args, {
			cwd: APP,
			stdio: ["ignore", "inherit", "inherit"],
			env: inherited,
		});
		child.on("exit", code => (code === 0 ? settle() : fail(new Error(`${command} exited ${code}`))));
		child.on("error", fail);
	});
}

/// Every file under `dir`, as paths relative to it.
async function walk(dir, prefix = "") {
	const found = [];
	for (const entry of await readdir(join(dir, prefix), { withFileTypes: true })) {
		const path = join(prefix, entry.name);
		if (entry.isDirectory()) {
			found.push(...(await walk(dir, path)));
		} else {
			found.push(path);
		}
	}
	return found;
}

/// Takes the snapshot of the whole app under the entry's base URL.
async function snapshot() {
	await run("node", ["scripts/snapshot.mjs", OUT], { TOPCOAT_BASE_URL: BASE });
}

/// Turns the snapshot into the directory the harness expects.
async function pack() {
	const out = join(APP, OUT);

	// The app's own pages go first, because one of them is the `index.html` at
	// the root that the entry's page is about to become.
	for (const page of OTHER_PAGES) await rm(join(out, page), { recursive: true, force: true });
	for (const file of OTHER_FILES) await rm(join(out, file), { force: true });

	// The page the entry is, moved up to the root. Every URL in it is absolute
	// under the base, which is what makes the move safe; `verify` proves it.
	await rename(join(out, PAGE.slice(1), "index.html"), join(out, "index.html"));
	await rm(join(out, PAGE.slice(1)), { recursive: true, force: true });
	await rm(join(out, ".nojekyll"), { force: true });

	// Source maps are for a reader with DevTools open, and the harness weighs
	// the directory it is given.
	for (const file of await walk(out)) {
		if (file.endsWith(".map")) await rm(join(out, file));
	}

	await writeFile(join(out, "package.json"), `${JSON.stringify(PACKAGE, null, 2)}\n`);
	await writeFile(join(out, "package-lock.json"), `${JSON.stringify(LOCK, null, 2)}\n`);

	// What the page actually fetches, for bench/report.mjs's size breakdown:
	// the snapshot carries every island's chunk because the import map names
	// them all, but this page loads only its own island's closure.
	const loaded = [
		"index.html",
		"demo/island-loader.js",
		"demo/chunks/bench.js",
		"demo/chunks/shared.js",
		"demo/topcoat-dom.js",
		"demo/island-rt.js",
	];
	await writeFile(join(out, "bench-artifacts.json"), `${JSON.stringify(loaded)}\n`);
}

/// A static server over the packed directory, mounted where the harness mounts
/// it, with the harness's own stylesheet stubbed in at the server root.
///
/// Serving it anywhere else would prove nothing: the whole question is whether a
/// page full of `/frameworks/non-keyed/topcoat-island/...` URLs resolves when it
/// is loaded from that path.
function serve(out) {
	const types = {
		".html": "text/html; charset=utf-8",
		".js": "text/javascript; charset=utf-8",
		".json": "application/json; charset=utf-8",
		".css": "text/css; charset=utf-8",
	};

	const server = createServer(async (request, reply) => {
		const url = new URL(request.url, "http://127.0.0.1");
		if (url.pathname === "/css/currentStyle.css") {
			reply.writeHead(200, { "content-type": types[".css"] });
			reply.end("/* the harness serves the real one */\n");
			return;
		}
		if (!url.pathname.startsWith(`${BASE}/`)) {
			reply.writeHead(404).end("outside the entry");
			return;
		}
		const path = join(out, url.pathname.slice(BASE.length + 1));
		try {
			const body = await readFile(path);
			reply.writeHead(200, { "content-type": types[extname(path)] ?? "application/octet-stream" });
			reply.end(body);
		} catch {
			reply.writeHead(404).end("no such file");
		}
	});

	return new Promise(settle => {
		server.listen(0, "127.0.0.1", () => settle({ server, port: server.address().port }));
	});
}

/// Every URL the packed page carries.
///
/// Read out of the HTML rather than listed here, so a page that starts loading
/// something new is checked without this script being told about it.
function referenced(html) {
	const urls = new Set();
	for (const [, url] of html.matchAll(/\s(?:src|href)="([^"]+)"/g)) urls.add(url);
	const map = html.match(/<script type="importmap">(.*?)<\/script>/s);
	if (!map) throw new Error("the packed page carries no import map");
	for (const url of Object.values(JSON.parse(map[1]).imports)) urls.add(url);
	return [...urls];
}

/// Loads the packed page the way the harness will and reports what failed.
async function verify(check) {
	const out = join(APP, OUT);
	const { server, port } = await serve(out);
	const origin = `http://127.0.0.1:${port}`;

	try {
		const reply = await fetch(`${origin}${BASE}/index.html`);
		check("the entry serves a page at its root", reply.status, 200);
		const html = await reply.text();

		check("which is the benchmark's page", html.includes('class="table table-hover table-striped test-data"'), true);
		for (const id of ["run", "runlots", "add", "update", "clear", "swaprows"]) {
			check(`with the ${id} button on it`, html.includes(`id="${id}"`), true);
		}
		check("and the island the harness will drive", html.includes('data-ti="bench"'), true);
		check("hydrated without waiting to be scrolled to", html.includes('data-tl-eager="bench"'), true);

		// The stylesheet is the harness's, from the server root. A prefixed one
		// would 404 for every entry but ours and is the mistake this catches.
		check("the stylesheet is the harness's own", html.includes('<link href="/css/currentStyle.css" rel="stylesheet">'), true);
		check("and did not pick up the base URL", html.includes(`${BASE}/css/`), false);

		const urls = referenced(html);
		const outside = urls.filter(url => !url.startsWith(`${BASE}/`) && url !== "/css/currentStyle.css");
		check("every other URL on the page is under the entry", outside.join(" "), "");

		for (const url of urls) {
			const found = await fetch(origin + url);
			check(`  ${url} is served`, found.status, 200);
		}

		// The chunk the loader will import for this island, resolved the way the
		// browser resolves it: through the page's own map.
		const map = JSON.parse(html.match(/<script type="importmap">(.*?)<\/script>/s)[1]);
		const chunk = map.imports["topcoat-island/bench"];
		check("the island's chunk is in the map", typeof chunk, "string");
		const source = await (await fetch(origin + chunk)).text();
		check("and is the compiled island", source.includes("export { __island_bench }"), true);
		check("importing the runtime by a specifier the map resolves", source.includes('from "topcoat-dom"'), true);
		check("and the host module by one too", source.includes('from "topcoat-island-rt"'), true);
	} finally {
		server.close();
	}
}

/// What one page load actually fetches.
///
/// NOT everything the page names: the import map names a chunk per island in the
/// app and this page has one island, so a count over the map would be four times
/// the truth. The browser fetches the page, the two scripts in its head, whatever
/// those import, and the island's own chunk, which the loader imports by name
/// once it has found the island in the document. That last step is the only one
/// this cannot read off the file, so it is written down: `data-tl-eager` says
/// which island, and the map says which file.
async function loaded(out, html) {
	const map = JSON.parse(html.match(/<script type="importmap">(.*?)<\/script>/s)[1]).imports;
	const island = html.match(/data-tl-eager="([^"]+)"/)[1];

	/// Where a specifier written inside `from` resolves to.
	const resolve = (specifier, from) => {
		if (map[specifier]) return map[specifier];
		if (specifier.startsWith("/")) return specifier;
		if (specifier.startsWith(".")) return join(from.slice(0, from.lastIndexOf("/")), specifier);
		throw new Error(`nothing resolves the specifier ${specifier}`);
	};

	const queue = [
		...[...html.matchAll(/<script[^>]+src="([^"]+)"/g)].map(([, url]) => url),
		resolve(`topcoat-island/${island}`, ""),
	];
	const found = ["index.html"];
	while (queue.length) {
		const url = queue.shift();
		const file = url.slice(BASE.length + 1);
		if (found.includes(file)) continue;
		found.push(file);
		const source = await readFile(join(out, file), "utf8");
		for (const [, specifier] of source.matchAll(/\bfrom\s*"([^"]+)"/g)) {
			queue.push(resolve(specifier, url));
		}
	}
	return found;
}

/// Weighs that, which is the number the harness's own brotli counter is about.
///
/// Gzip at level 9 rather than brotli, because that is what `smoke/budgets.mjs`
/// records and a number worth comparing is one measured the same way twice. The
/// harness's figure will be smaller.
async function weigh() {
	const out = join(APP, OUT);
	const html = await readFile(join(out, "index.html"), "utf8");

	let raw = 0;
	let gzip = 0;
	for (const file of await loaded(out, html)) {
		const body = await readFile(join(out, file));
		const packed = gzipSync(body, { level: 9 }).length;
		raw += body.length;
		gzip += packed;
		console.log(`      ${file}: ${body.length} raw, ${packed} gzip`);
	}

	console.log(`\n      one page load: ${raw} raw, ${gzip} gzip`);
}

const failures = [];
const check = (what, actual, expected) => {
	const ok = actual === expected;
	console.log(`${ok ? "ok  " : "FAIL"}  ${what}: ${JSON.stringify(actual)}`);
	if (!ok) failures.push(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
};

if (!existsSync(join(APP, "target", "assets"))) {
	throw new Error("target/assets is missing: run `topcoat asset bundle` first");
}

await rm(join(APP, OUT), { recursive: true, force: true });

try {
	await snapshot();
	await pack();
	console.log("");
	await verify(check);
	console.log("");
	await weigh();
} finally {
	// The tree this lane works in is the default build, and a base-URL binary
	// left behind would serve prefixed URLs to every other check. Put it back
	// unless the caller wants to look at what was built.
	if (!process.argv.includes("--keep")) {
		console.log("\nrestoring the default build");
		await run("cargo", ["build"], { TOPCOAT_BASE_URL: null });
	}
}

console.log("");
if (failures.length) {
	for (const failure of failures) console.log(failure);
	console.log(`${failures.length} failed`);
	process.exit(1);
}

const packed = await stat(join(APP, OUT));
console.log(`${relative(APP, join(APP, OUT))}/ packed${packed.isDirectory() ? "" : "?"} and verified`);
