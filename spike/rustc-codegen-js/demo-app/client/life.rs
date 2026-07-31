//! Conway's Game of Life, as pure functions over a board the page owns.
//!
//! Nothing in this module holds state. The board is a JavaScript array of booleans that the page
//! allocates, keeps and draws; a call hands it over as a `&[bool]` to read or a `&mut [bool]` to
//! write, and writes through the `&mut` land in that same array with nothing copied either way.
//! Reading the emitted JavaScript next to this file, a slice is the object `{buf, off, len}`, so
//! `cells[i]` is `cells.buf[cells.off + i]`.
//!
//! Splitting the work this way is what keeps the two languages honest: JavaScript decides when a
//! generation happens and what it looks like, Rust decides what the next generation *is*.

/// The eight cells that surround a cell, as offsets from it.
///
/// A table rather than a pair of nested loops, because the centre cell is not its own neighbour
/// and a table says so by leaving `(0, 0)` out.
const NEIGHBOURS: [(i32, i32); 8] =
    [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)];

/// The five living cells of a glider, as offsets from the cell it is stamped at.
const GLIDER: [(i32, i32); 5] = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];

/// The state `life_seed` falls back to, because a seed of zero would leave `xorshift` producing
/// zero forever.
const DEFAULT_SEED: u32 = 0x2545_f491;

/// How many of the eight neighbours of `(x, y)` are alive.
///
/// The board wraps: a coordinate that runs off one edge comes back on the opposite one. That is
/// what `rem_euclid` is for, and why it is not `%`. The two differ exactly on negative numbers,
/// which is exactly the case that arises here: `-1 % w` is `-1`, but `(-1).rem_euclid(w)` is
/// `w - 1`, the far edge.
///
/// Exported only so that the emitted JavaScript carries this name and the page can call it from
/// a console to check a single cell. The step function calls it directly.
#[unsafe(no_mangle)]
pub fn living_neighbours(cells: &[bool], w: i32, h: i32, x: i32, y: i32) -> i32 {
    if w <= 0 || h <= 0 {
        return 0;
    }

    let mut alive = 0;
    for &(dx, dy) in NEIGHBOURS.iter() {
        let nx = (x + dx).rem_euclid(w);
        let ny = (y + dy).rem_euclid(h);
        if let Some(&true) = cells.get((ny * w + nx) as usize) {
            alive += 1;
        }
    }
    alive
}

/// Writes the generation after `cur` into `next` and returns how many cells it left alive.
///
/// Two boards rather than one: a cell's fate depends on its neighbours *before* the step, so
/// writing into the board being read would let the first half of a generation change the second
/// half. The page keeps both and swaps them.
///
/// The rule itself is one `match` over a pair, which is the whole of Life: a living cell with two
/// or three living neighbours stays alive, a dead cell with exactly three is born, and every
/// other case is dead.
#[unsafe(no_mangle)]
pub fn life_step(cur: &[bool], next: &mut [bool], w: i32, h: i32) -> i32 {
    if w <= 0 || h <= 0 {
        return 0;
    }

    let mut population = 0;
    for y in 0..h {
        for x in 0..w {
            let index = (y * w + x) as usize;
            let alive = match (cur[index], living_neighbours(cur, w, h, x, y)) {
                (true, 2) | (true, 3) => true,
                (false, 3) => true,
                _ => false,
            };

            next[index] = alive;
            if alive {
                population += 1;
            }
        }
    }
    population
}

/// One step of a 32 bit xorshift generator.
///
/// The caller carries the state, so this is a pure function of its input and the page can replay
/// any board it has seen by remembering the seed alone. Feeding it zero returns zero, which is
/// why `life_seed` refuses a zero seed.
///
/// Exported only so that the emitted JavaScript carries this name and the sequence can be
/// stepped by hand from a console.
#[unsafe(no_mangle)]
pub fn xorshift(state: u32) -> u32 {
    let mut x = state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Fills `cells` at random and returns the generator state to start the next fill from.
///
/// `density` is a percentage, clamped to `0..=100`: at 0 every cell is dead, at 100 every cell is
/// alive. A `seed` of zero means "pick one", so that a page which has no seed yet can pass the
/// number it has.
#[unsafe(no_mangle)]
pub fn life_seed(cells: &mut [bool], seed: u32, density: i32) -> u32 {
    let mut state = if seed == 0 { DEFAULT_SEED } else { seed };
    let threshold = density.clamp(0, 100) as u32;

    for cell in cells.iter_mut() {
        state = xorshift(state);
        *cell = state % 100 < threshold;
    }
    state
}

/// Flips the cell at `(x, y)`, and does nothing at all if that is not a cell on the board.
///
/// Out of range is ignored rather than a panic: this is what a click on the canvas calls, and a
/// click landing a pixel outside the grid is a normal thing for a page to do.
#[unsafe(no_mangle)]
pub fn life_toggle(cells: &mut [bool], w: i32, x: i32, y: i32) {
    if w <= 0 || x < 0 || x >= w || y < 0 {
        return;
    }

    if let Some(cell) = cells.get_mut((y * w + x) as usize) {
        *cell = !*cell;
    }
}

/// Kills every cell.
#[unsafe(no_mangle)]
pub fn life_clear(cells: &mut [bool]) {
    for cell in cells.iter_mut() {
        *cell = false;
    }
}

/// Stamps a glider with its top left corner at `(x, y)`, wrapping at the edges as `life_step`
/// does.
///
/// The cells it covers are set alive and the ones around them are left as they were, so stamping
/// onto a busy board adds a glider rather than clearing a space for one.
#[unsafe(no_mangle)]
pub fn life_glider(cells: &mut [bool], w: i32, h: i32, x: i32, y: i32) {
    if w <= 0 || h <= 0 {
        return;
    }

    for &(dx, dy) in GLIDER.iter() {
        let cx = (x + dx).rem_euclid(w);
        let cy = (y + dy).rem_euclid(h);
        if let Some(cell) = cells.get_mut((cy * w + cx) as usize) {
            *cell = true;
        }
    }
}

/// How many cells are alive.
///
/// `life_step` already returns this for the board it wrote, so this is for the boards nothing
/// stepped: the one the page just seeded, stamped or clicked on.
#[unsafe(no_mangle)]
pub fn life_population(cells: &[bool]) -> i32 {
    cells.iter().filter(|&&alive| alive).count() as i32
}
