// Wiring between the page and the compiled client crate.
//
// This is a classic script, loaded after shim.js and app.js, so `__rt` exists
// and every exported Rust function is already a global by its `#[no_mangle]`
// name. The exports this file expects, all defined in client/app.rs:
//
//   life_step(cur, next, w, h) -> population
//   life_seed(cells, seed, density) -> next seed
//   life_toggle(cells, w, x, y)
//   life_clear(cells)
//   life_glider(cells, w, h, x, y)
//   life_population(cells) -> population
//   fmt_demo(spec, text) -> number of pieces written
//   panic_demo(choice) -> a number, or a thrown panic

// ---------------------------------------------------------------------------
// Runtime extensions
//
// shim.js is the compiler's own file, copied verbatim. The foreign items this
// demo adds on top of it live here. `js_log_str` is already in the shim.
// ---------------------------------------------------------------------------

let emitted = "";

globalThis.__rt.js_emit = (piece) => {
  emitted += piece;
};

globalThis.__rt.js_panic = (message, file, line, column) => {
  const error = new Error(message);
  error.rustFile = file;
  error.rustLine = line;
  error.rustColumn = column;
  throw error;
};

// The `&[T]` ABI in one line: the backing array, a start offset, a length.
const slice = (array) => ({ buf: array, off: 0, len: array.length });

// ---------------------------------------------------------------------------
// Life
// ---------------------------------------------------------------------------

(() => {
  // Must match the constants the server renders the grid with.
  const W = 40;
  const H = 25;
  const COUNT = W * H;

  const grid = document.getElementById("life-grid");
  const cells = Array.from(grid.children);
  const genEl = document.getElementById("life-gen");
  const popEl = document.getElementById("life-pop");
  const playEl = document.getElementById("life-play");
  const rateEl = document.getElementById("life-rate");
  const densityEl = document.getElementById("life-density");

  // The two boards are plain JavaScript arrays. Rust writes into them through
  // the slice records below, which point at these very arrays.
  let front = new Array(COUNT).fill(false);
  let back = new Array(COUNT).fill(false);
  let frontSlice = slice(front);
  let backSlice = slice(back);

  // What the DOM currently shows, so a render only touches cells that moved.
  const drawn = new Array(COUNT).fill(null);

  let generation = 0;
  let population = 0;
  let playing = true;
  let seedState = (Date.now() >>> 0) || 1;

  const render = () => {
    for (let i = 0; i < COUNT; i += 1) {
      if (drawn[i] !== front[i]) {
        cells[i].classList.toggle("on", front[i]);
        drawn[i] = front[i];
      }
    }

    genEl.textContent = String(generation);
    popEl.textContent = String(population);
  };

  const step = () => {
    population = life_step(frontSlice, backSlice, W, H);

    const boards = [back, front];
    front = boards[0];
    back = boards[1];

    const slices = [backSlice, frontSlice];
    frontSlice = slices[0];
    backSlice = slices[1];

    generation += 1;
    render();
  };

  const randomize = () => {
    seedState = life_seed(frontSlice, seedState, Number(densityEl.value));
    generation = 0;
    population = life_population(frontSlice);
    render();
  };

  const clear = () => {
    life_clear(frontSlice);
    generation = 0;
    population = 0;
    render();
  };

  const glider = () => {
    life_glider(frontSlice, W, H, Math.floor(W / 2) - 1, Math.floor(H / 2) - 1);
    population = life_population(frontSlice);
    render();
  };

  const setPlaying = (on) => {
    playing = on;
    playEl.textContent = on ? "pause" : "play";
  };

  playEl.addEventListener("click", () => setPlaying(!playing));
  document.getElementById("life-step").addEventListener("click", () => {
    setPlaying(false);
    step();
  });
  document.getElementById("life-random").addEventListener("click", randomize);
  document.getElementById("life-glider").addEventListener("click", glider);
  document.getElementById("life-clear").addEventListener("click", () => {
    setPlaying(false);
    clear();
  });

  grid.addEventListener("click", (event) => {
    const cell = event.target.closest(".cell");
    if (!cell) {
      return;
    }

    const index = Number(cell.dataset.i);
    life_toggle(frontSlice, W, index % W, Math.floor(index / W));
    population = life_population(frontSlice);
    render();
  });

  // A timestamp accumulator, so the slider sets generations per second rather
  // than generations per frame.
  let previous = 0;
  let pending = 0;

  const frame = (timestamp) => {
    requestAnimationFrame(frame);

    if (!playing || previous === 0) {
      previous = timestamp;
      return;
    }

    pending += timestamp - previous;
    previous = timestamp;

    const interval = 1000 / Number(rateEl.value);
    let steps = 0;
    while (pending >= interval && steps < 8) {
      step();
      pending -= interval;
      steps += 1;
    }

    // A backgrounded tab leaves a large debt behind; do not try to repay it.
    if (pending > 1000) {
      pending = 0;
    }
  };

  randomize();
  setPlaying(true);
  requestAnimationFrame(frame);
})();

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

(() => {
  const kindEl = document.getElementById("fmt-kind");
  const numberEl = document.getElementById("fmt-number");
  const textEl = document.getElementById("fmt-text");
  const widthEl = document.getElementById("fmt-width");
  const precisionEl = document.getElementById("fmt-precision");
  const precisionOnEl = document.getElementById("fmt-precision-on");
  const fillEl = document.getElementById("fmt-fill");
  const alignEl = document.getElementById("fmt-align");
  const radixEl = document.getElementById("fmt-radix");
  const alternateEl = document.getElementById("fmt-alternate");
  const signEl = document.getElementById("fmt-sign");

  const outEl = document.getElementById("fmt-out");
  const piecesEl = document.getElementById("fmt-pieces");
  const callEl = document.getElementById("fmt-call");

  const FILLS = [" ", "0", "*", "."];
  const ALIGNS = ["<", "^", ">"];
  const RADIX_TYPES = ["x", "X", "b", "o"];
  // The argument each arm formats. The char and temperature arms build theirs
  // out of `spec.number`.
  const ARGUMENTS = ["number", "text", "ch", "temperature", "number"];

  const setEnabled = (element, on) => {
    element.disabled = !on;
    const field = element.closest(".field");
    if (field) {
      field.classList.toggle("off", !on);
    }
  };

  // Only the text kind reads the text box; the others all work from the
  // number. Fill and alignment are absent from the radix arms and from the
  // signed integer arm, which spells its own `{:+width$}`.
  const padded = (spec) => spec.kind !== 4 && !(spec.kind === 0 && spec.sign);

  // Which controls the chosen arm actually reads.
  const applyKind = (kind) => {
    const spec = { kind, sign: signEl.checked };

    setEnabled(numberEl, kind !== 1);
    setEnabled(textEl, kind === 1);
    setEnabled(precisionEl, kind === 1 || kind === 2 || kind === 3);
    setEnabled(precisionOnEl, kind === 1 || kind === 2 || kind === 3);
    setEnabled(fillEl, padded(spec));
    setEnabled(alignEl, padded(spec));
    setEnabled(radixEl, kind === 4);
    setEnabled(alternateEl, kind === 4);
    setEnabled(signEl, kind === 0);
  };

  // A readable approximation of the format string the chosen arm uses. The
  // Rust spells width and precision as `width$` and `.precision$`, since those
  // two are the only parts that can arrive while the program runs.
  const callLine = (spec) => {
    const width = spec.width > 0 ? String(spec.width) : "";
    let format = "";

    if (spec.kind === 4) {
      // The radix arms pad with the sign aware `0` flag, not with a fill.
      if (spec.alternate) {
        format += "#";
      }
      format += width === "" ? "" : "0" + width;
      format += RADIX_TYPES[spec.radix];
    } else if (spec.kind === 0 && spec.sign) {
      format += "+" + width;
    } else {
      if (FILLS[spec.fill] !== " ") {
        format += FILLS[spec.fill];
      }
      format += ALIGNS[spec.align] + width;

      // An integer never looks at a precision, so its arm does not pass one.
      if (spec.precision >= 0 && spec.kind !== 0) {
        format += "." + String(spec.precision);
      }
    }

    return `write!(out, "{:${format}}", ${ARGUMENTS[spec.kind]})`;
  };

  const run = () => {
    const kind = Number(kindEl.value);
    applyKind(kind);

    let number;
    try {
      number = BigInt(numberEl.value.trim() || "0");
    } catch (error) {
      outEl.textContent = "";
      piecesEl.textContent = "not an integer";
      callEl.textContent = "";
      return;
    }

    const spec = {
      kind,
      number,
      fill: Number(fillEl.value),
      align: Number(alignEl.value),
      width: Number(widthEl.value),
      precision: precisionOnEl.checked ? Number(precisionEl.value) : -1,
      radix: Number(radixEl.value),
      alternate: alternateEl.checked,
      sign: signEl.checked,
    };

    emitted = "";
    const pieces = fmt_demo(spec, textEl.value);

    outEl.textContent = emitted;
    piecesEl.textContent = `${pieces} pieces, ${[...emitted].length} chars`;
    callEl.textContent = callLine(spec);
  };

  const controls = [
    kindEl,
    numberEl,
    textEl,
    widthEl,
    precisionEl,
    precisionOnEl,
    fillEl,
    alignEl,
    radixEl,
    alternateEl,
    signEl,
  ];

  for (const control of controls) {
    control.addEventListener("input", run);
  }

  run();
})();

// ---------------------------------------------------------------------------
// Panics
// ---------------------------------------------------------------------------

(() => {
  const choiceEl = document.getElementById("panic-choice");
  const resultEl = document.getElementById("panic-result");
  const messageEl = document.getElementById("panic-message");
  const locationEl = document.getElementById("panic-location");

  document.getElementById("panic-run").addEventListener("click", () => {
    const choice = Number(choiceEl.value);

    try {
      const value = panic_demo(choice);
      resultEl.className = "ok";
      messageEl.textContent = `returned ${value}, no panic`;
      locationEl.textContent = "";
    } catch (error) {
      resultEl.className = "panicked";
      messageEl.textContent = error.message;
      locationEl.textContent = `${error.rustFile}:${error.rustLine}:${error.rustColumn}`;
      console.error(error);
    }
  });
})();
