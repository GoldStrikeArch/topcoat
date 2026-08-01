//! The client half of the page's islands: a `#![no_std]` crate whose whole
//! content is the island modules the server also compiles.
//!
//! There is no code of its own here on purpose. An island's client half is
//! generated from the same view the server rendered, and the entry point the
//! loader calls is the `#[no_mangle]` function that generation produces, which
//! is both this crate's export and the root its dead code elimination keeps.

// What is NOT here, and used to be: `#![allow(internal_features)]` and
// `#![feature(panic_internals, trivial_clone, derive_clone_copy_internals)]`.
//
// A crate that uses the view macros is macro expanded first and the EXPANDED
// TEXT is what gets compiled, so anything a macro lowers to has to be spelled in
// a source file rather than only understood by the compiler that wrote it.
// Three of the library's own lowerings are internal and need a gate for that
// reason alone. None of it is about the islands: the same code compiled in ONE
// pass needs no gate at all.
//
// So the gates are a property of the two-pass arrangement, which is jsc-build's
// and not this crate's, and jsc-build injects them into the expand step. See
// `jsc-build/src/toolchain.rs`, `EXPANSION_GATES`, which also says which
// lowering wants which gate.
#![no_std]
#![no_main]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[path = "../island/bench.rs"]
mod bench;

#[path = "../island/counter.rs"]
mod counter;

#[path = "../island/life.rs"]
mod life;

#[path = "../island/mines.rs"]
mod mines;

#[path = "../island/nested.rs"]
mod nested;

#[path = "../island/panel.rs"]
mod panel;

#[path = "../island/sand.rs"]
mod sand;
