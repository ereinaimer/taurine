// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
use clap::Parser;
use taurine_core::{db, paths};
use tracing::{debug, error, info};

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine")]
#[command(about = "Fast, secure and easy to use text expander and keyboard automation tool")]
struct Cli {
    /// Increase console verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Run silently (disable console logging)
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    taurine_core::logs::init_tracing_for_app(cli.verbose, cli.quiet);

    // Install a panic hook that:
    // 1) writes structured diagnostics into tracing + daily log file
    // 2) prints the human-friendly color-eyre report.
    let (panic_hook, _eyre_hook) = color_eyre::config::HookBuilder::new().into_hooks();
    let color_eyre_panic = panic_hook.into_panic_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        taurine_core::logs::handle_panic_info(panic_info);
        color_eyre_panic(panic_info);
    }));

    if let Err(e) = run() {
        error!(error=%e, "Taurine exited with an error");
        return std::process::ExitCode::from(1);
    }

    std::process::ExitCode::SUCCESS
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    info!("Initializing Taurine v{}", env!("CARGO_PKG_VERSION"));

    let db_path = paths::get_db_path();
    let is_new = !db_path.exists();

    debug!("Database path: {}", db_path.display());

    let conn = db::init_db().map_err(|e| {
        error!(error=%e, "Failed to open database");
        e
    })?;

    if is_new {
        info!("New database created at {}", db_path.display());
    }

    // Run schema migrations — safe to call every startup, already-applied
    // migrations are no-ops tracked by PRAGMA user_version.
    db::run_migrations(&conn).map_err(|e| {
        error!(error=%e, "Schema migration failed");
        e
    })?;

    Ok(())
}