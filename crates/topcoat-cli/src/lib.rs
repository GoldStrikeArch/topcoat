#![cfg_attr(docsrs, feature(doc_cfg))]

mod asset;
#[cfg(feature = "client")]
mod client;
mod common;
mod dev;
mod fmt;
mod ui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "topcoat")]
pub struct TopcoatCli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start a development server
    Dev(dev::DevCommand),
    /// Format topcoat `view!` macros
    Fmt(fmt::FmtCommand),
    /// Inspect assets embedded in the binary
    Asset(asset::AssetCommand),
    /// Manage premade UI components in your project
    Ui(ui::UiCommand),
    /// Build the toolchain client crates are compiled to JavaScript with
    #[cfg(feature = "client")]
    Client(client::ClientCommand),
}

pub async fn run() {
    let cli = TopcoatCli::parse();
    match cli.command {
        Command::Ui(cmd) => cmd.run(),
        Command::Fmt(cmd) => cmd.run().await,
        Command::Dev(cmd) => cmd.run().await,
        Command::Asset(cmd) => cmd.run().await,
        #[cfg(feature = "client")]
        Command::Client(cmd) => cmd.run().await,
    }
}
