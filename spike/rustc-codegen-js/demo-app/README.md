# demo-app

First contact between the `rustc_codegen_js` backend and Topcoat. A Topcoat
server renders the page; the interactive parts are a `#![no_std]` Rust crate in
`client/`, compiled to plain JavaScript at build time and loaded by the browser
as an ordinary script.

Three panels at `/`:

- **Life**, over boards that stay JavaScript arrays, passed to Rust as
  `&[bool]` and `&mut [bool]`.
- **Formatting**, calling compiled `core::fmt` on every keystroke.
- **Panics**, showing a Rust `file:line:column` read from `PanicInfo` inside the
  compiled crate's `#[panic_handler]`.

And three island pages, which are the same idea turned on the DOM: one `view!`
body is rendered to HTML by the server and compiled to a dom-expressions
template by the backend, and the browser takes over the markup that is already
there.

- `/island`, a counter (`island/counter.rs`). Hydrated eagerly.
- `/island/nested`, the same through two component boundaries
  (`island/nested.rs`).
- `/island/showcase`, three islands on one page that are *programs* rather than
  bindings. See [The showcase](#the-showcase).

Each island is compiled into a chunk of its own, plus a shared chunk for what
more than one of them reaches, and the page's import map names them. The loader
fetches an island's chunk when the island is about to be seen, so an island
below the fold costs nothing until it is scrolled to; `dom::script(eager: ..)`
names the islands that should not wait.

## The showcase

`/island/showcase` is the page the compiler is for. The other island pages each
make one point about the machinery; this one asks it for whole programs, and all
three demos on it were impossible before it. The expression language that came
before had no `match`, no `for`, no structs and no recursion, so anything with a
rule in it was hand written JavaScript beside the Rust — `client/life.rs` and
its 351 line `glue.js`, still on `/`, are what that looked like.

Three islands on one page, which no other page here has. They hydrate one at a
time, in the order the loader finds them, and only Life is eager:
`dom::script(events: "click input contextmenu", eager: "life")`. The other two
chunks are not fetched until they are scrolled to.

**Life** (`island/life.rs`), a 32×20 board. Conway's rule is a `match` over a
pair, the neighbour count wraps at the edges, and the step is double buffered:
read `LIFE_CUR`, write `LIFE_NEXT`, exchange. Click a cell to toggle it, and the
controls play, step, reset, clear, scatter and set the rate. Two subscriptions
keep 640 cells affordable — the driver hole reads the clock and renders one
number, so running it 20 times a second is free, while the grid's loop reads the
generation and rebuilds only when the board really moved.

**Falling sand** (`island/sand.rs`), a 60×40 grid on a 360×240 canvas. The
first thing in this repository that paints: every pixel is a `fillRect` the
compiled Rust asked for, through eight `#[js_extern]` declarations and no
drawing loop. A grain falls straight down if it can and diagonally otherwise,
and which side it prefers is a hash of its own coordinates and the frame number
rather than an RNG with state in it. A frame paints the cells that *changed* and
no others, so a full redraw would be 2400 calls and a settled pile is zero.
Turning a `clientX` into a cell is compiled Rust too, subtracting the canvas's
own rectangle.

**Minesweeper** (`island/mines.rs`), 12×10 with 15 mines. The first click is
always safe because nothing is placed until it happens: the mines are dealt from
the seed the server rendered into `data-ts`, skipping the clicked cell and its
eight neighbours. Opening a blank cell runs a recursive flood fill, and a right
click flags rather than opening because the handler calls `prevent_default()`.
One board of 121 bytes holds the field as bitflags; adjacent counts are not
stored, because they are a function of the mines and a stored copy is a second
answer that can disagree with the first.

**The source viewer** is the fourth panel and has no client code at all: a
collapsed `<details>` with `island/life.rs` in one `<pre>` and the chunk the
backend compiled it to in the other, both read at build time with `include_str!`
and escaped by the view's own default. It is a server rendered page reading its
own build output.

### What the host module lends them

An island's crate is `#![no_std]` with no heap and no mutable statics, and a
`#[js_extern]` declaration cannot express a callback. Those two limits are the
whole reason `src/island-rt.mjs` exists: it holds what has to survive between
calls, and it makes the functions that have to be handed to the browser. A
compiled island calls an `extern "C"` function as `__rt.<name>(..)`, and the
import map resolves `topcoat-island-rt` to that file.

Seven exports are shared by the three demos and specific to none of them:

| export | what it is for |
| --- | --- |
| `claim(id)` | A once-guard per island, since the client crate cannot hold a `bool` between calls. Setup runs on the first effect and has to know it is the first. |
| `board(id, len)` | A `{ buf, off, len }` record over a persistent `Array(len).fill(0)`, consumed as `&'static mut [u8]`. The array is the host's; the backend indexes it as `record.buf[record.off + i]`, so nothing is copied in either direction. |
| `board_ro(id, len)` | The same array borrowed to read. A second name because an `extern` name is a JavaScript name, and one declaration is `&mut` and the other is not. |
| `board_swap(a, b)` | Exchanges two boards' `buf`. The record does not move, so a slice taken before the exchange is over the new cells afterwards — which is what makes double buffering a swap rather than a copy. |
| `clock_start(sig, ms)` | `setInterval` writing a signal. A `Sig<T>` crosses as solid's raw `[read, write]` pair, so JavaScript can write it; a Rust closure could not be handed over. |
| `pointer_sink(x, y, down)` | Returns one closure that writes `clientX`, `clientY` and `buttons` into three signals. Pointer coordinates have no other way across. |
| `rt_reset()` | Clears the claims, the boards and the intervals, so `smoke/check.mjs` can run an island twice and then exit. |

Board ids are `LIFE_CUR=1`, `LIFE_NEXT=2`, `SAND_CUR=3`, `SAND_NEXT=4`,
`MINES=5`, and an island claims under its own first board id.

### What these three cost the pages that do not have them

The shared chunk went from 227 gzipped bytes to 1804 when they landed, because
all three reach `str::as_bytes` and the panic and slice helpers under it, and
what more than one island reaches is hoisted. Chunking splits an island's own
code out, not the runtime underneath it, so `/island` now downloads 2465 gzipped
bytes where it used to download 919 — the counter's own chunk is unchanged and
the floor beneath it is not. `smoke/budgets.json` records the new numbers. This
is the one number worth watching: the per-island chunks are doing their job, and
the shared floor is where a fourth demo would be felt by everybody.

The showcase itself pays for all three: `page:showcase` is 15016 gzipped bytes,
the three chunks plus the shared one, against 2465 for `/island` next door. It
has a budget of its own because it is the only page carrying more than one
island, so no per-island number covers it.

### Two constraints these islands are shaped by

**A `$( .. )` value has no free function calls.** A reactive hole and a handler
are compiled by the server's expression language as well as by the client
compiler, and that language lowers `f(a)` to `f.call((a,))`, which a Rust `fn`
item is not. Method calls do lower. So a handler reads its event and writes a
signal and nothing else, and the work happens where plain Rust is still plain
Rust: a `for` loop's iterable, a `signal` initialiser, or a method on the signal
being bumped. `island/life.rs`'s `Control` trait and `island/mines.rs`'s `cells`
and `notes` are the two shapes that takes.

**A grid uses one handler, not one per cell.** A `@click` inside a `for` row
works -- the thunk snapshots the row's environment -- but it is one closure per
cell rebuilt on every render. All three grids use one handler on the container
and a `value=(index)` on each cell instead, read back with `e.target_value()`,
which is the delegated event's original target.
`examples/dom-tests/23_for_row_handlers.rs` pins both shapes.

## One time setup

The backend and the `core` it compiles against are built once per checkout and
take minutes:

```sh
cd spike/rustc-codegen-js
scripts/build.sh
JS_EXTRA_ARGS='' scripts/build_sysroot.sh
```

The island's view goes through a proc macro, which runs inside rustc and so is
built for the host. It is quick, but it has to be rebuilt whenever the emitter
changes, because `build.rs` loads the dylib rather than the source:

```sh
cd spike/rustc-codegen-js
cargo build --release -p view-dom-macro
```

## Running it

Build the Topcoat CLI once, from the repository root:

```sh
cargo build -p topcoat-cli --bin topcoat
```

Then run the dev server, which bundles the assets and reloads the page on a
change:

```sh
cd spike/rustc-codegen-js/demo-app
../../../target/debug/topcoat dev
```

Without the CLI's dev server, bundle the assets once and use cargo directly:

```sh
topcoat asset bundle
cargo run
```

## Checking the island

Two checks run without a browser, both after `cargo build`:

```sh
node smoke/check.mjs             # the compiled chunks and the loader
cargo run & node smoke/runtime.mjs   # the served runtime, against what a chunk imports
```

`smoke/check.mjs` drives the compiled island over a node graph shaped like the
HTML the server sends and asserts the loop the compiled Rust owns: the template
matches the server's markup, the server's node is claimed rather than cloned, the
signal is seeded from the argument the loader passes, and clicking either button
moves both the rendered count and the bound `disabled` property.
`smoke/loader.mjs`, which `check.mjs` also runs, reads the loader out of
`src/dom.rs` and runs it against a stub document: it pins that an eager island is
hydrated at once, that a lazy island's chunk is not fetched until it is seen,
that each island hydrates against its own key prefix, and that a hydration lost
to a key the server never wrote is reported in a dev build and nowhere else.
`smoke/life.mjs`, `smoke/sand.mjs` and `smoke/mines.mjs` do the same for the
showcase's three, each against the real chunk and the real `src/island-rt.mjs`:
Life's first render is asserted against the same opening board `island/life.rs`'s
own tests assert on the server, so a change that reached one half fails the
other; a container click toggles the cell whose `value` it was given, and driving
the clock moves a glider one cell diagonally. Sand runs against a recording
canvas, which is what makes "a frame paints only the cells whose value changed"
a measurement rather than a claim. Mines recomputes the flood fill in JavaScript
and compares, and covers first-click safety, flagging, a loss and a win.
`smoke/budgets.mjs` checks each chunk against its recorded size.
`smoke/runtime.mjs` reads the names a compiled chunk imports out of the chunk
itself, checks the served runtime against them, and checks that a signal it hands
out drives an effect it hands out: one artifact, one reactive graph. Because that
runtime is one self-contained file, linking it under node is the same as the
browser's import map resolving it.

What neither can check is the part that needs a real DOM: there is no DOM
implementation in the tree, so the registry that maps a `data-hk` to a node is
never exercised. That is the browser checklist:

1. Open `/island`. The count reads 5 before any JavaScript runs; view source and
   the counter is in the HTML, the wrapper carries `data-ti`, `data-tk`, and
   `data-ts="[5.0]"`, and the outer `<div>` carries `data-hk="i0.0"`.
2. Click `+1` and `-1`. The count follows, and `-1` disables itself at zero.
   Nothing is fetched: the whole loop is the compiled module.
3. With the console open, confirm nothing was logged. A missing entry point or a
   hydration mismatch both report there.
4. In Elements, expand the island and turn on **Break on > subtree
   modifications** before reloading. Expect none: the hole walk now claims the
   server's text node alone, so the first insert writes into that node rather
   than building a new one, and the `<!--$-->`/`<!--/-->` pair stays where the
   server put it. `smoke/check.mjs` pins the claim this rests on. Anything that
   does break here is a regression worth reporting, not the design.
5. Open `/island/showcase` with the console and the Network panel open. Nothing
   is logged: a hydration that rebuilt the server's markup instead of adopting it
   warns there under `topcoat dev`, and three islands on one page is the first
   time that could happen to more than one at a time. `life.js` is fetched at
   once; `sand.js` and `mines.js` arrive only as their panels are scrolled to.
6. Life is already running. Click a cell mid-run and it toggles without losing a
   generation; drag the speed slider and the rate follows; `play / pause`,
   `step`, `reset`, `clear` and `random` each do what they say.
7. Draw on the sand with the pointer held down. Grains pile and settle rather
   than falling for ever, `wall` draws something the sand rests on, and `erase`
   takes it away. Release the pointer outside the canvas, then click a brush: it
   must not stamp a blob where the pointer was.
8. On the minefield, click anywhere first: it never blows up, and it usually
    opens a region rather than a single cell. Right click flags a cell and no
    context menu appears. Flag every mine, or open every safe cell, and the
    readout says so; click a mine and it does not. `restart` deals a new field
    from the same seed.
    One known edge, worth knowing rather than hunting: a right click landing in
    the window between the panel scrolling into view and `mines.js` arriving is
    captured by the pre-hydration bootstrap and replayed afterwards, so the cell
    is flagged — but the bootstrap only records the event, it does not call
    `preventDefault`, so the browser's own context menu appears for that one
    click. It is the standard hydration capture script, verbatim, and nothing in
    the island can reach that click before the island exists.
9. Expand the source viewer. The Rust pane is `island/life.rs` as written and
    the JavaScript pane is the chunk it compiled to, both readable, neither
    escaped into markup. The page runs no code for either.
10. Open `/` and check the three panels still work, and that their own runtime is
    untouched: view source there still shows `::topcoat::signal(` and
    `data-topcoat-on:click`, neither of which appears inside an island.

## Layout notes

**Its own workspace.** The `Cargo.toml` starts with an empty `[workspace]`
table. That keeps Topcoat's dependency graph out of the spike's `Cargo.lock`,
and keeps a bare `cargo build --release` in the spike from recursing into this
build script and its minutes long compile.

**The JavaScript routes bypass `asset!`.** `/demo/app.js`, `/demo/app.js.map`,
`/demo/shim.js`, `/demo/glue.js`, `/demo/island-rt.js` and
everything under `/demo/chunks/` are plain routes. The compiled program
finds its map through a relative `//# sourceMappingURL=app.js.map` comment, and
a content hashed asset name would break that link. They are served with
`Cache-Control: no-store`, since the URLs are stable while the files change on
every build. `demo.css` has no such constraint and goes through the real asset
pipeline.

**`client/` is not a cargo target.** The backend compiles it from a single root
file during `build.rs`, so cargo never sees it: `cargo check`, `cargo clippy`,
and `cargo fmt` all skip it. Its `#[no_mangle]` functions are the exports, and
their names appear in `/demo/app.js` exactly as written.

## Static deployment

The whole app snapshots into a static site: the islands are compiled at build
time and every page is server rendered, so a crawl of a locally running server
is the complete site. The showcase page's islands stay fully interactive.

A host that serves the site under a subpath (a GitHub Pages project site lives
at `/<repo>/`) needs every URL prefixed, which happens at the source rather
than by rewriting output: `TOPCOAT_BASE_URL` is read at compile time by
`src/base.rs` and `build.rs`, and an empty value (the default) renders pages
byte for byte as before.

```sh
TOPCOAT_BASE_URL=/topcoat cargo build
../../../target/debug/topcoat asset bundle
TOPCOAT_BASE_URL=/topcoat node scripts/snapshot.mjs dist
```

`.github/workflows/deploy-demos.yml` runs exactly this on every push to the
spike branch and deploys `dist/` to GitHub Pages. One time setup on a fork:
Settings -> Pages -> Source -> GitHub Actions. No secrets are involved.
