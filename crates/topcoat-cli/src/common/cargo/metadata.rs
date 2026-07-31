use std::path::{Path, PathBuf};
use std::process::Stdio;

use tokio::process::Command;

/// The output of `cargo metadata` for the current workspace.
pub struct Metadata(serde_json::Value);

impl Metadata {
    /// Query the workspace's own metadata (`cargo metadata --no-deps`).
    /// Returns `None` when cargo cannot be spawned or reports an error.
    pub async fn workspace() -> Option<Self> {
        Self::run(&["--no-deps"]).await
    }

    /// Query metadata with the dependency graph resolved, so the output also
    /// lists path dependencies living outside the workspace. Returns `None`
    /// when cargo cannot be spawned or reports an error.
    pub async fn full() -> Option<Self> {
        Self::run(&[]).await
    }

    async fn run(extra_args: &[&str]) -> Option<Self> {
        let output = Command::new("cargo")
            .args(["metadata", "--format-version=1"])
            .args(extra_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .ok()?
            .wait_with_output()
            .await
            .ok()?;

        if !output.status.success() {
            return None;
        }

        serde_json::from_slice(&output.stdout).ok().map(Self)
    }

    /// The workspace's cargo target directory.
    pub fn target_dir(&self) -> Option<PathBuf> {
        self.0["target_directory"].as_str().map(PathBuf::from)
    }

    /// The workspace root directory, which holds the root manifest and
    /// lockfile; in a virtual workspace it is not a package of its own.
    pub fn workspace_root(&self) -> Option<PathBuf> {
        self.0["workspace_root"].as_str().map(PathBuf::from)
    }

    /// The manifest directory of every local package: a package without a
    /// `source` is local -- a workspace member or a path dependency, wherever
    /// it lives on disk.
    pub fn local_package_dirs(&self) -> Vec<PathBuf> {
        self.0["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|package| package["source"].is_null())
            .filter_map(|package| {
                let manifest = Path::new(package["manifest_path"].as_str()?);
                Some(manifest.parent()?.to_path_buf())
            })
            .collect()
    }

    /// Every package the query returned, in the order cargo listed them.
    #[cfg(feature = "client")]
    pub fn packages(&self) -> impl Iterator<Item = Package<'_>> {
        self.0["packages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(Package)
    }
}

/// One package of a [`Metadata`] query.
//
// Only the client build reads a package's own fields; the rest of the CLI works
// off the whole query.
#[cfg(feature = "client")]
#[derive(Clone, Copy)]
pub struct Package<'a>(pub(crate) &'a serde_json::Value);

#[cfg(feature = "client")]
impl Package<'_> {
    /// The package name.
    pub fn name(&self) -> &str {
        self.0["name"].as_str().unwrap_or_default()
    }

    /// The directory holding the package's manifest.
    pub fn manifest_dir(&self) -> Option<&Path> {
        Path::new(self.0["manifest_path"].as_str()?).parent()
    }

    /// The `[package.metadata]` table, which cargo passes through untouched.
    pub fn metadata(&self) -> &serde_json::Value {
        &self.0["metadata"]
    }

    /// Where the build dependency named `name` lives, when the package has it
    /// as a path dependency.
    pub fn build_dependency_dir(&self, name: &str) -> Option<&Path> {
        self.0["dependencies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|dependency| dependency["kind"] == "build")
            .filter(|dependency| dependency["name"] == name)
            .find_map(|dependency| Some(Path::new(dependency["path"].as_str()?)))
    }
}
