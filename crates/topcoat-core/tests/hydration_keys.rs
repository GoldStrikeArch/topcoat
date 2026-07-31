//! Hydration keys against the client runtime's own numbering.
//!
//! The client mints the key it looks a node up by, so the encoding is not ours
//! to choose: the server either writes the same string or the client finds
//! nothing. `fixtures/hydration_keys.json` is that encoding, extracted from the
//! client runtime by running its key function over a spread of counts and
//! recording what came out, so these are the client's answers and not a second
//! reading of the same rule.
//!
//! Each case is a context id, a count, and the key the client produced for the
//! two. The context id is whatever the client happened to be numbering under,
//! while an [`IslandInstance`] always numbers under its own prefix, so a case
//! applies by its suffix: the part of the key past the context id is what the
//! count alone decides, and matching it is what these assert. That the suffix
//! is the same under every context id is the point of the encoding.

use std::collections::BTreeSet;

use serde_json::Value;
use topcoat_core::island::IslandInstance;

/// A case from the fixture: a count and the key suffix the client wrote for it.
struct Case {
    count: u32,
    suffix: String,
}

/// The fixture's cases, in file order.
fn cases() -> Vec<Case> {
    let fixture = include_str!("fixtures/hydration_keys.json");
    let fixture: Value = serde_json::from_str(fixture).expect("the fixture is json");
    let triples = fixture["triples"].as_array().expect("triples is an array");

    triples
        .iter()
        .map(|triple| {
            let context = triple["contextId"].as_str().expect("contextId is a string");
            let key = triple["key"].as_str().expect("key is a string");
            let count = triple["count"].as_u64().expect("count is a number");
            let suffix = key
                .strip_prefix(context)
                .expect("a key starts with its context id");
            Case {
                count: count.try_into().expect("a count fits a u32"),
                suffix: suffix.to_owned(),
            }
        })
        .collect()
}

#[test]
fn every_key_the_client_mints_is_the_key_the_server_writes() {
    let cases = cases();
    assert_eq!(cases.len(), 264, "the fixture should carry every case");

    let instance = IslandInstance::from_index(0);
    let prefix = instance.key_prefix();
    for case in &cases {
        assert_eq!(
            instance.key(case.count),
            format!("{prefix}{}", case.suffix),
            "key for ordinal {}",
            case.count,
        );
    }
}

#[test]
fn the_suffix_a_count_gets_does_not_depend_on_what_precedes_it() {
    let mut per_count: Vec<(u32, BTreeSet<String>)> = Vec::new();
    for case in cases() {
        match per_count.iter_mut().find(|(count, _)| *count == case.count) {
            Some((_, suffixes)) => {
                suffixes.insert(case.suffix);
            }
            None => per_count.push((case.count, BTreeSet::from([case.suffix]))),
        }
    }

    // The fixture covers eight context ids, so each count appears eight times.
    // One suffix each is what lets a key be written from the count alone.
    for (count, suffixes) in per_count {
        assert_eq!(suffixes.len(), 1, "count {count} wrote {suffixes:?}");
    }
}

#[test]
fn a_key_from_any_instance_parses_back_to_the_ordinal_the_client_counted() {
    for case in cases() {
        for index in [0, 1, 10, 4_294_967_295] {
            let instance = IslandInstance::from_index(index);
            let key = instance.key(case.count);
            assert_eq!(
                IslandInstance::parse_key(&key),
                Some((instance, case.count)),
                "`{key}` should parse back",
            );
        }
    }
}
