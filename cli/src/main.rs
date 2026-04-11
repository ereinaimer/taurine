// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
pub mod commands;
pub mod service;

use clap::{Parser, Subcommand};
use tracing::{error, info};

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine")]
#[command(about = "Fast, secure and easy to use text expander and keyboard automation tool")]
struct Cli {
    /// Increase console verbosity
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Disable console logging output
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    /// Disable writing logs to the log file
    #[arg(long, global = true)]
    no_log_file: bool,

    /// Disable tracking colors in console output
    #[arg(long, global = true)]
    no_color: bool,

    /// Internal flag used by the OS service manager (DO NOT RUN MANUALLY)
    #[arg(long, hide = true)]
    daemon: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start Taurine
    #[command(alias = "start")]
    Up,
    /// Stop Taurine
    #[command(alias = "stop")]
    Down,
    /// Restart Taurine
    #[command(alias = "reboot")]
    Restart,
    /// Check if Taurine is currently running
    Status,
    /// Add a new automation
    #[command(alias = "set")]
    Add { trigger: String, output: String },
    /// Remove an existing automation by trigger
    #[command(aliases = ["rm", "remove"])]
    Delete { trigger: String },
    /// List all automations
    #[command(alias = "ls")]
    List,
    /// Manage application settings
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Set a configuration value
    Set { key: String, value: String },
    /// List all current configuration values
    #[command(alias = "ls")]
    List,
    /// Reset a configuration value to default
    Reset { key: String },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    let component = if cli.daemon {
        taurine_core::logs::LogComponent::Daemon
    } else {
        taurine_core::logs::LogComponent::Cli
    };

    let _guard = taurine_core::logs::init_tracing_for_app(
        cli.verbose,
        cli.quiet,
        cli.no_log_file,
        cli.no_color,
        component,
    );

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

fn run(cli: Cli) -> taurine_core::error::Result<()> {
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
        Some(Commands::Up) | Some(Commands::Restart) => {
            // Open the DB (idempotent: runs migrations + seeds if needed) and
            // read the user's start_on_boot preference before handing off to
            // the platform service layer.
            let start_on_boot = {
                use taurine_core::db::init;
                use taurine_core::settings::SettingsManager;
                let conn = init::setup()?;

                let settings_manager = SettingsManager::new(&conn);
                settings_manager.load_all().start_on_boot
            };

            if matches!(cli.command, Some(Commands::Restart)) {
                service::restart(start_on_boot)?;
            } else {
                service::up(start_on_boot)?;
            }
        }
        Some(Commands::Down) => service::down()?,
        Some(Commands::Status) => service::status()?,
        Some(Commands::Add { trigger, output }) => {
            commands::add::execute(trigger, output)?;
        }
        Some(Commands::Delete { trigger }) => {
            commands::delete::execute(trigger)?;
        }
        Some(Commands::List) => {
            commands::list::execute()?;
        }
        Some(Commands::Config { action }) => match action {
            ConfigAction::Set { key, value } => commands::config::execute_set(key, value)?,
            ConfigAction::List => commands::config::execute_list()?,
            ConfigAction::Reset { key } => commands::config::execute_reset(key)?,
        },
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
