//! The upstream constant tables the emitter classifies against.
//!
//! The tables are generated at build time from `contract/fixtures/`, so a drift
//! between this crate and the pinned runtime shows up as a build or test failure
//! rather than as markup the runtime mishandles.

include!(concat!(env!("OUT_DIR"), "/contract.rs"));

/// Returns `true` if handlers for `name` are delegated from the document root
/// rather than bound on the node with `addEventListener`.
#[must_use]
pub fn is_delegated(name: &str) -> bool {
    DELEGATED_EVENTS.contains(&name)
}

/// Returns `true` if `name` is written as a DOM property rather than as an
/// attribute.
///
/// The child properties (`innerHTML`, `textContent`, ...) are properties too: they
/// are never inlined into a template, always assigned at runtime.
#[must_use]
pub fn is_property(name: &str) -> bool {
    PROPERTIES.contains(&name) || CHILD_PROPERTIES.contains(&name)
}

/// Returns `true` if `name` is an attribute whose presence alone carries the
/// value.
#[must_use]
pub fn is_boolean_attribute(name: &str) -> bool {
    BOOLEAN_ATTRIBUTES.contains(&name)
}

/// Returns `true` if a template rooted at `name` has to be wrapped in a literal
/// `<svg>` and flagged `isSVG`.
///
/// `<svg>` itself is an ordinary HTML element to the fragment parser, so a
/// template rooted at one parses in the right namespace already and takes no
/// flag. Every other SVG element only parses as SVG inside an `<svg>`, so it
/// needs the wrapper, and the wrapper is what the flag's extra unwrap depth
/// reads through.
///
/// Names are compared verbatim: SVG tag names are camelCased (`linearGradient`,
/// `feGaussianBlur`) and reach the template as they were written.
#[must_use]
pub fn is_svg_only_element(name: &str) -> bool {
    name != "svg" && SVG_ELEMENTS.contains(&name)
}

/// The attribute a property name is written as, for the elements that take the
/// attribute form: `className` is written `class`, `htmlFor` is written `for`.
#[must_use]
pub fn alias(name: &str) -> Option<&'static str> {
    ALIASES
        .iter()
        .find(|(property, _)| *property == name)
        .map(|(_, attribute)| *attribute)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_contract_pins_twenty_two_delegated_events() {
        assert_eq!(DELEGATED_EVENTS.len(), 22);
    }

    #[test]
    fn delegated_event_names_are_unique_and_sorted() {
        let mut sorted = DELEGATED_EVENTS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, DELEGATED_EVENTS.to_vec());
    }

    #[test]
    fn only_listed_events_are_delegated() {
        assert!(is_delegated("click"));
        assert!(is_delegated("pointerdown"));
        assert!(!is_delegated("focus"));
        assert!(!is_delegated("submit"));
        assert!(!is_delegated("Click"));
    }

    #[test]
    fn properties_cover_both_casings_and_the_child_properties() {
        assert!(is_property("value"));
        assert!(is_property("readOnly"));
        assert!(is_property("readonly"));
        assert!(is_property("innerHTML"));
        assert!(is_property("textContent"));
        assert!(!is_property("href"));
        assert!(!is_property("title"));
    }

    #[test]
    fn boolean_attributes_are_the_lowercase_spellings() {
        assert!(is_boolean_attribute("disabled"));
        assert!(is_boolean_attribute("checked"));
        assert!(!is_boolean_attribute("value"));
    }

    #[test]
    fn the_two_upstream_aliases_are_read() {
        assert_eq!(alias("className"), Some("class"));
        assert_eq!(alias("htmlFor"), Some("for"));
        assert_eq!(alias("class"), None);
    }

    #[test]
    fn the_contract_pins_seventy_seven_svg_elements() {
        assert_eq!(SVG_ELEMENTS.len(), 77);
        assert!(SVG_ELEMENTS.contains(&"svg"));
    }

    #[test]
    fn only_an_svg_element_that_is_not_svg_itself_needs_the_wrapper() {
        // The root of a template that already parses in the SVG namespace.
        assert!(!is_svg_only_element("svg"));
        // The ones that do not, including the camelCased spellings, which are
        // compared verbatim rather than lowercased.
        assert!(is_svg_only_element("rect"));
        assert!(is_svg_only_element("linearGradient"));
        assert!(is_svg_only_element("feGaussianBlur"));
        assert!(!is_svg_only_element("lineargradient"));
        // Ordinary HTML, including the two names SVG and HTML share.
        assert!(!is_svg_only_element("div"));
        assert!(!is_svg_only_element("my-element"));
        // `font` and `image` are SVG elements upstream even though HTML has the
        // names too, so a template rooted at one is wrapped. That follows the
        // reference plugin rather than second-guessing it.
        assert!(is_svg_only_element("font"));
        assert!(is_svg_only_element("image"));
    }
}
