// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
pub mod commands;

use clap::{Parser, Subcommand, ValueEnum};
use tracing::{error, info};

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine", version = env!("CARGO_PKG_VERSION"), disable_version_flag = true)]
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

    /// Show log level prefixes (INFO, DEBUG, WARN) in console output
    #[arg(long, global = true)]
    show_log_prefixes: bool,

    /// Print version information and exit
    #[arg(long, global = true)]
    version: bool,

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
    /// Update Taurine to the latest version
    Update,
    /// Check if Taurine is currently running
    Status,
    /// Add a new automation
    #[command(alias = "set")]
    Add(AddArgs),
    /// Remove an existing automation by trigger
    #[command(aliases = ["rm", "remove"])]
    Delete {
        #[arg(required = true, num_args = 1..)]
        triggers: Vec<String>,
    },
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
        path: Option<std::path::PathBuf>,
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
    /// Configure AI settings
    Ai {
        #[command(subcommand)]
        action: AiAction,
    },
    /// Generate or install shell completions
    Completions {
        #[command(subcommand)]
        action: ShellCompletionAction,
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

#[derive(Subcommand, Debug)]
enum ShellCompletionAction {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
    Install,
    Uninstall,
}

#[derive(Subcommand, Debug)]
enum AiAction {
    /// Add or update a provider credential in the OS keyring
    Add {
        #[arg(long, value_enum)]
        provider: AiProvider,
    },
    /// List configured AI providers
    List,
    /// List models known for a provider
    Models {
        #[arg(long, value_enum)]
        provider: AiProvider,
    },
    /// Remove a provider credential from the OS keyring
    Remove {
        #[arg(long, value_enum)]
        provider: AiProvider,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AiProvider {
    Openai,
    Claude,
    Gemini,
    Xai,
    Groq,
    Deepseek,
    Cohere,
    Together,
    Fireworks,
    Nebius,
    Mimo,
    Zai,
    BigModel,
    GithubCopilot,
    Custom,
}

impl AiProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Claude => "claude",
            Self::Gemini => "gemini",
            Self::Xai => "xai",
            Self::Groq => "groq",
            Self::Deepseek => "deepseek",
            Self::Cohere => "cohere",
            Self::Together => "together",
            Self::Fireworks => "fireworks",
            Self::Nebius => "nebius",
            Self::Mimo => "mimo",
            Self::Zai => "zai",
            Self::BigModel => "bigmodel",
            Self::GithubCopilot => "github_copilot",
            Self::Custom => "custom",
        }
    }
}

impl From<AiProvider> for taurine_core::ai::AiProvider {
    fn from(value: AiProvider) -> Self {
        match value {
            AiProvider::Openai => Self::Openai,
            AiProvider::Claude => Self::Claude,
            AiProvider::Gemini => Self::Gemini,
            AiProvider::Xai => Self::Xai,
            AiProvider::Groq => Self::Groq,
            AiProvider::Deepseek => Self::Deepseek,
            AiProvider::Cohere => Self::Cohere,
            AiProvider::Together => Self::Together,
            AiProvider::Fireworks => Self::Fireworks,
            AiProvider::Nebius => Self::Nebius,
            AiProvider::Mimo => Self::Mimo,
            AiProvider::Zai => Self::Zai,
            AiProvider::BigModel => Self::BigModel,
            AiProvider::GithubCopilot => Self::GithubCopilot,
            AiProvider::Custom => Self::Custom,
        }
    }
}

#[derive(Parser, Debug)]
#[command(args_conflicts_with_subcommands = true)]
pub struct AddArgs {
    #[command(subcommand)]
    pub sub: Option<AddSubcommand>,

    /// Interpret the trigger positional as a hotkey trigger
    #[arg(long)]
    pub hotkey: bool,

    /// Limit execution to specific applications (comma-separated list).
    /// Labels are exact and case-insensitive: Windows executables without .exe (e.g. 'code'),
    /// Linux WM_CLASS (e.g. 'google-chrome'), macOS localized names (e.g. 'Visual Studio Code').
    #[arg(long)]
    pub include_apps: Option<String>,

    /// Prevent execution in specific applications (comma-separated list).
    /// Labels are exact and case-insensitive: Windows executables without .exe (e.g. 'code'),
    /// Linux WM_CLASS (e.g. 'google-chrome'), macOS localized names (e.g. 'Visual Studio Code').
    #[arg(long)]
    pub exclude_apps: Option<String>,

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
        /// Interpret the trigger positional as a hotkey trigger
        #[arg(long)]
        hotkey: bool,
        /// The script content (optional if --file is used)
        #[arg(required_unless_present = "file")]
        content: Option<String>,
        /// Path to the script file
        #[arg(short, long)]
        file: Option<std::path::PathBuf>,
        /// Interpreter to use (bash, powershell, python, node, node-esm, cmd)
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
        /// Limit execution to specific applications (comma-separated list).
        /// Labels are exact and case-insensitive: Windows executables without .exe (e.g. 'code'),
        /// Linux WM_CLASS (e.g. 'google-chrome'), macOS localized names (e.g. 'Visual Studio Code').
        #[arg(long)]
        include_apps: Option<String>,
        /// Prevent execution in specific applications (comma-separated list).
        /// Labels are exact and case-insensitive: Windows executables without .exe (e.g. 'code'),
        /// Linux WM_CLASS (e.g. 'google-chrome'), macOS localized names (e.g. 'Visual Studio Code').
        #[arg(long)]
        exclude_apps: Option<String>,
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
    Node,
    NodeEsm,
    Cmd,
}

impl From<ScriptInterpreterCli> for taurine_core::engine::shell::ScriptInterpreter {
    fn from(val: ScriptInterpreterCli) -> Self {
        match val {
            ScriptInterpreterCli::Bash => Self::Bash,
            ScriptInterpreterCli::Powershell => Self::PowerShell,
            ScriptInterpreterCli::Python => Self::Python,
            ScriptInterpreterCli::Node => Self::Node,
            ScriptInterpreterCli::NodeEsm => Self::NodeEsm,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchTarget {
    Daemon,
    Tui,
    Command,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();

    if cli.version {
        println!("taurine {}", VERSION);
        return std::process::ExitCode::SUCCESS;
    }

    let launch_target = launch_target(&cli);

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
        cli.show_log_prefixes,
        component,
        launch_target == LaunchTarget::Tui,
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

    if let Err(e) = run(cli, launch_target) {
        error!(error=%e, "Taurine exited with an error");
        return std::process::ExitCode::from(1);
    }

    std::process::ExitCode::SUCCESS
}

fn run(cli: Cli, launch_target: LaunchTarget) -> taurine_core::error::Result<()> {
    match launch_target {
        LaunchTarget::Daemon => {
            info!("Initializing Taurine v{VERSION}");

            // Auto-update: check at most once every 24 hours, non-blocking
            let auto_update = {
                use taurine_core::{db::init, settings::SettingsManager};
                init::setup()
                    .ok()
                    .map(|conn| SettingsManager::new(&conn).load_all().auto_update)
                    .unwrap_or(true)
            };
            if auto_update && commands::update::should_check_now() {
                std::thread::spawn(|| {
                    let _ = commands::update::run_auto_update();
                });
            }

            // Execute the startup sequence (database init, seed, etc.)
            taurine_daemon::start()?;
            info!("Taurine daemon stopped cleanly.");
        }
        LaunchTarget::Tui => return taurine_tui::run(),
        LaunchTarget::Command => {}
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
                taurine_core::service::restart(start_on_boot)?;
            } else {
                taurine_core::service::up(start_on_boot)?;
            }
        }
        Some(Commands::Down) => taurine_core::service::down()?,
        Some(Commands::Status) => taurine_core::service::status()?,
        Some(Commands::Update) => commands::update::execute()?,
        Some(Commands::Add(args)) => {
            if let Some(AddSubcommand::Script {
                trigger,
                hotkey,
                content,
                file,
                lang,
                mode,
                os,
                include_apps,
                exclude_apps,
            }) = args.sub
            {
                commands::script::execute(
                    trigger,
                    hotkey,
                    content,
                    file,
                    lang.map(Into::into),
                    mode.into(),
                    os.to_db_str().map(|s| s.to_string()).unwrap_or_else(|| {
                        taurine_core::db::get_current_os_db_string().to_string()
                    }),
                    include_apps,
                    exclude_apps,
                )?;
            } else if let (Some(t), Some(o)) = (args.trigger, args.output) {
                let os = args
                    .os
                    .to_db_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| taurine_core::db::get_current_os_db_string().to_string());
                commands::add::execute(
                    t,
                    o,
                    os,
                    args.hotkey,
                    args.include_apps,
                    args.exclude_apps,
                )?;
            } else {
                // Show help for add command if neither subcommand nor positional args are valid
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                if let Some(add_cmd) = cmd.get_subcommands_mut().find(|c| c.get_name() == "add") {
                    add_cmd.print_help()?;
                }
            }
        }
        Some(Commands::Delete { triggers }) => {
            commands::delete::execute(triggers)?;
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
                    error!("error: provide a key to reset or use --all to reset everything");
                    std::process::exit(1);
                }
            }
        },
        Some(Commands::Ai { action }) => match action {
            AiAction::Add { provider } => commands::ai::execute_add(provider.into())?,
            AiAction::List => commands::ai::execute_list()?,
            AiAction::Models { provider } => commands::ai::execute_models(provider.into())?,
            AiAction::Remove { provider } => commands::ai::execute_remove(provider.into())?,
        },
        Some(Commands::Completions { action }) => {
            commands::completions::handle_completion(&action)?;
        }
        None => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            cmd.print_help()?;
        }
    }

    Ok(())
}

fn launch_target(cli: &Cli) -> LaunchTarget {
    if cli.daemon {
        LaunchTarget::Daemon
    } else if cli.command.is_none() {
        LaunchTarget::Tui
    } else {
        LaunchTarget::Command
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ai_add_provider() {
        let cli = Cli::try_parse_from(["taurine", "ai", "add", "--provider", "openai"])
            .expect("ai add should parse");

        match cli.command {
            Some(Commands::Ai {
                action: AiAction::Add { provider },
            }) => assert_eq!(provider, AiProvider::Openai),
            other => panic!("unexpected command parse: {other:?}"),
        }
    }

    #[test]
    fn parses_ai_models_provider() {
        let cli = Cli::try_parse_from(["taurine", "ai", "models", "--provider", "gemini"])
            .expect("ai models should parse");

        match cli.command {
            Some(Commands::Ai {
                action: AiAction::Models { provider },
            }) => assert_eq!(provider, AiProvider::Gemini),
            other => panic!("unexpected command parse: {other:?}"),
        }
    }

    #[test]
    fn rejects_forbidden_ai_add_key_flag() {
        let err = Cli::try_parse_from([
            "taurine",
            "ai",
            "add",
            "--provider",
            "claude",
            "--key",
            "secret",
        ])
        .expect_err("--key must be rejected");

        let rendered = err.to_string();
        assert!(
            rendered.contains("--key"),
            "expected clap to mention the forbidden flag, got: {rendered}"
        );
    }

    #[test]
    fn parses_add_hotkey_flag_as_boolean_mode() {
        let cli = Cli::try_parse_from(["taurine", "add", "--hotkey", "Ctrl+Shift+G", "git status"])
            .expect("add --hotkey should parse");

        match cli.command {
            Some(Commands::Add(args)) => {
                assert!(args.hotkey);
                assert_eq!(args.trigger.as_deref(), Some("Ctrl+Shift+G"));
                assert_eq!(args.output.as_deref(), Some("git status"));
            }
            other => panic!("unexpected command parse: {other:?}"),
        }
    }

    #[test]
    fn parses_add_script_hotkey_flag() {
        let cli = Cli::try_parse_from([
            "taurine",
            "add",
            "script",
            "--hotkey",
            "ctrl+shift+w",
            "-l",
            "powershell",
            "winget install [0]",
        ])
        .expect("add script --hotkey should parse");

        match cli.command {
            Some(Commands::Add(AddArgs {
                sub:
                    Some(AddSubcommand::Script {
                        trigger,
                        hotkey,
                        content,
                        ..
                    }),
                ..
            })) => {
                assert!(hotkey);
                assert_eq!(trigger, "ctrl+shift+w");
                assert_eq!(content.as_deref(), Some("winget install [0]"));
            }
            other => panic!("unexpected command parse: {other:?}"),
        }
    }

    #[test]
    fn no_args_route_to_tui() {
        let cli = Cli::try_parse_from(["taurine"]).expect("no-args invocation should parse");
        assert_eq!(launch_target(&cli), LaunchTarget::Tui);
    }

    #[test]
    fn subcommands_continue_to_route_to_cli_handlers() {
        let cli = Cli::try_parse_from(["taurine", "ls"]).expect("list alias should parse");
        assert_eq!(launch_target(&cli), LaunchTarget::Command);
    }

    #[test]
    fn daemon_flag_keeps_daemon_launch_path() {
        let cli = Cli::try_parse_from(["taurine", "--daemon"]).expect("--daemon should parse");
        assert_eq!(launch_target(&cli), LaunchTarget::Daemon);
    }

    #[test]
    fn version_flag_prints_expected_format() {
        // --version should exit successfully and print "taurine <semver>"
        let cli = Cli::try_parse_from(["taurine", "--version"]).expect("--version should parse");
        assert!(cli.version, "version flag should be true");
        // The actual output is printed in main() before launch_target is called,
        // so we verify the flag routing here and the constant exists.
        assert!(
            VERSION.contains('.'),
            "VERSION constant should be a semver string, got: {VERSION}"
        );
    }

    #[test]
    fn verbose_flag_only_invocation_routes_to_tui() {
        let cli = Cli::try_parse_from(["taurine", "--verbose"])
            .expect("flag-only invocation should parse");
        assert_eq!(launch_target(&cli), LaunchTarget::Tui);
    }

    #[test]
    fn no_log_file_flag_only_invocation_routes_to_tui() {
        let cli = Cli::try_parse_from(["taurine", "--no-log-file"])
            .expect("flag-only invocation should parse");
        assert_eq!(launch_target(&cli), LaunchTarget::Tui);
    }

    #[test]
    fn quiet_flag_only_invocation_routes_to_tui() {
        let cli =
            Cli::try_parse_from(["taurine", "-q"]).expect("flag-only invocation should parse");
        assert_eq!(launch_target(&cli), LaunchTarget::Tui);
    }
}
