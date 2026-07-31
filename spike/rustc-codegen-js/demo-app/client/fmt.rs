//! `core::fmt` driven from the page: a format specification chosen in the browser, applied by
//! the real formatting machinery, streamed back out piece by piece.
//!
//! There is no buffer anywhere in this module. `core::fmt` does not build a finished string and
//! hand it over; it walks the format template and calls `write_str` for each piece it produces,
//! and [`Emit`] forwards every one of those calls straight to the page. Padding is pieces too, so
//! a value asked for in a width of eight arrives as several calls rather than one. The piece
//! count comes back as the return value, which makes that visible from JavaScript.
//!
//! The one thing that cannot come from the page is the format template itself. A fill character,
//! an alignment and a radix are part of the template, and `core::fmt` resolves a template at
//! compile time into a table of pieces and argument descriptions. So the choices arrive as
//! numbers and are turned back into templates by matching: the fill and alignment matrix below is
//! twelve `write!` calls because there are twelve combinations, not because twelve is a pleasant
//! number. Only width and precision can be handed over at run time, through the `w$` and `.p$`
//! forms, because those two are ordinary arguments.

use core::fmt::{Display, Formatter, Result, Write};

use crate::prelude::emit;

/// Format the number as a decimal integer.
pub const KIND_INT: i32 = 0;
/// Format the text argument.
pub const KIND_TEXT: i32 = 1;
/// Format the number as the character with that Unicode scalar value.
pub const KIND_CHAR: i32 = 2;
/// Format the number as a [`Temperature`], through a hand written `Display`.
pub const KIND_DISPLAY: i32 = 3;
/// Format the number in one of the four radices.
pub const KIND_RADIX: i32 = 4;

/// Pad with `' '`.
pub const FILL_SPACE: i32 = 0;
/// Pad with `'0'`.
pub const FILL_ZERO: i32 = 1;
/// Pad with `'*'`.
pub const FILL_STAR: i32 = 2;
/// Pad with `'.'`.
pub const FILL_DOT: i32 = 3;

/// Value first, padding after it.
pub const ALIGN_LEFT: i32 = 0;
/// Padding split either side of the value, the odd character going to the right.
pub const ALIGN_CENTER: i32 = 1;
/// Padding first, value after it.
pub const ALIGN_RIGHT: i32 = 2;

/// `{:x}`.
pub const RADIX_LOWER_HEX: i32 = 0;
/// `{:X}`.
pub const RADIX_UPPER_HEX: i32 = 1;
/// `{:b}`.
pub const RADIX_BINARY: i32 = 2;
/// `{:o}`.
pub const RADIX_OCTAL: i32 = 3;

/// The largest width or precision `core::fmt` will accept.
///
/// Both are `u16` inside the formatter, and handing it anything larger is a panic of its own
/// ("Formatting argument out of range") rather than a clamp, so the page's numbers are clamped
/// here before they get that far.
const MAX_COUNT: i32 = u16::MAX as i32;

/// The precision to use when the page asks for none.
///
/// Not a placeholder: it is a real precision handed to `core::fmt`, chosen as large as one is
/// allowed to be so that no text the page could sensibly send is truncated by it. It has to be a
/// real one, because `Formatter::pad` measures a string in characters, and the only way it can
/// skip that measurement is when a precision tells it how far to walk. So "no precision" is
/// spelled here as "a precision nothing reaches".
const NO_PRECISION: usize = MAX_COUNT as usize;

/// A format specification, authored in JavaScript as an object literal and read here by field
/// name.
///
/// Every field is a number or a boolean because that is what survives the crossing unchanged:
/// `i32` arrives as a JavaScript number, `bool` as a boolean, and `i64` as a `BigInt`, which is
/// the only JavaScript type that can hold one exactly.
pub struct Spec {
    /// Which `KIND_` constant to format as.
    pub kind: i32,
    /// The number to format, reused as a Unicode scalar value by [`KIND_CHAR`] and as tenths of
    /// a degree by [`KIND_DISPLAY`].
    pub number: i64,
    /// Which `FILL_` constant to pad with.
    pub fill: i32,
    /// Which `ALIGN_` constant to pad towards.
    pub align: i32,
    /// The minimum width, or 0 for none. A width never truncates.
    pub width: i32,
    /// The maximum width in characters, or -1 for none. Only text and characters are truncated
    /// by it; a number ignores it.
    pub precision: i32,
    /// Which `RADIX_` constant [`KIND_RADIX`] uses.
    pub radix: i32,
    /// Whether a radix carries its `0x`, `0b` or `0o` prefix.
    pub alternate: bool,
    /// Whether a decimal integer carries a `+` when it is positive.
    pub sign: bool,
}

/// A `core::fmt::Write` sink that counts the pieces it is given and forwards each one.
///
/// This is the whole of the crate's output path, and it is four lines because a sink is allowed
/// to be: `core::fmt` asks only that a destination can take a `&str`. Nothing is stored, so
/// nothing has to be allocated, which matters in a crate with no allocator at all.
pub struct Emit {
    /// How many times `write_str` has been called.
    pub pieces: u32,
}

impl Write for Emit {
    fn write_str(&mut self, s: &str) -> Result {
        self.pieces += 1;
        emit(s);
        Ok(())
    }
}

/// A temperature in tenths of a degree Celsius.
///
/// The point of it is the `Display` impl: the page picks the kind, but the *formatting* of a
/// value of a type `core` has never heard of is written here, in Rust, and reached through
/// exactly the same `{}` the built in types are.
pub struct Temperature {
    /// Tenths of a degree, so 235 is 23.5 degrees.
    pub tenths: i32,
}

impl Display for Temperature {
    /// Writes the value as `23.5°C`.
    ///
    /// Worth noticing twice over. First, a hand written `Display` that writes to the formatter
    /// directly ignores the width, fill and alignment the caller asked for: those are offered to
    /// an impl through `Formatter`, and an impl that does not consult them is not padded. That is
    /// ordinary Rust, and it is why the display kind comes back unpadded no matter what the page
    /// selects. Second, `°` is two bytes of UTF-8, and it crosses to JavaScript intact, which is
    /// less obvious than it sounds for a language whose strings are UTF-16.
    ///
    /// Below one tenth of a degree the integer division loses the sign, so -5 tenths reads as
    /// `0.5°C`. Splitting the sign out is the fix, and this impl is the version that does not.
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}.{}°C", self.tenths / 10, (self.tenths % 10).abs())
    }
}

/// Formats `value` into `out` with the fill and alignment the page chose.
///
/// The twelve arms are the whole trick. A fill character and an alignment live inside the format
/// template, and templates are compiled, so there is no way to compute one; what there is instead
/// is one arm per combination, each with the template spelled out, chosen by a `match` over the
/// two numbers. Width and precision do not need an arm each because `w$` and `.p$` read them from
/// arguments at run time.
///
/// `value` is a `&dyn Display`, so the same twelve arms serve integers, text, characters and
/// [`Temperature`] alike: one virtual call per formatted value, and one copy of the matrix rather
/// than four.
fn write_padded(
    out: &mut Emit,
    value: &dyn Display,
    fill: i32,
    align: i32,
    width: usize,
    precision: usize,
) {
    let _ = match (fill, align) {
        (FILL_SPACE, ALIGN_LEFT) => write!(out, "{: <width$.precision$}", value),
        (FILL_SPACE, ALIGN_CENTER) => write!(out, "{: ^width$.precision$}", value),
        (FILL_SPACE, ALIGN_RIGHT) => write!(out, "{: >width$.precision$}", value),
        (FILL_ZERO, ALIGN_LEFT) => write!(out, "{:0<width$.precision$}", value),
        (FILL_ZERO, ALIGN_CENTER) => write!(out, "{:0^width$.precision$}", value),
        (FILL_ZERO, ALIGN_RIGHT) => write!(out, "{:0>width$.precision$}", value),
        (FILL_STAR, ALIGN_LEFT) => write!(out, "{:*<width$.precision$}", value),
        (FILL_STAR, ALIGN_CENTER) => write!(out, "{:*^width$.precision$}", value),
        (FILL_STAR, ALIGN_RIGHT) => write!(out, "{:*>width$.precision$}", value),
        (FILL_DOT, ALIGN_LEFT) => write!(out, "{:.<width$.precision$}", value),
        (FILL_DOT, ALIGN_CENTER) => write!(out, "{:.^width$.precision$}", value),
        (FILL_DOT, ALIGN_RIGHT) => write!(out, "{:.>width$.precision$}", value),
        // A fill or an alignment the page has not defined: no alignment at all, which leaves
        // each type to pad the way it prefers. Text goes left, a number goes right.
        _ => write!(out, "{:width$.precision$}", value),
    };
}

/// Formats `number` in the chosen radix, zero padded to `width`.
///
/// Radix and prefix are template syntax, so this is another matrix: four radices times the `#`
/// that turns `ff` into `0xff`. The padding here is always zero padding, because that is what a
/// number written in a radix is conventionally padded with, and because `{:#010x}` puts its zeros
/// after the `0x` rather than in front of it, which no fill and alignment pair can do.
///
/// A negative number has no minus sign in a radix. It is printed from its bits, so -1 as `i64` is
/// sixteen `f`s.
fn write_radix(out: &mut Emit, number: i64, radix: i32, alternate: bool, width: usize) {
    let _ = match (radix, alternate) {
        (RADIX_LOWER_HEX, false) => write!(out, "{:0width$x}", number),
        (RADIX_LOWER_HEX, true) => write!(out, "{:#0width$x}", number),
        (RADIX_UPPER_HEX, false) => write!(out, "{:0width$X}", number),
        (RADIX_UPPER_HEX, true) => write!(out, "{:#0width$X}", number),
        (RADIX_BINARY, false) => write!(out, "{:0width$b}", number),
        (RADIX_BINARY, true) => write!(out, "{:#0width$b}", number),
        (RADIX_OCTAL, false) => write!(out, "{:0width$o}", number),
        (RADIX_OCTAL, true) => write!(out, "{:#0width$o}", number),
        // A radix the page has not defined. Base ten has no `#` prefix to add.
        _ => write!(out, "{:0width$}", number),
    };
}

/// Formats `number` as a decimal integer, padded to `width`.
///
/// A `+` is template syntax like a fill character is, so asking for one is an arm of its own
/// rather than an extra argument, and it comes with the `{:+width$}` zero-free padding rather
/// than the matrix.
///
/// The matrix path has a wart worth putting on the page. A fill of `'0'` chosen here is a fill
/// character, which is not the same thing as the `0` flag in `{:04}`: the flag is sign aware and
/// puts its zeros after the minus, a fill is not and pads the whole thing. So -7 in a width of
/// four is `-007` from `{:04}` and `00-7` from a `'0'` fill with right alignment. Both are
/// reachable from the page, which makes the difference easy to see rather than easy to trip over.
///
/// A precision is not passed on, because a number has no use for one: `Formatter::pad_integral`,
/// which every integer `Display` ends in, never looks at it.
fn write_int(out: &mut Emit, number: i64, sign: bool, fill: i32, align: i32, width: usize) {
    if sign {
        let _ = write!(out, "{:+width$}", number);
    } else {
        write_padded(out, &number, fill, align, width, NO_PRECISION);
    }
}

/// Formats one value the way `spec` asks and returns how many pieces that took.
///
/// The pieces themselves have already gone to the page through [`Emit`] by the time this returns,
/// so the count is a receipt rather than the result: joining the pieces gives the string, and the
/// count says how much work assembling it was. A value that needs no padding is one piece; a
/// padded one is a piece per fill character plus the value.
///
/// `text` is only read by [`KIND_TEXT`]; the other kinds all work from `spec.number`.
#[unsafe(no_mangle)]
pub fn fmt_demo(spec: &Spec, text: &str) -> i32 {
    let mut out = Emit { pieces: 0 };

    let width = spec.width.clamp(0, MAX_COUNT) as usize;
    let precision = if spec.precision < 0 {
        NO_PRECISION
    } else {
        spec.precision.min(MAX_COUNT) as usize
    };

    match spec.kind {
        KIND_TEXT => write_padded(&mut out, &text, spec.fill, spec.align, width, precision),
        KIND_CHAR => {
            let value = char::from_u32(spec.number as u32).unwrap_or('?');
            write_padded(&mut out, &value, spec.fill, spec.align, width, precision)
        }
        KIND_DISPLAY => {
            let value = Temperature { tenths: spec.number as i32 };
            write_padded(&mut out, &value, spec.fill, spec.align, width, precision)
        }
        KIND_RADIX => write_radix(&mut out, spec.number, spec.radix, spec.alternate, width),
        KIND_INT => write_int(&mut out, spec.number, spec.sign, spec.fill, spec.align, width),
        // A kind the page has not defined falls back to the plainest one there is.
        _ => write_int(&mut out, spec.number, spec.sign, spec.fill, spec.align, width),
    }

    out.pieces as i32
}
