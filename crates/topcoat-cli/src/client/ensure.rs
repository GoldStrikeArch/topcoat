use std::{fs, path::Path};

use console::style;
use jsc_build::{Step, Toolchain};

use super::{ClientError, ClientPackage};

/// The scope of the client build's lock inside the shared Topcoat cache.
const CACHE_SCOPE: &str = "client";
/// The lock file serializing setup runs against one checkout.
const LOCK_NAME: &str = "setup.lock";

/// The steps to perform again even though they are already done.
///
/// Whether a built artifact is stale cannot be told from the outside: the
/// backend's sources are a whole rustc plugin, and the sysroot is `core`
/// compiled by it. So the setup performs the steps that are *missing*, and
/// redoing a step that is merely out of date is asked for explicitly.
#[derive(Clone, Copy, Default)]
pub struct Rebuild {
    /// Rebuild the backend dylib, and with it the view macros: both are built
    /// from the checkout's own sources and go stale together.
    pub backend: bool,
    /// Rebuild the sysroot.
    pub sysroot: bool,
}

impl Rebuild {
    /// Whether `step` is to be performed even when it is already done.
    fn wants(self, step: Step) -> bool {
        match step {
            Step::Backend | Step::ViewMacros => self.backend,
            Step::Sysroot => self.sysroot,
            // Installing a toolchain that is already installed is `rustup`'s
            // business, and asking for it is `rustup update`.
            Step::Toolchain => false,
        }
    }
}

/// What one run of the setup performed.
pub struct Report {
    package: String,
    performed: Vec<Step>,
}

impl Report {
    /// The steps it performed, in the order it performed them. Empty means the
    /// checkout was already ready.
    pub fn performed(&self) -> &[Step] {
        &self.performed
    }

    /// The steps it performed, as a list to print.
    pub fn summary(&self) -> String {
        if self.performed.is_empty() {
            return "already ready".to_owned();
        }
        self.performed
            .iter()
            .map(|step| step.label())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Report what the setup did, on the line the steps announced themselves on.
    pub fn print(&self) {
        eprintln!(
            "  {} {} {}",
            style("client").cyan().bold(),
            style(format!("[{}]", self.package)).dim(),
            style(self.summary()).green()
        );
    }
}

/// Perform every setup step `package` needs before its client crate can be
/// compiled, and nothing else.
///
/// The steps live in the spike checkout, not in the project: the pinned
/// nightly, the codegen backend built with it, the `core` sysroot the backend
/// compiled, and the view macros. They are what lets the project itself be
/// built with any toolchain, since each one runs in a child process that names
/// the nightly it needs.
///
/// Progress is written to stderr as it happens, because a step takes minutes
/// the first time and a command that looks hung is worse than a noisy one.
///
/// # Errors
///
/// Returns `Err` if the checkout cannot be located or used, if a step fails, or
/// if a step leaves its precondition unmet.
pub async fn ensure(package: &ClientPackage, rebuild: Rebuild) -> Result<Report, ClientError> {
    let root = package
        .spike_root()
        .ok_or_else(|| ClientError::NoSpikeRoot(package.name().to_owned()))?
        .to_path_buf();
    let name = package.name().to_owned();
    let emit_args = package.emit_args();

    // The steps are child processes that take minutes and write to this
    // process's stderr, so they run on a blocking thread rather than in the
    // async runtime the dev server's event loop lives in.
    tokio::task::spawn_blocking(move || perform(name, &root, &emit_args, rebuild))
        .await
        .expect("client setup task panicked")
}

/// Run the setup, holding the checkout's lock for as long as it takes.
fn perform(
    package: String,
    root: &Path,
    emit_args: &str,
    rebuild: Rebuild,
) -> Result<Report, ClientError> {
    let toolchain = Toolchain::new(root).map_err(ClientError::Build)?;
    let lock = Lock::acquire(&toolchain)?;

    // Under the lock: another process may have finished the whole setup while
    // this one waited for it.
    let planned = plan(&toolchain, emit_args, rebuild);
    let mut performed = Vec::new();
    for (step, reason) in planned {
        announce(&package, step, reason.as_deref());
        let fix = toolchain.fix(step, emit_args);
        toolchain
            .apply(&fix)
            .map_err(|source| ClientError::step(&fix, source))?;
        performed.push(step);
    }

    // The steps are shell scripts; a script that exits zero without producing
    // what it promised would otherwise surface much later, as a compile error
    // inside the project's build script.
    if let Some(unmet) = toolchain.preconditions(emit_args, true).into_iter().next() {
        return Err(ClientError::StillUnmet(Box::new(unmet)));
    }
    drop(lock);

    Ok(Report { package, performed })
}

/// The steps to perform, in the order they have to run: everything the checkout
/// is missing, plus anything [`Rebuild`] asks for again.
fn plan(toolchain: &Toolchain, emit_args: &str, rebuild: Rebuild) -> Vec<(Step, Option<String>)> {
    let unmet = toolchain.preconditions(emit_args, true);
    Step::ALL
        .into_iter()
        .filter_map(|step| {
            let missing = unmet.iter().find(|unmet| unmet.step() == step);
            let asked_for = missing.is_some() || rebuild.wants(step);
            asked_for.then(|| (step, missing.map(|unmet| unmet.reason().to_owned())))
        })
        .collect()
}

/// Report a step that is about to run, and why it is needed.
fn announce(package: &str, step: Step, reason: Option<&str>) {
    let reason = reason.unwrap_or("rebuilding on request");
    eprintln!(
        "  {} {} {}",
        style("client").cyan().bold(),
        style(format!("[{package}]")).dim(),
        style(format!("{}: {reason}", step.label())).yellow()
    );
}

/// An exclusive lock on one spike checkout's setup.
///
/// Two projects can share a checkout, and both can be asked to prepare it at
/// once. Its steps are not safe to run twice at the same time: they build into
/// one target directory and copy a patched standard library into one place. The
/// lock lets the first run proceed while the rest wait and find the work done.
///
/// It lives beside the artifacts it guards, under the checkout's own target
/// directory, so every project reaching for that checkout takes the same lock.
/// The file is never removed: locking acts on the file's inode, so unlinking it
/// would let a waiter and a newcomer that recreates the path lock two different
/// inodes and both proceed. The operating system drops the lock if a holder
/// dies, so a crash cannot wedge later runs.
struct Lock {
    file: fs::File,
}

impl Lock {
    /// Take the lock, waiting for another holder to release it.
    fn acquire(toolchain: &Toolchain) -> Result<Self, ClientError> {
        let dir = topcoat_core::cache::cache_dir_in(toolchain.root().join("target"), CACHE_SCOPE);
        fs::create_dir_all(&dir).map_err(|source| ClientError::lock(&dir, source))?;

        let path = dir.join(LOCK_NAME);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| ClientError::lock(&path, source))?;
        file.lock()
            .map_err(|source| ClientError::lock(&path, source))?;
        Ok(Self { file })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_run_asks_for_nothing_extra() {
        let rebuild = Rebuild::default();

        assert!(Step::ALL.into_iter().all(|step| !rebuild.wants(step)));
    }

    #[test]
    fn rebuilding_the_backend_rebuilds_the_view_macros_with_it() {
        let rebuild = Rebuild {
            backend: true,
            sysroot: false,
        };

        assert!(rebuild.wants(Step::Backend));
        assert!(rebuild.wants(Step::ViewMacros));
        assert!(!rebuild.wants(Step::Sysroot));
        assert!(!rebuild.wants(Step::Toolchain));
    }

    #[test]
    fn rebuilding_the_sysroot_leaves_the_backend_alone() {
        let rebuild = Rebuild {
            backend: false,
            sysroot: true,
        };

        assert!(rebuild.wants(Step::Sysroot));
        assert!(!rebuild.wants(Step::Backend));
    }

    #[test]
    fn a_report_of_nothing_says_the_checkout_was_ready() {
        let report = Report {
            package: "app".to_owned(),
            performed: Vec::new(),
        };

        assert!(report.performed().is_empty());
        assert_eq!(report.summary(), "already ready");
    }

    #[test]
    fn a_report_lists_what_it_performed_in_order() {
        let report = Report {
            package: "app".to_owned(),
            performed: vec![Step::Backend, Step::Sysroot],
        };

        assert_eq!(report.summary(), "backend, sysroot");
    }
}
