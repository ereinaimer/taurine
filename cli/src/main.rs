// Licensed under the Aimer Software License (ASL). See LICENSE for details.

use clap::Parser;
use taurine_core::db;
use tracing::{debug, error, info};
use tracing_subscriber::EnvFilter;

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine")]
#[command(about = "Fast, secure and easy to use text expander and keyboard automation tool")]
struct Cli {
    /// Enable verbose logging (includes timestamps, module paths, and debug logs)
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

    info!("Initializing Taurine database...");
    
    match db::init_db() {
        Ok(_) => {
            let path = taurine_core::paths::get_db_path();
            info!("Database initialized successfully");
            debug!("Database path: {}", path.display());
        }
        Err(e) => {
            error!("Failed to initialize database: {}", e);
            std::process::exit(1);
        }
    }
}
