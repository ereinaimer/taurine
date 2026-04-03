// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
pub mod service;

use clap::{Parser, Subcommand};
use tracing::{error, info};

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine")]
#[command(about = "Fast, secure and easy to use text expander and keyboard automation tool")]
struct Cli {
    /// Increase console verbosity
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Disable console logging output
    #[arg(short, long, conflicts_with = "verbose")]
    quiet: bool,

    /// Disable writing logs to the log file
    #[arg(long)]
    no_log_file: bool,

    /// Internal flag used by the OS service manager (DO NOT RUN MANUALLY)
    #[arg(long, hide = true)]
    daemon: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start Taurine
    Up,
    /// Stop Taurine
    Down,
    /// Check if Taurine is currently running
    Status,
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    let _guard = taurine_core::logs::init_tracing_for_app(cli.verbose, cli.quiet, cli.no_log_file);

    // Install a panic hook that:
    // 1) writes structured diagnostics into tracing + daily log file
    // 2) prints the human-friendly color-eyre report.
    let (panic_hook, _eyre_hook) = color_eyre::config::HookBuilder::new().into_hooks();
    let color_eyre_panic = panic_hook.into_panic_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        taurine_core::logs::handle_panic_info(panic_info);
        color_eyre_panic(panic_info);
    }));

    if let Err(e) = run(cli) {
        error!(error=%e, "Taurine exited with an error");
        return std::process::ExitCode::from(1);
    }

    std::process::ExitCode::SUCCESS
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    if cli.daemon {
        info!("Initializing Taurine v{}", env!("CARGO_PKG_VERSION"));

        // Execute the startup sequence (database init, seed, etc.)
        taurine_daemon::start()?;

        info!("Taurine is alive. Idling...");

        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
        }
    }

    match cli.command {
        Some(Commands::Up) => service::up()?,
        Some(Commands::Down) => service::down()?,
        Some(Commands::Status) => service::status()?,
        None => {
            if !cli.daemon {
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                cmd.print_help()?;
            }
        }
    }

    Ok(())
}
