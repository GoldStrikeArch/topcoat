use std::fmt::{self, Display, Formatter};

/// What went wrong on the way to or back from a procedure.
///
/// A client has no request context to carry an error through and no status to
/// answer with, so an error is a message and nothing else. Everything a client
/// can fail at reports through this: encoding the arguments, the call itself,
/// and decoding the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    /// An error reporting `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// What went wrong.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self(message.to_owned())
    }
}

/// What anything on the client side of a view answers with.
pub type Result<T = (), E = Error> = std::result::Result<T, E>;

/// The two halves of a `Result`, whatever the type was spelled as.
///
/// A procedure declares the type it returns, and the client half has to name
/// that type's `Ok` half on its own. Reading it off this trait resolves it even
/// when the procedure was written through an alias, which naming `Result`'s
/// first parameter directly would not.
pub trait ResultExt {
    /// The type the `Result` carries when it succeeded.
    type T;
    /// The type it carries when it failed.
    type E;
}

impl<T, E> ResultExt for std::result::Result<T, E> {
    type T = T;
    type E = E;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_displays_the_message_it_was_made_from() {
        assert_eq!(Error::new("no reply").to_string(), "no reply");
        assert_eq!(Error::from("no reply".to_owned()).message(), "no reply");
        assert_eq!(Error::from("no reply"), Error::new("no reply"));
    }

    #[test]
    fn result_ext_names_both_halves_through_an_alias() {
        type Answer = Result<u8>;
        // Both are named without spelling `Result`'s parameters, which is what
        // the generated call needs when the procedure used an alias.
        let ok: <Answer as ResultExt>::T = 1;
        let err: <Answer as ResultExt>::E = Error::new("no");
        assert_eq!(ok, 1);
        assert_eq!(err.message(), "no");
    }
}
