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
        tracing_subscriber::fmt()
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

    let db_path = paths::get_db_path();

    // Only initialize if the database file does not exist
    if !db_path.exists() {
        info!("Database not found. Initializing Taurine database at {}", db_path.display());
        
        if let Err(e) = db::init_db() {
            error!("Failed to initialize database: {}", e);
            std::process::exit(1);
        }
    } else {
        debug!("Database already exists at {}. Skipping initialization.", db_path.display());
    }
}