use std::fmt;

use serde::{Deserialize, Deserializer, de};
use topcoat_core::island::IslandInstance;
use uuid::Uuid;

/// The identity of one runtime value the client has to find again.
///
/// A value declared inside an island is numbered by its position in that
/// island, so the client arrives at the same id by walking the same view.
/// Outside an island nothing predicts ids, so a random one is cheaper and
/// cannot collide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Id {
    Random(Uuid),
    Island {
        instance: IslandInstance,
        ordinal: u32,
    },
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Random(uuid) => uuid.fmt(f),
            // The same encoding hydration keys are written with, so the one
            // island numbering has one spelling everywhere it is written down.
            Self::Island { instance, ordinal } => instance.write_key(f, *ordinal),
        }
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = <&str>::deserialize(deserializer)?;
        encoded.parse().map_err(de::Error::custom)
    }
}

impl std::str::FromStr for Id {
    type Err = ParseIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(uuid) = Uuid::parse_str(s) {
            return Ok(Self::Random(uuid));
        }
        let (instance, ordinal) = IslandInstance::parse_key(s).ok_or(ParseIdError)?;
        Ok(Self::Island { instance, ordinal })
    }
}

/// The error returned when a string is neither a random id nor an island one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParseIdError;

impl fmt::Display for ParseIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a uuid or an island id such as `i0.3`")
    }
}

impl std::error::Error for ParseIdError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(id: Id) {
        assert_eq!(id.to_string().parse::<Id>().unwrap(), id);
    }

    #[test]
    fn a_random_id_renders_as_its_uuid() {
        let id = Id::Random(Uuid::nil());
        assert_eq!(id.to_string(), "00000000-0000-0000-0000-000000000000");
        round_trip(id);
    }

    #[test]
    fn an_island_id_renders_as_the_instance_and_the_ordinal() {
        let single_digit = Id::Island {
            instance: IslandInstance::from_index(2),
            ordinal: 3,
        };
        assert_eq!(single_digit.to_string(), "i2.3");
        round_trip(single_digit);

        // Past one digit the ordinal carries its length, the way a hydration
        // key does: one island numbering, one spelling.
        let two_digits = Id::Island {
            instance: IslandInstance::from_index(2),
            ordinal: 13,
        };
        assert_eq!(two_digits.to_string(), "i2.a13");
        round_trip(two_digits);
    }

    #[test]
    fn a_string_that_is_neither_form_is_rejected() {
        for encoded in ["", "i0", "0.1", "ix.1", "i0.x", "i0.1.2", "i0.13", "i0.a1"] {
            assert!(
                encoded.parse::<Id>().is_err(),
                "`{encoded}` should not parse",
            );
        }
    }
}
