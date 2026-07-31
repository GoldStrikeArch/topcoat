//! The JSON a procedure's arguments and its reply cross the wire as.
//!
//! Both functions are bound on serde's own traits rather than on a concrete
//! JSON type. That is what lets the client half of this crate answer them with
//! a hand-written encoder sized for a browser while the generated code that
//! calls them stays the same.

use serde::{Serialize, de::DeserializeOwned};

use crate::{Error, Result};

/// `value` as JSON.
///
/// A procedure's arguments are a tuple, which is a JSON array, so a call with
/// one argument still sends an array of one.
///
/// # Errors
///
/// Returns `Err` when `value` cannot be represented as JSON.
pub fn to_json<T>(value: &T) -> Result<String>
where
    T: Serialize + ?Sized,
{
    serde_json::to_string(value).map_err(|err| Error::new(err.to_string()))
}

/// The JSON in `text`, read as a `T`.
///
/// # Errors
///
/// Returns `Err` when `text` is not JSON, or is JSON of another shape.
pub fn from_json<T>(text: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str(text).map_err(|err| Error::new(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_cross_as_an_array_even_when_there_is_one() {
        // The shape the generated call builds: a tuple of references.
        assert_eq!(to_json(&(&"ru",)).unwrap(), r#"["ru"]"#);
        assert_eq!(to_json(&(&"ru", &2)).unwrap(), r#"["ru",2]"#);
    }

    #[test]
    fn a_reply_reads_back_as_the_type_the_procedure_returns() {
        let reply: Vec<String> = from_json(r#"["rust","ruby"]"#).unwrap();
        assert_eq!(reply, ["rust", "ruby"]);
    }

    #[test]
    fn a_reply_of_another_shape_reports_rather_than_panics() {
        let err = from_json::<Vec<String>>("{}").unwrap_err();
        assert!(!err.message().is_empty());
    }
}
