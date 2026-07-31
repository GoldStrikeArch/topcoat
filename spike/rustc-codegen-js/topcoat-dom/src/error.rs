//! The error a client-side call answers with.

use alloc::string::{String, ToString};

/// What went wrong, as text.
///
/// The same shape the server half carries, and for the same reason: a client has one thing to say
/// about a failed call and no vocabulary for it beyond what the server sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error(String);

impl Error {
    /// An error reading `message`.
    #[must_use]
    pub fn new(message: impl ToString) -> Self {
        Self(message.to_string())
    }

    /// What it says.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self(String::from(message))
    }
}

/// A result that carries an [`Error`] unless told otherwise.
pub type Result<T = (), E = Error> = core::result::Result<T, E>;

/// The two halves of a `Result` type, so a macro can name the success half of a return type it was
/// handed whole.
///
/// The client expansion writes `<Result<Hits> as ResultExt>::T` because the procedure declared
/// `Result<Hits>` and the client function returns `Result<Hits>` too, with its own error type.
pub trait ResultExt {
    /// The type the `Result` carries when it succeeded.
    type T;
    /// The type it carries when it failed.
    type E;
}

impl<T, E> ResultExt for core::result::Result<T, E> {
    type T = T;
    type E = E;
}
