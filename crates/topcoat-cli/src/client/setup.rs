use clap::Args;
use console::style;

use super::{ClientPackage, Rebuild, ensure};

#[derive(Args)]
pub struct SetupArgs {
    /// Prepare only the named package
    #[arg(short, long)]
    package: Option<String>,
    /// Rebuild the codegen backend and the view macros even when they are built
    #[arg(long)]
    rebuild_backend: bool,
    /// Rebuild the sysroot even when it is built
    #[arg(long)]
    rebuild_sysroot: bool,
}

/// Prepare every client crate in the workspace, reporting what it took.
pub async fn run(args: SetupArgs) {
    let packages = ClientPackage::discover(args.package.as_deref()).await;
    if packages.is_empty() {
        eprintln!();
        eprintln!(
            "  {}",
            style(match &args.package {
                Some(package) => format!("{package} compiles no client crate"),
                None => "no package in this workspace compiles a client crate".to_owned(),
            })
            .dim()
        );
        eprintln!(
            "  {}",
            style("a package that does has `jsc-build` as a build dependency").dim()
        );
        eprintln!();
        return;
    }

    let rebuild = Rebuild {
        backend: args.rebuild_backend,
        sysroot: args.rebuild_sysroot,
    };

    eprintln!();
    for package in &packages {
        match ensure(package, rebuild).await {
            Ok(report) => report.print(),
            Err(error) => error.print_and_exit(),
        }
    }
    eprintln!();
}
