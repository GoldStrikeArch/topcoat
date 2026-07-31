//! Hydration keys across component boundaries, against the client runtime's own
//! numbering.
//!
//! A component renders a view of its own, which numbers its keys from zero, so
//! the keys inside it are scoped by the numbering its caller had reached. The
//! client mints the key it looks a node up by, so that scoping is not ours to
//! choose either: `fixtures/hydration_keys_nested.json` is what the client
//! produced for a spread of component trees, and what a server rendering the same
//! tree has to produce too.
//!
//! Each case is a tree written in a small shape language and the keys the client
//! asked for, in order. A case is driven by building the view the shape describes
//! and rendering it as an island, so what is compared is the keys a render
//! actually wrote.

use serde_json::Value;
use topcoat_core::context::Cx;
use topcoat_view::{HydrationSite, View, ViewParts};

/// The marker a key list too long to store in full is abridged around.
const ELIDED: &str = "elided";

/// One case: the tree, and the keys the client asked for while hydrating it.
struct Case {
    name: String,
    shape: String,
    /// The client's keys, which may be abridged around [`ELIDED`].
    keys: Vec<String>,
    /// How many keys the unabridged list has.
    count: usize,
}

impl Case {
    /// The keys this case asserts, as `(index, key)` pairs: every key of a list
    /// stored in full, and the ends of one that is abridged.
    fn asserted(&self) -> Vec<(usize, &str)> {
        let (head, tail) = match self.keys.iter().position(|key| key.contains(ELIDED)) {
            // Stored in full, so every key is asserted at its own index.
            None => (self.keys.as_slice(), [].as_slice()),
            // Abridged, so the head keeps its indices and the tail is counted
            // back from the end of the unabridged list.
            Some(at) => (&self.keys[..at], &self.keys[at + 1..]),
        };
        let from_the_end = self.count - tail.len();
        head.iter()
            .enumerate()
            .chain(
                tail.iter()
                    .enumerate()
                    .map(|(index, key)| (from_the_end + index, key)),
            )
            .map(|(index, key)| (index, key.as_str()))
            .collect()
    }
}

/// The fixture's cases, in file order.
fn cases() -> Vec<Case> {
    let fixture = include_str!("fixtures/hydration_keys_nested.json");
    let fixture: Value = serde_json::from_str(fixture).expect("the fixture is json");
    fixture["cases"]
        .as_array()
        .expect("cases is an array")
        .iter()
        .map(|case| Case {
            name: case["name"].as_str().expect("a name").to_owned(),
            shape: case["shape"].as_str().expect("a shape").to_owned(),
            keys: case["elementKeys"]
                .as_array()
                .expect("elementKeys is an array")
                .iter()
                .map(|key| key.as_str().expect("a key").to_owned())
                .collect(),
            count: case["elementKeyCount"]
                .as_u64()
                .expect("a count")
                .try_into()
                .expect("a count fits a usize"),
        })
        .collect()
}

/// One item of a shape: `e` is a template root, `C(...)` is a component
/// boundary, `C{...}(...)` one called with child content, and `k` the place a
/// body renders the child content it was called with.
enum Item {
    Element,
    Component {
        child: Option<Vec<Item>>,
        body: Vec<Item>,
    },
    ChildContent,
}

impl Item {
    /// Parses a run of space-separated items, stopping at `)`.
    ///
    /// An item may be followed by `*n`, which repeats it.
    fn parse(shape: &mut &str) -> Vec<Self> {
        let mut items = Vec::new();
        loop {
            *shape = shape.trim_start();
            let item = if let Some(rest) = shape.strip_prefix('C') {
                *shape = rest;
                // Child content, if the boundary was called with any, comes
                // before the body: it is an argument, and that is the order it
                // is built in.
                let child = shape.strip_prefix('{').map(|rest| {
                    *shape = rest;
                    let child = Self::parse(shape);
                    *shape = shape
                        .trim_start()
                        .strip_prefix('}')
                        .expect("child content is closed");
                    child
                });
                *shape = shape
                    .trim_start()
                    .strip_prefix('(')
                    .expect("a component body is opened");
                let body = Self::parse(shape);
                *shape = shape
                    .trim_start()
                    .strip_prefix(')')
                    .expect("a component body is closed");
                Self::Component { child, body }
            } else if let Some(rest) = shape.strip_prefix('e') {
                *shape = rest;
                Self::Element
            } else if let Some(rest) = shape.strip_prefix('k') {
                *shape = rest;
                Self::ChildContent
            } else {
                return items;
            };

            let repeats = match shape.strip_prefix('*') {
                None => 1,
                Some(rest) => {
                    let digits =
                        rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                    let (count, rest) = rest.split_at(digits);
                    *shape = rest;
                    count.parse().expect("a repeat count")
                }
            };
            for _ in 1..repeats {
                items.push(item.duplicate());
            }
            items.push(item);
        }
    }

    fn duplicate(&self) -> Self {
        match self {
            Self::Element => Self::Element,
            Self::ChildContent => Self::ChildContent,
            Self::Component { child, body } => Self::Component {
                child: child.as_ref().map(|child| Self::clone_all(child)),
                body: Self::clone_all(body),
            },
        }
    }

    fn clone_all(items: &[Self]) -> Vec<Self> {
        items.iter().map(Self::duplicate).collect()
    }

    /// The view a run of items describes.
    ///
    /// A template root is written as the hydration site alone: what a case is
    /// about is which key a site gets, not the markup around it.
    fn view(items: &[Self]) -> View {
        let mut parts = ViewParts::new();
        for item in items {
            match item {
                Self::Element => parts.push_hydration_site(HydrationSite::TemplateRoot),
                Self::ChildContent => parts.push_view(View::child_content()),
                Self::Component { child, body } => {
                    let body = Self::view(body);
                    parts.push_view(match child {
                        None => body.component(),
                        Some(child) => body.component_with_child(Self::view(child)),
                    })
                }
            };
        }
        View::new(parts)
    }
}

/// The keys a shape's view writes, with the island's own prefix taken off.
fn keys(shape: &str) -> Vec<String> {
    let mut rest = shape;
    let items = Item::parse(&mut rest);
    assert!(
        rest.trim().is_empty(),
        "`{shape}` was not read to the end, `{rest}` is left",
    );

    let cx = Cx::default();
    let instance = cx.islands().next_instance();
    let html = Item::view(&items).island(instance).render(&cx);

    let prefix = instance.key_prefix();
    html.split(" data-hk=\"")
        .skip(1)
        .map(|rest| {
            let key = rest.split('"').next().expect("the attribute is closed");
            key.strip_prefix(&prefix)
                .expect("a key starts with its island's prefix")
                .to_owned()
        })
        .collect()
}

/// The eager child content rows: the shape and the keys it writes, in document
/// order.
fn eager_child_content_rows() -> Vec<(String, String, Vec<String>)> {
    let fixture = include_str!("fixtures/hydration_keys_nested.json");
    let fixture: Value = serde_json::from_str(fixture).expect("the fixture is json");
    fixture["eagerChildContent"]["rows"]
        .as_array()
        .expect("rows is an array")
        .iter()
        .map(|row| {
            (
                row["name"].as_str().expect("a name").to_owned(),
                row["shape"].as_str().expect("a shape").to_owned(),
                row["keys"]
                    .as_array()
                    .expect("keys is an array")
                    .iter()
                    .map(|key| key.as_str().expect("a key").to_owned())
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn the_shape_language_reads_the_trees_the_fixture_describes() {
    // The parser is what every case below is read through, so it is worth
    // pinning on its own.
    assert_eq!(keys("e").len(), 1);
    assert_eq!(keys("e*3").len(), 3);
    assert_eq!(keys("C()").len(), 0);
    assert_eq!(keys("C(e) e").len(), 2);
    assert_eq!(keys("C(e)*3").len(), 3);
    assert_eq!(keys("C(e C(e)) e").len(), 3);
    assert_eq!(keys("C{e}(e k)").len(), 2);
    assert_eq!(keys("C{e}(k)*2").len(), 2);
    assert_eq!(keys("C{e}(e)").len(), 1);
}

#[test]
fn eager_child_content_is_numbered_where_the_call_is() {
    let rows = eager_child_content_rows();
    assert_eq!(rows.len(), 9, "the fixture should carry every row");

    for (name, shape, expected) in &rows {
        assert_eq!(&keys(shape), expected, "`{shape}` ({name})");
    }
}

#[test]
fn every_key_the_client_asks_for_across_a_boundary_is_the_key_the_server_writes() {
    let cases = cases();
    assert_eq!(cases.len(), 102, "the fixture should carry every case");

    for case in &cases {
        let written = keys(&case.shape);
        assert_eq!(
            written.len(),
            case.count,
            "`{}` ({}) wrote {} keys for {} nodes",
            case.shape,
            case.name,
            written.len(),
            case.count,
        );
        for (index, expected) in case.asserted() {
            assert_eq!(
                written[index], expected,
                "`{}` ({}) key {index}",
                case.shape, case.name,
            );
        }
    }
}

#[test]
fn a_boundarys_spend_outlives_the_component() {
    // The trap the contract names: a component spends one slot of its caller and
    // the spend has to survive the restore. Handing back a saved copy of the
    // caller's numbering would replay the slot and shift every later key.
    assert_eq!(keys("C(e) C(e) C(e)"), ["00", "10", "20"]);
    assert_eq!(keys("e C(e) e"), ["0", "10", "2"]);
}

#[test]
fn an_empty_component_still_spends_a_slot() {
    assert_eq!(keys("C() e"), ["1"]);
    assert_eq!(keys("C()*3 e"), ["3"]);
}

#[test]
fn the_letter_comes_from_the_counter_of_the_context_that_spends_the_slot() {
    // Not from the depth and not from the position among siblings: both counters
    // cross the ten boundary here, and each writes its own letter.
    let written = keys("e*10 C(e*11) e");
    assert_eq!(written.first().map(String::as_str), Some("0"));
    assert_eq!(written[9], "9");
    assert_eq!(written[10], "a100");
    assert_eq!(written[20], "a10a10");
    assert_eq!(written[21], "a11");
}
