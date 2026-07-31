//! The boundary between this crate and the page that hosts it.
//!
//! Three things cross that boundary, all of them declared here: the two functions the page
//! lends the crate (`js_log_str` and `js_emit`), and the one it lends the panic handler
//! (`js_panic`). An `extern` declaration is a promise the compiler cannot check, so it is
//! inherently `unsafe`, and this module is the only place in the crate that writes the word.
//! Everything else calls the safe one-line wrappers below.
//!
//! In the emitted JavaScript an `extern "C"` function is a call to `__rt.<name>(...)`, so this
//! module is also the shortest way to see the calling convention: a `&str` crosses as a plain
//! JavaScript string, and a `-> !` function is one the emitted code never returns from.

#![allow(dead_code)]

// `improper_ctypes` is the right warning for the wrong target. It says a `&str` has no C
// equivalent and to pass a pointer and a length instead, which is true of C and not of the
// language on the other side of this boundary: a `&str` crosses to JavaScript as a JavaScript
// string, so a pointer and a length would be the awkward encoding here rather than the natural
// one.
#[allow(improper_ctypes)]
unsafe extern "C" {
    /// Writes one line to the page's log pane.
    pub fn js_log_str(s: &str);

    /// Takes one piece of a formatted string. `core::fmt` hands out a formatted value in
    /// pieces rather than as a finished buffer, and this is where they land.
    pub fn js_emit(piece: &str);

    /// Ends the program with a message and the source location it came from.
    pub fn js_panic(message: &str, file: &str, line: u32, column: u32) -> !;
}

/// Writes one line to the page's log pane.
pub fn log(s: &str) {
    unsafe { js_log_str(s) }
}

/// Hands one piece of a formatted string to the page.
pub fn emit(piece: &str) {
    unsafe { js_emit(piece) }
}

/// Ends the program, reporting where it went wrong.
pub fn abort(message: &str, file: &str, line: u32, column: u32) -> ! {
    unsafe { js_panic(message, file, line, column) }
}

/// The one item a `no_std` binary owes `core`: where a panic goes.
///
/// Both halves of a `PanicInfo` are readable here. `message()` is a `core::fmt::Arguments`, and
/// `as_str()` returns its text whenever the template is a single literal with nothing
/// interpolated into it, which covers the messages `core` itself raises for `unwrap` on a `None`
/// and for a division by zero. A message assembled from arguments, such as the index and length
/// in a bounds check, has no single literal to return and reports as "panicked" instead.
///
/// `location()` is the end of the `#[track_caller]` chain: a `&Location` that each caller in the
/// chain passed along, which is why the file and line reported are the ones a reader of the
/// source would point at rather than a line inside `core`. See `fail.rs` for what that chain is
/// worth.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    let message = match info.message().as_str() {
        Some(text) => text,
        None => "panicked",
    };
    let (file, line, column) = match info.location() {
        Some(location) => (location.file(), location.line(), location.column()),
        None => ("<unknown>", 0, 0),
    };
    abort(message, file, line, column)
}
