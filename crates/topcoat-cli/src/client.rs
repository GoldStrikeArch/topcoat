#![doc = include_str!("../docs/client.md")]
//!
//! # How it is put together
//!
//! - [`config`]: which packages compile a client crate, and with which checkout.
//! - [`ensure`]: performing the steps that are missing, under a lock.
//! - [`setup`]: the `topcoat client setup` command.
//!
//! The steps themselves are not defined here. `jsc-build` decides what a ready
//! checkout looks like and what command makes it ready, and this module runs
//! that list, so the CLI and a build script can never disagree about it.

mod config;
mod ensure;
mod error;
mod setup;

pub use config::*;
pub use ensure::*;
pub use error::*;
pub use setup::*;

use clap::{Args, Subcommand};
use console::style;

#[derive(Args)]
pub struct ClientCommand {
    #[command(subcommand)]
    command: ClientSubcommand,
}

#[derive(Subcommand)]
enum ClientSubcommand {
    /// Build the toolchain a client crate is compiled with, if it is not built
    Setup(SetupArgs),
}

impl ClientCommand {
    pub async fn run(self) {
        match self.command {
            ClientSubcommand::Setup(args) => setup::run(args).await,
        }
    }
}

/// Prepare the client crates of the workspace, or of `package` alone, before
/// the application is built.
///
/// Reports what it did to the terminal and answers whether the build can go
/// ahead. A workspace with no client crate does nothing and costs one
/// `cargo metadata` query.
pub async fn prepare(package: Option<&str>) -> bool {
    for package in ClientPackage::discover(package).await {
        match ensure(&package, Rebuild::default()).await {
            // A checkout that was already ready is the normal case, and saying
            // so on every rebuild would be noise.
            Ok(report) if report.performed().is_empty() => {}
            Ok(report) => {
                report.print();
                eprintln!();
            }
            Err(error) => {
                eprintln!(
                    "  {}",
                    style(format!("client setup failed: {error}")).red().bold()
                );
                eprintln!();
                return false;
            }
        }
    }
    true
}
