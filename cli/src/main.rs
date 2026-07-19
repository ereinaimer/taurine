// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.
pub mod commands;

use clap::{Parser, Subcommand, ValueEnum};
use tracing::{error, info};

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine", version = env!("CARGO_PKG_VERSION"), disable_version_flag = true)]
#[command(about = "Text expander")]
struct Cli {
    /// Increase console verbosity
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Suppress console output
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    quiet: bool,

    /// Disable log file
    #[arg(long, global = true)]
    no_log_file: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

    /// Show log prefixes
    #[arg(long, global = true)]
    show_log_prefixes: bool,

    /// Print version
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
    /// Update Taurine
    Update,
    /// Check Taurine status
    Status,
    #[cfg(target_os = "linux")]
    /// Configure system permissions for hardware access
    #[command(hide = true)]
    Setup,
    /// Add a new automation
    #[command(alias = "set")]
    Add(Box<AddArgs>),
    /// Remove an automation
    #[command(aliases = ["rm", "remove"])]
    Delete {
        /// Remove by tag
        #[arg(long)]
        tag: Option<String>,

        #[arg(required_unless_present = "tag", num_args = 0..)]
        triggers: Vec<String>,
    },
    /// List all automations
    #[command(alias = "ls")]
    List {
        /// Sort results by
        #[arg(long, value_enum, hide_possible_values = true)]
        sort: Option<SortBy>,

        /// Ascending order
        #[arg(long, conflicts_with = "desc")]
        asc: bool,

        /// Descending order
        #[arg(long, conflicts_with = "asc")]
        desc: bool,

        /// Plain output
        #[arg(long)]
        plain: bool,

        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Export automations to a file
    Export {
        /// Destination file path
        path: Option<std::path::PathBuf>,
        /// Plaintext (no encryption)
        #[arg(short = 'p', long)]
        plain: bool,
        /// Include settings
        #[arg(short = 's', long)]
        settings: bool,
        /// Include stats
        #[arg(short = 't', long)]
        stats: bool,
        /// Include sensitive settings
        #[arg(short = 'x', long)]
        sensitive: bool,
    },
    /// Import automations from a file
    Import {
        /// Source file path
        path: std::path::PathBuf,
        /// Collision resolution
        #[arg(short = 'c', long, value_enum)]
        conflict: Option<ImportConflictCli>,
        /// Overwrite local settings
        #[arg(short = 's', long)]
        settings: bool,
        /// Import stats strategy
        #[arg(short = 't', long, value_enum, num_args = 0..=1, require_equals = true, default_missing_value = "merge")]
        stats: Option<ImportStatsCli>,
        /// Import sensitive settings
        #[arg(short = 'x', long)]
        sensitive: bool,
    },
    /// Manage application settings
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Configure AI
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
    /// List configuration
    #[command(alias = "ls")]
    List,
    /// Reset a configuration value
    Reset {
        /// The setting key to reset
        key: Option<String>,
        /// Reset all settings
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
    /// Add/update AI provider
    Add {
        #[arg(long, value_enum)]
        provider: AiProvider,
    },
    /// List providers
    List,
    /// List provider models
    Models {
        #[arg(long, value_enum)]
        provider: AiProvider,
    },
    /// Remove AI provider
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

    /// Hotkey trigger
    #[arg(long)]
    pub hotkey: bool,

    /// Regex trigger
    #[arg(long, conflicts_with = "hotkey")]
    pub regex: bool,

    /// Allowed apps
    #[arg(long)]
    pub include_apps: Option<String>,

    /// Excluded apps
    #[arg(long)]
    pub exclude_apps: Option<String>,

    /// Trigger
    pub trigger: Option<String>,
    /// Output
    pub output: Option<String>,
    /// Target OS
    #[arg(long, value_enum, default_value = "all")]
    pub os: TargetOsCli,

    /// Tags
    #[arg(long = "tag", value_delimiter = ',', num_args = 1..)]
    pub tag: Option<Vec<String>>,

    /// Auto-case
    #[arg(long)]
    pub auto_case: bool,
}

#[derive(Subcommand, Debug)]
pub enum AddSubcommand {
    /// Add script automation
    Script {
        /// Trigger
        trigger: String,
        /// Hotkey trigger
        #[arg(long)]
        hotkey: bool,
        /// Regex trigger
        #[arg(long, conflicts_with = "hotkey")]
        regex: bool,
        /// Script content
        #[arg(required_unless_present = "file")]
        content: Option<String>,
        /// Script file
        #[arg(short, long)]
        file: Option<std::path::PathBuf>,
        /// Interpreter
        #[arg(
            short = 'l',
            long = "lang",
            value_enum,
            required_unless_present = "file"
        )]
        lang: Option<ScriptInterpreterCli>,
        /// Run mode
        #[arg(short = 'm', long = "mode", value_enum, default_value = "inline")]
        mode: ScriptBehaviorCli,
        /// Target OS
        #[arg(long, value_enum, default_value = "current")]
        os: TargetOsCli,
        /// Allowed apps
        #[arg(long)]
        include_apps: Option<String>,
        /// Excluded apps
        #[arg(long)]
        exclude_apps: Option<String>,

        /// Tags
        #[arg(long = "tag", value_delimiter = ',', num_args = 1..)]
        tag: Option<Vec<String>>,

        /// Auto-case
        #[arg(long)]
        auto_case: bool,
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
pub enum ImportStatsCli {
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
        #[cfg(target_os = "linux")]
        Some(Commands::Setup) => {
            taurine_core::service::linux_setup()?;
        }
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
                regex,
                content,
                file,
                lang,
                mode,
                os,
                include_apps,
                exclude_apps,
                tag,
                auto_case,
            }) = args.sub
            {
                let trigger_type = if hotkey {
                    taurine_core::db::crud::TriggerType::Hotkey
                } else if regex {
                    taurine_core::db::crud::TriggerType::Regex
                } else {
                    taurine_core::db::crud::TriggerType::Word
                };
                commands::script::execute_with_trigger_type(
                    trigger,
                    trigger_type,
                    content,
                    file,
                    lang.map(Into::into),
                    mode.into(),
                    os.to_db_str().map(|s| s.to_string()).unwrap_or_else(|| {
                        taurine_core::db::get_current_os_db_string().to_string()
                    }),
                    include_apps,
                    exclude_apps,
                    tag,
                    auto_case,
                )?;
            } else if let (Some(t), Some(o)) = (args.trigger, args.output) {
                let os = args
                    .os
                    .to_db_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| taurine_core::db::get_current_os_db_string().to_string());
                let trigger_type = if args.hotkey {
                    taurine_core::db::crud::TriggerType::Hotkey
                } else if args.regex {
                    taurine_core::db::crud::TriggerType::Regex
                } else {
                    taurine_core::db::crud::TriggerType::Word
                };
                commands::add::execute_with_trigger_type(
                    t,
                    o,
                    os,
                    trigger_type,
                    args.include_apps,
                    args.exclude_apps,
                    args.tag,
                    args.auto_case,
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
        Some(Commands::Delete { triggers, tag }) => {
            commands::delete::execute(triggers, tag)?;
        }
        Some(Commands::List {
            sort,
            asc,
            desc,
            plain,
            tag,
        }) => {
            commands::list::execute(sort, asc, desc, plain, tag)?;
        }
        Some(Commands::Export {
            path,
            plain,
            settings,
            stats,
            sensitive,
        }) => {
            commands::export::execute(path, plain, settings, stats, sensitive)?;
        }
        Some(Commands::Import {
            path,
            conflict,
            settings,
            stats,
            sensitive,
        }) => {
            commands::import::execute(path, conflict, settings, stats, sensitive)?;
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
            Some(Commands::Add(args)) => {
                if let Some(AddSubcommand::Script {
                    trigger,
                    hotkey,
                    content,
                    ..
                }) = &args.sub
                {
                    assert!(hotkey);
                    assert_eq!(trigger, "ctrl+shift+w");
                    assert_eq!(content.as_deref(), Some("winget install [0]"));
                } else {
                    panic!("expected script subcommand");
                }
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

    #[test]
    fn cli_add_regex_flag_parses() {
        let cli = Cli::try_parse_from(["taurine", "add", "--regex", "issue-(\\d+)", "link/[0]"])
            .expect("add --regex should parse");
        match cli.command {
            Some(Commands::Add(args)) => {
                assert!(args.regex);
                assert_eq!(args.trigger.as_deref(), Some("issue-(\\d+)"));
                assert_eq!(args.output.as_deref(), Some("link/[0]"));
            }
            other => panic!("unexpected parse output: {other:?}"),
        }
    }

    #[test]
    fn parses_import_conflict_flags() {
        // Test long flag --conflict
        let cli = Cli::try_parse_from(["taurine", "import", "backup.tau", "--conflict", "skip"])
            .expect("import --conflict should parse");
        match cli.command {
            Some(Commands::Import { conflict, .. }) => {
                assert_eq!(conflict, Some(ImportConflictCli::Skip));
            }
            other => panic!("unexpected command parse: {other:?}"),
        }

        // Test short flag -c
        let cli = Cli::try_parse_from(["taurine", "import", "backup.tau", "-c", "overwrite"])
            .expect("import -c should parse");
        match cli.command {
            Some(Commands::Import { conflict, .. }) => {
                assert_eq!(conflict, Some(ImportConflictCli::Overwrite));
            }
            other => panic!("unexpected command parse: {other:?}"),
        }
    }
}
