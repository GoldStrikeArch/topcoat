//! The JSON a client-side call encodes its arguments in and decodes its reply from.
//!
//! The server half of this crate is `serde_json`, and this is not. Compiling `serde` and
//! `serde_json` through the JavaScript backend is a probe of its own, and the seam does not need
//! it settled: the client expansion of `#[procedure(serde)]` calls `to_json` and `from_json` and
//! names no bound, so it compiles against either. What a small implementation costs is generality;
//! what it buys is that the seam can be run today rather than blocked behind a dependency.
//!
//! The shapes here are the ones the wire has: an argument list is a JSON ARRAY, always, even for
//! one argument and even for none (`contract/PROCEDURES.md`), and a reply is whatever the
//! procedure returned.

use alloc::string::{String, ToString};

use crate::{Error, Result};

/// A value that can be written as JSON.
pub trait ToJson {
    /// Appends this value's JSON to `out`.
    fn write_json(&self, out: &mut String);
}

/// A value that can be read from JSON.
pub trait FromJson: Sized {
    /// Reads one from `text`, which is the whole document.
    ///
    /// # Errors
    ///
    /// Returns why `text` is not this shape.
    fn from_json(text: &str) -> Result<Self>;
}

/// `value` as JSON.
///
/// # Errors
///
/// Returns why the value could not be written. Nothing here fails today; the signature is the
/// server half's, because the expansion writes a `?` after this call.
pub fn to_json<T>(value: &T) -> Result<String>
where
    T: ToJson + ?Sized,
{
    let mut out = String::new();
    value.write_json(&mut out);
    Ok(out)
}

/// The `T` in `text`.
///
/// # Errors
///
/// Returns why `text` is not a `T`.
pub fn from_json<T>(text: &str) -> Result<T>
where
    T: FromJson,
{
    T::from_json(text)
}

impl<T> ToJson for &T
where
    T: ToJson + ?Sized,
{
    fn write_json(&self, out: &mut String) {
        (**self).write_json(out);
    }
}

impl ToJson for str {
    fn write_json(&self, out: &mut String) {
        out.push('"');
        for character in self.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                other => out.push(other),
            }
        }
        out.push('"');
    }
}

impl ToJson for String {
    fn write_json(&self, out: &mut String) {
        self.as_str().write_json(out);
    }
}

impl ToJson for bool {
    fn write_json(&self, out: &mut String) {
        out.push_str(match self {
            true => "true",
            false => "false",
        });
    }
}

impl ToJson for f64 {
    fn write_json(&self, out: &mut String) {
        out.push_str(&self.to_string());
    }
}

/// A one-argument call. The array is what makes a single argument a list rather than a bare value,
/// which is the wire's rule and the one a hand-written client gets wrong.
impl<A> ToJson for (A,)
where
    A: ToJson,
{
    fn write_json(&self, out: &mut String) {
        out.push('[');
        self.0.write_json(out);
        out.push(']');
    }
}

impl<A, B> ToJson for (A, B)
where
    A: ToJson,
    B: ToJson,
{
    fn write_json(&self, out: &mut String) {
        out.push('[');
        self.0.write_json(out);
        out.push(',');
        self.1.write_json(out);
        out.push(']');
    }
}

impl FromJson for String {
    fn from_json(text: &str) -> Result<Self> {
        let body = text.trim();
        let inner = body
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .ok_or_else(|| Error::new("the reply is not a JSON string"))?;

        let mut out = String::with_capacity(inner.len());
        let mut characters = inner.chars();
        while let Some(character) = characters.next() {
            if character != '\\' {
                out.push(character);
                continue;
            }
            match characters.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => return Err(Error::new("the reply ends in an escape")),
            }
        }
        Ok(out)
    }
}

impl FromJson for bool {
    fn from_json(text: &str) -> Result<Self> {
        match text.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => Err(Error::new(alloc::format!("`{other}` is not a boolean"))),
        }
    }
}
