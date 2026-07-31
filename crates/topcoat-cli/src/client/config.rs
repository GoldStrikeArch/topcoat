use std::{
    env,
    path::{Path, PathBuf},
};

use jsc_build::SPIKE_ROOT_ENV;

use crate::common::cargo::{Metadata, Package};

/// The build dependency that compiles a client crate. A package that has it is
/// a package the client build phase applies to.
const DRIVER: &str = "jsc-build";
/// The `[package.metadata.topcoat]` key holding the client build settings.
const TABLE: &str = "client";
/// The key naming the spike checkout inside that table.
const SPIKE_ROOT_KEY: &str = "spike-root";
/// The key holding the backend's emit-affecting options inside that table.
const EMIT_ARGS_KEY: &str = "emit-args";

/// A package that compiles a client crate to JavaScript.
///
/// Nothing has to be declared for the common case: a package that has
/// `jsc-build` as a build dependency compiles a client crate, and the resolved
/// path of that dependency says which spike checkout it compiles it with. The
/// `[package.metadata.topcoat.client]` table is for the cases that cannot be
/// inferred:
///
/// ```toml
/// [package.metadata.topcoat.client]
/// # The spike checkout, relative to this manifest. Needed only when the
/// # `jsc-build` build dependency does not come from it.
/// spike-root = "../rustc-codegen-js"
/// # The emit-affecting backend options the build script passes to
/// # `BuildConfig::emit_arg`. The sysroot has to be built with the same set,
/// # so the setup has to know them. Options passed to `crate_arg` change only
/// # one crate's output and do not belong here.
/// emit-args = ["js-minify=on"]
/// ```
///
/// [`SPIKE_ROOT_ENV`] overrides the checkout for both, the same way it
/// overrides it for the build script itself.
#[derive(Debug, Clone)]
pub struct ClientPackage {
    name: String,
    spike_root: Option<PathBuf>,
    emit_args: Vec<String>,
}

impl ClientPackage {
    /// Every workspace package that compiles a client crate, or only the one
    /// named by `package`.
    ///
    /// An empty list means no package does, which is the answer for every
    /// project that has no client crate.
    pub async fn discover(package: Option<&str>) -> Vec<Self> {
        let Some(metadata) = Metadata::workspace().await else {
            return Vec::new();
        };
        // The environment overrides every manifest at once: it is set to point
        // a whole project at another checkout.
        let overridden = env::var_os(SPIKE_ROOT_ENV).map(PathBuf::from);
        metadata
            .packages()
            .filter(|found| package.is_none_or(|name| found.name() == name))
            .filter_map(Self::read)
            .map(|mut found| {
                if overridden.is_some() {
                    found.spike_root.clone_from(&overridden);
                }
                found
            })
            .collect()
    }

    /// The client build settings of `package`, or `None` when it compiles no
    /// client crate.
    fn read(package: Package<'_>) -> Option<Self> {
        let declared = package
            .metadata()
            .get("topcoat")
            .and_then(|topcoat| topcoat.get(TABLE));
        let driver = package.build_dependency_dir(DRIVER);
        if declared.is_none() && driver.is_none() {
            return None;
        }
        // A package can say `client = true` to opt in without settings.
        let table = declared.filter(|table| table.is_object());

        let configured = table
            .and_then(|table| table.get(SPIKE_ROOT_KEY)?.as_str())
            .zip(package.manifest_dir())
            .map(|(root, manifest_dir)| manifest_dir.join(root));
        let emit_args = table
            .and_then(|table| table.get(EMIT_ARGS_KEY)?.as_array())
            .into_iter()
            .flatten()
            .filter_map(|arg| Some(arg.as_str()?.to_owned()))
            .collect();

        Some(Self {
            name: package.name().to_owned(),
            spike_root: spike_root(configured, driver),
            emit_args,
        })
    }

    /// The package name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The spike checkout the client crate is compiled with, or `None` when
    /// nothing says where it is.
    pub fn spike_root(&self) -> Option<&Path> {
        self.spike_root.as_deref()
    }

    /// The emit-affecting backend options, as the scripts take them.
    pub fn emit_args(&self) -> String {
        self.emit_args.join(" ")
    }
}

/// The spike checkout to use: what the manifest declares, else the checkout the
/// build dependency comes from.
fn spike_root(configured: Option<PathBuf>, driver: Option<&Path>) -> Option<PathBuf> {
    configured.or_else(|| Some(driver?.parent()?.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `cargo metadata` package with `fields` spliced into it.
    fn package(fields: &str) -> serde_json::Value {
        serde_json::from_str(&format!(
            r#"{{
                "name": "app",
                "manifest_path": "/repo/app/Cargo.toml",
                "metadata": null,
                "dependencies": [],
                {fields}
                "id": "app"
            }}"#
        ))
        .unwrap()
    }

    #[test]
    fn a_package_without_the_driver_compiles_no_client_crate() {
        let value = package(
            r#""dependencies": [
                { "name": "tokio", "kind": null, "path": null }
            ],"#,
        );

        assert!(ClientPackage::read(Package(&value)).is_none());
    }

    #[test]
    fn a_normal_dependency_on_the_driver_is_not_the_trigger() {
        let value = package(
            r#""dependencies": [
                { "name": "jsc-build", "kind": null, "path": "/spike/jsc-build" }
            ],"#,
        );

        assert!(ClientPackage::read(Package(&value)).is_none());
    }

    #[test]
    fn the_checkout_is_the_one_the_build_dependency_comes_from() {
        let value = package(
            r#""dependencies": [
                { "name": "jsc-build", "kind": "build", "path": "/spike/jsc-build" }
            ],"#,
        );

        let found = ClientPackage::read(Package(&value)).unwrap();

        assert_eq!(found.name(), "app");
        assert_eq!(found.spike_root(), Some(Path::new("/spike")));
        assert_eq!(found.emit_args(), "");
    }

    #[test]
    fn the_manifest_can_name_the_checkout_and_the_emit_options() {
        let value = package(
            r#""metadata": {
                "topcoat": {
                    "client": {
                        "spike-root": "../spike",
                        "emit-args": ["js-minify=on", "js-queue=off"]
                    }
                }
            },"#,
        );

        let found = ClientPackage::read(Package(&value)).unwrap();

        assert_eq!(found.spike_root(), Some(Path::new("/repo/app/../spike")));
        assert_eq!(found.emit_args(), "js-minify=on js-queue=off");
    }

    #[test]
    fn a_declared_table_wins_over_the_build_dependency() {
        let value = package(
            r#""metadata": { "topcoat": { "client": { "spike-root": "/elsewhere" } } },
            "dependencies": [
                { "name": "jsc-build", "kind": "build", "path": "/spike/jsc-build" }
            ],"#,
        );

        let found = ClientPackage::read(Package(&value)).unwrap();

        assert_eq!(found.spike_root(), Some(Path::new("/elsewhere")));
    }

    #[test]
    fn a_declared_table_with_no_checkout_leaves_it_unresolved() {
        let value = package(r#""metadata": { "topcoat": { "client": {} } },"#);

        let found = ClientPackage::read(Package(&value)).unwrap();

        assert_eq!(found.name(), "app");
        assert_eq!(found.spike_root(), None);
    }
}
