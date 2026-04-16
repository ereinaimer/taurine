// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
pub mod commands;
pub mod service;

use clap::{Parser, Subcommand, ValueEnum};
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

    /// Disable showing colors in console output
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
    Add(AddArgs),
    /// Remove an existing automation by trigger
    #[command(aliases = ["rm", "remove"])]
    Delete { trigger: String },
    /// List all automations
    #[command(alias = "ls")]
    List {
        /// Sort the list by a specific criteria
        #[arg(long, value_enum, hide_possible_values = true)]
        sort: Option<SortBy>,

        /// Sort in ascending order
        #[arg(long, conflicts_with = "desc")]
        asc: bool,

        /// Sort in descending order
        #[arg(long, conflicts_with = "asc")]
        desc: bool,

        /// Disable table decorations and borders
        #[arg(long)]
        plain: bool,
    },
    /// Export automations to a file
    Export {
        /// Destination file path
        path: std::path::PathBuf,
        /// Write a plaintext export without encryption
        #[arg(long)]
        no_encrypt: bool,
        /// Include settings in the exported payload
        #[arg(long)]
        with_settings: bool,
        /// Include automation usage stats and daily metrics in the exported payload
        #[arg(long)]
        with_metrics: bool,
    },
    /// Import automations from a file
    Import {
        /// Source file path
        path: std::path::PathBuf,
        /// How to resolve trigger + target_os collisions during import
        #[arg(long, value_enum)]
        on_conflict: Option<ImportConflictCli>,
        /// Overwrite local settings with imported values
        #[arg(long)]
        include_settings: bool,
        /// Include imported metrics. Omit the flag to ignore metrics entirely; `--include-metrics` alone uses `merge`.
        #[arg(long, value_enum, num_args = 0..=1, require_equals = true, default_missing_value = "merge")]
        include_metrics: Option<ImportMetricsCli>,
    },
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
    /// Reset a configuration value to default. Use --all to reset everything.
    Reset {
        /// The setting key to reset
        key: Option<String>,
        /// Reset all settings to factory defaults
        #[arg(long)]
        all: bool,
    },
}

#[derive(Parser, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct AddArgs {
    #[command(subcommand)]
    pub sub: Option<AddSubcommand>,

    /// Trigger for standard text expansion
    pub trigger: Option<String>,
    /// Output for standard text expansion
    pub output: Option<String>,
    /// The target operating system (windows, linux, macos, all, android, ios)
    #[arg(long, value_enum, default_value = "all")]
    pub os: TargetOsCli,
}

#[derive(Subcommand, Debug)]
pub enum AddSubcommand {
    /// Add a shell script automation
    Script {
        /// The trigger string
        trigger: String,
        /// The script content (optional if --file is used)
        #[arg(required_unless_present = "file")]
        content: Option<String>,
        /// Path to the script file
        #[arg(short, long)]
        file: Option<std::path::PathBuf>,
        /// Interpreter to use (bash, powershell, python, cmd)
        #[arg(
            short = 'l',
            long = "lang",
            value_enum,
            required_unless_present = "file"
        )]
        lang: Option<ScriptInterpreterCli>,
        /// Execution mode (inline, silent)
        #[arg(short = 'm', long = "mode", value_enum, default_value = "inline")]
        mode: ScriptBehaviorCli,
        /// The target operating system (windows, linux, macos, all, android, ios)
        #[arg(long, value_enum, default_value = "current")]
        os: TargetOsCli,
    },
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum TargetOsCli {
    Windows,
    Linux,
    Macos,
    All,
    Android,
    Ios,
    Current,
}

impl TargetOsCli {
    fn to_db_str(&self) -> Option<&'static str> {
        match self {
            Self::Windows => Some("win"),
            Self::Macos => Some("mac"),
            Self::Linux => Some("linux"),
            Self::All => Some("all"),
            Self::Android => Some("android"),
            Self::Ios => Some("ios"),
            Self::Current => None,
        }
    }
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum ScriptInterpreterCli {
    Bash,
    Powershell,
    Python,
    Cmd,
}

impl From<ScriptInterpreterCli> for taurine_core::engine::shell::ScriptInterpreter {
    fn from(val: ScriptInterpreterCli) -> Self {
        match val {
            ScriptInterpreterCli::Bash => Self::Bash,
            ScriptInterpreterCli::Powershell => Self::PowerShell,
            ScriptInterpreterCli::Python => Self::Python,
            ScriptInterpreterCli::Cmd => Self::Cmd,
        }
    }
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum ScriptBehaviorCli {
    Inline,
    Silent,
}

impl From<ScriptBehaviorCli> for taurine_core::engine::shell::ScriptBehavior {
    fn from(val: ScriptBehaviorCli) -> Self {
        match val {
            ScriptBehaviorCli::Inline => Self::Inline,
            ScriptBehaviorCli::Silent => Self::Silent,
        }
    }
}

#[derive(ValueEnum, Clone, Debug, PartialEq)]
pub enum SortBy {
    /// Sort by trigger alphabetically
    Alpha,
    /// Sort by usage count
    Usage,
    /// Sort by creation date
    Created,
    /// Sort by last used date
    Recent,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportConflictCli {
    Prompt,
    Skip,
    Overwrite,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMetricsCli {
    Ignore,
    Merge,
    Overwrite,
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
        Some(Commands::Add(args)) => {
            if let Some(AddSubcommand::Script {
                trigger,
                content,
                file,
                lang,
                mode,
                os,
            }) = args.sub
            {
                commands::script::execute(
                    trigger,
                    content,
                    file,
                    lang.map(Into::into),
                    mode.into(),
                    os.to_db_str().map(|s| s.to_string()).unwrap_or_else(|| {
                        taurine_core::db::get_current_os_db_string().to_string()
                    }),
                )?;
            } else if let (Some(t), Some(o)) = (args.trigger, args.output) {
                let os = args
                    .os
                    .to_db_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| taurine_core::db::get_current_os_db_string().to_string());
                commands::add::execute(t, o, os)?;
            } else {
                // Show help for add command if neither subcommand nor positional args are valid
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                if let Some(add_cmd) = cmd.get_subcommands_mut().find(|c| c.get_name() == "add") {
                    add_cmd.print_help()?;
                }
            }
        }
        Some(Commands::Delete { trigger }) => {
            commands::delete::execute(trigger)?;
        }
        Some(Commands::List {
            sort,
            asc,
            desc,
            plain,
        }) => {
            commands::list::execute(sort, asc, desc, plain)?;
        }
        Some(Commands::Export {
            path,
            no_encrypt,
            with_settings,
            with_metrics,
        }) => {
            commands::export::execute(path, no_encrypt, with_settings, with_metrics)?;
        }
        Some(Commands::Import {
            path,
            on_conflict,
            include_settings,
            include_metrics,
        }) => {
            commands::import::execute(path, on_conflict, include_settings, include_metrics)?;
        }
        Some(Commands::Config { action }) => match action {
            ConfigAction::Set { key, value } => commands::config::execute_set(key, value)?,
            ConfigAction::List => commands::config::execute_list()?,
            ConfigAction::Reset { key, all } => {
                if all {
                    commands::config::execute_reset_all()?;
                } else if let Some(k) = key {
                    commands::config::execute_reset(k)?;
                } else {
                    eprintln!("error: provide a key to reset or use --all to reset everything");
                    std::process::exit(1);
                }
            }
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
