use std::{
    fmt, io,
    path::{Path, PathBuf},
};

use console::style;
use jsc_build::{Fix, SPIKE_ROOT_ENV, Unmet};

/// Why a package's client toolchain could not be prepared.
pub enum ClientError {
    /// Nothing says which spike checkout the named package compiles its client
    /// crate with.
    NoSpikeRoot(String),
    /// The checkout cannot be used.
    Build(jsc_build::BuildError),
    /// A setup step failed. The command it ran is carried along, so a reader
    /// can run it by hand and watch it fail in full.
    Step {
        command: String,
        source: jsc_build::BuildError,
    },
    /// A step ran and reported success, but what it was to produce is still
    /// not there.
    StillUnmet(Box<Unmet>),
    /// The lock serializing setup runs could not be taken.
    Lock { path: PathBuf, source: io::Error },
}

impl ClientError {
    /// The step run by `fix` failed.
    pub(super) fn step(fix: &Fix, source: jsc_build::BuildError) -> Self {
        Self::Step {
            command: fix.to_string(),
            source,
        }
    }

    /// The lock at `path` could not be taken.
    pub(super) fn lock(path: &Path, source: io::Error) -> Self {
        Self::Lock {
            path: PathBuf::from(path),
            source,
        }
    }

    /// Report the error to the terminal and exit non-zero.
    pub fn print_and_exit(self) -> ! {
        eprintln!("  {}", style(self.to_string()).red().bold());
        eprintln!();
        std::process::exit(1);
    }
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSpikeRoot(package) => write!(
                f,
                concat!(
                    "cannot tell which checkout {} compiles its client crate with; ",
                    "set {} or [package.metadata.topcoat.client] spike-root",
                ),
                package, SPIKE_ROOT_ENV,
            ),
            Self::Build(error) => write!(f, "{error}"),
            Self::Step { command, source } => write!(f, "{source}\n{command}"),
            Self::StillUnmet(unmet) => write!(
                f,
                "the {} step reported success but left work undone: {unmet}",
                unmet.step().label(),
            ),
            Self::Lock { path, source } => write!(
                f,
                "failed to lock the client setup at {}: {source}",
                path.display(),
            ),
        }
    }
}

impl fmt::Debug for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for ClientError {}
