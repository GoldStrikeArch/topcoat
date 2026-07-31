//! Generates the contract tables the emitter classifies against.
//!
//! The tables live in `contract/fixtures/`, extracted from the pinned upstream tree
//! by the contract harness. Reading them here rather than copying them means an
//! upstream drift that changes a fixture is a build failure in this crate, not a
//! silent disagreement with the runtime.

use std::{env, fmt::Write as _, fs, path::Path};

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../contract/fixtures");
    let events = read(&fixtures.join("delegated-events.json"));
    let properties = read(&fixtures.join("properties.json"));

    let mut out = String::new();
    write_slice(&mut out, "DELEGATED_EVENTS", &array(&events, "events"));
    write_slice(&mut out, "PROPERTIES", &array(&properties, "Properties"));
    write_slice(
        &mut out,
        "CHILD_PROPERTIES",
        &array(&properties, "ChildProperties"),
    );
    write_slice(
        &mut out,
        "BOOLEAN_ATTRIBUTES",
        &array(&properties, "BooleanAttributes"),
    );
    write_slice(&mut out, "SVG_ELEMENTS", &array(&properties, "SVGElements"));
    write_pairs(&mut out, "ALIASES", &object(&properties, "Aliases"));

    let path = Path::new(&env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("contract.rs");
    fs::write(&path, out).expect("the generated tables are writable");
}

/// Reads a fixture, and tells cargo to rerun when it changes.
fn read(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn write_slice(out: &mut String, name: &str, values: &[String]) {
    let count = values.len();
    writeln!(out, "pub const {name}: [&str; {count}] = [").expect("writing to a string succeeds");
    for value in values {
        writeln!(out, "    {value:?},").expect("writing to a string succeeds");
    }
    out.push_str("];\n");
}

fn write_pairs(out: &mut String, name: &str, values: &[(String, String)]) {
    let count = values.len();
    writeln!(out, "pub const {name}: [(&str, &str); {count}] = [")
        .expect("writing to a string succeeds");
    for (key, value) in values {
        writeln!(out, "    ({key:?}, {value:?}),").expect("writing to a string succeeds");
    }
    out.push_str("];\n");
}

/// The string array at the top-level `key` of `json`.
fn array(json: &str, key: &str) -> Vec<String> {
    let mut cursor = Cursor::at_value(json, key, '[');
    let mut values = Vec::new();
    while let Some(value) = cursor.next_string() {
        values.push(value);
    }
    values
}

/// The string-to-string object at the top-level `key` of `json`.
fn object(json: &str, key: &str) -> Vec<(String, String)> {
    let mut cursor = Cursor::at_value(json, key, '{');
    let mut values = Vec::new();
    while let Some(name) = cursor.next_string() {
        let value = cursor
            .next_string()
            .unwrap_or_else(|| panic!("`{key}.{name}` has no value"));
        values.push((name, value));
    }
    values
}

/// Reads the string literals of one JSON array or object.
///
/// The fixtures are machine generated and hold nothing but strings inside these
/// values, so scanning for quotes is enough and saves a JSON dependency in the
/// build graph.
struct Cursor<'a> {
    rest: &'a str,
    close: char,
}

impl<'a> Cursor<'a> {
    /// A cursor over the value opened by `open` at the top-level `key` of `json`.
    fn at_value(json: &'a str, key: &str, open: char) -> Self {
        let quoted = format!("\"{key}\"");
        let start = json
            .find(&quoted)
            .unwrap_or_else(|| panic!("the fixture has no `{key}`"))
            + quoted.len();
        let rest = &json[start..];
        let open_at = rest
            .find(open)
            .unwrap_or_else(|| panic!("`{key}` does not open with `{open}`"));
        Self {
            rest: &rest[open_at + 1..],
            close: if open == '[' { ']' } else { '}' },
        }
    }

    /// The next string literal, or `None` at the end of the value.
    fn next_string(&mut self) -> Option<String> {
        let mut characters = self.rest.char_indices();
        loop {
            let (_, character) = characters.next()?;
            if character == self.close {
                self.rest = "";
                return None;
            }
            if character == '"' {
                break;
            }
        }

        let mut value = String::new();
        let mut escaped = false;
        for (index, character) in characters {
            if escaped {
                value.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                self.rest = &self.rest[index + 1..];
                return Some(value);
            } else {
                value.push(character);
            }
        }
        panic!("a string literal is unterminated")
    }
}
