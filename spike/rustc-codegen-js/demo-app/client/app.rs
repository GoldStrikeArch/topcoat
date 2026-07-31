//! The client half of the demo: ordinary Rust, compiled to JavaScript, running in the page you
//! are reading this from.
//!
//! Nothing here is written for a compiler backend. It is a `#![no_std]` crate against the real
//! `core`, and every interesting thing it does, the iterators, the `match` over a tuple, the
//! `dyn` dispatch, the `#[track_caller]` chain, `core::fmt` itself, is `core`'s code compiled
//! alongside this crate's. The JavaScript beside this source is the output, and it is worth
//! reading in pairs: a function here, the function it became there.
//!
//! There is no allocator, so there is no `Vec`, no `String` and no `format!`. That turns out to
//! be less of a restriction than it sounds. The board in [`life`] is a JavaScript array the page
//! owns and Rust borrows, and the formatting in [`fmt`] streams its output a piece at a time
//! instead of building a string to hand back. Both are how you would write this even with an
//! allocator to hand.
//!
//! The three modules are three things to try:
//!
//! * [`life`]: Conway's Game of Life over a shared array, to watch real work happen on data
//!   neither language copies.
//! * [`fmt`]: `core::fmt` driven from controls in the page, to see what a format template is and
//!   what part of one can be decided at run time.
//! * [`fail`]: panics, and the `#[track_caller]` machinery that decides which line they blame.
//!
//! [`prelude`] holds the boundary itself, and the crate's only `unsafe`.

#![no_std]
#![no_main]

#[path = "prelude.rs"]
mod prelude;

#[path = "fail.rs"]
mod fail;
#[path = "fmt.rs"]
mod fmt;
#[path = "life.rs"]
mod life;
