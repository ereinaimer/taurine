// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
use clap::Parser;
use taurine_core::{db, paths};
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine")]
#[command(about = "Fast, secure and easy to use text expander and keyboard automation tool")]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    let cli = Cli::parse();

    let default_level = if cli.verbose { "debug" } else { "info" };
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level));

    if cli.verbose {
        let timer = tracing_subscriber::fmt::time::LocalTime::new(
            time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
        );

        tracing_subscriber::fmt()
            .with_timer(timer)
            .with_env_filter(env_filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .compact()
            .with_target(false)
            .with_file(false)
            .with_line_number(false)
            .without_time()
            .with_env_filter(env_filter)
            .init();
    }

    if let Err(e) = run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    info!("Initializing Taurine v{}", env!("CARGO_PKG_VERSION"));

    let db_path = paths::get_db_path();
    let is_new = !db_path.exists();

    debug!("Database path: {}", db_path.display());

    let conn = db::init_db().map_err(|e| {
        error!("Failed to open database: {}", e);
        e
    })?;

    if is_new {
        info!("New database created at {}", db_path.display());
    }

    // Run schema migrations — safe to call every startup, already-applied
    // migrations are no-ops tracked by PRAGMA user_version.
    db::run_migrations(&conn).map_err(|e| {
        error!("Schema migration failed: {}", e);
        e
    })?;

    Ok(())
}