//! Panics, and where they say they came from.
//!
//! A panic in this crate ends in `prelude`'s `#[panic_handler]`, which reports a message and a
//! source location to the page. This module exists to show where that location comes from,
//! because it is not where the panic physically happened.
//!
//! `Option::unwrap` carries `#[track_caller]`. That attribute means the function is compiled with
//! an extra hidden argument, a `&Location`, and every caller fills it in with its own file, line
//! and column. So the location a panic reports is the line the reader would point at, not a line
//! inside `core`. The attribute is also transitive: put it on a function of your own and that
//! function stops being the answer and starts passing its own caller's answer along.
//!
//! [`unwrap_here`] and [`unwrap_at_caller`] are the same three lines, and differ by one
//! attribute. Running both is the demo: same message, different reported line.

/// Returns `Some` when `present` and `None` otherwise.
///
/// A function rather than a literal `None`, so that the `unwrap`s below genuinely run instead of
/// being folded away.
fn missing(present: bool) -> Option<i32> {
    if present { Some(1) } else { None }
}

/// Unwraps, and owns the blame.
///
/// No `#[track_caller]`, so this function is where the `#[track_caller]` chain ends: it fills in
/// the hidden location argument `unwrap` expects with the position of the `unwrap()` call on the
/// line below. A panic here reports *this* line, whoever called it.
fn unwrap_here(value: Option<i32>) -> i32 {
    value.unwrap()
}

/// Unwraps, and passes the blame on.
///
/// The one attribute is the whole difference from [`unwrap_here`]. With it, this function takes a
/// hidden location argument of its own and hands that same one to `unwrap` instead of making a
/// fresh one, so a panic here reports the line that called *this* function. That is what a helper
/// wants: the caller made the mistake, and the caller is where a reader needs to look.
#[track_caller]
fn unwrap_at_caller(value: Option<i32>) -> i32 {
    value.unwrap()
}

/// Panics in the way `choice` selects, or returns 7 if it selects none.
///
/// The choices are:
///
/// * 0, and anything unlisted: no panic. Returns 7, which is the page's proof that the call
///   really crossed into Rust and came back rather than being caught somewhere on the way.
/// * 1: `unwrap` on a `None` inside [`unwrap_here`], reported at that function's own line.
/// * 2: the same `unwrap` on a `None` inside [`unwrap_at_caller`], reported at the call below.
///   Two lines apart in this file, one attribute apart in the source.
/// * 3: an index past the end of an array.
/// * 4: a division by zero.
/// * 5: a `panic!` with a message of its own.
///
/// The index and the divisor are both computed from `choice` rather than written as constants, so
/// that they are only known while the program runs and nothing can decide the outcome earlier.
#[unsafe(no_mangle)]
pub fn panic_demo(choice: i32) -> i32 {
    let table = [10, 20, 30, 40];

    match choice {
        1 => unwrap_here(missing(false)),
        2 => unwrap_at_caller(missing(false)),
        3 => table[(choice as usize) * 2],
        4 => 100 / (choice - 4),
        5 => panic!("a panic message written as a literal, carried through core and back out"),
        _ => {
            crate::prelude::log("panic_demo: nothing to panic about, returning 7");
            7
        }
    }
}
