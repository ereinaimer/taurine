// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

use clap::{Parser, Subcommand, ValueEnum};

pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Taurine Command Line Interface
#[derive(Parser, Debug)]
#[command(name = "taurine", version = env!("CARGO_PKG_VERSION"), disable_version_flag = true)]
#[command(about = "Text expander")]
pub struct Cli {
    /// Increase console verbosity
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(crate) verbose: u8,

    /// Suppress console output
    #[arg(short, long, global = true, conflicts_with = "verbose")]
    pub(crate) quiet: bool,

    /// Disable log file
    #[arg(long, global = true)]
    pub(crate) no_log_file: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    pub(crate) no_color: bool,

    /// Show log prefixes
    #[arg(long, global = true)]
    pub(crate) show_log_prefixes: bool,

    /// Print version
    #[arg(long, global = true)]
    pub(crate) version: bool,

    /// Internal flag used by the OS service manager (DO NOT RUN MANUALLY)
    #[arg(long, hide = true)]
    pub(crate) daemon: bool,

    /// Internal flag used to spawn the auto-updater process (DO NOT RUN MANUALLY)
    #[arg(long, hide = true)]
    pub(crate) auto_update: bool,

    /// Output in JSON format
    #[arg(long, global = true)]
    pub(crate) json: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
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
    /// Add a new trigger
    #[command(alias = "set")]
    Add(Box<AddArgs>),
    /// Remove a trigger
    #[command(aliases = ["rm", "remove"])]
    Delete {
        /// Remove by tag
        #[arg(long)]
        tag: Option<String>,

        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,

        #[arg(required_unless_present = "tag", num_args = 0..)]
        triggers: Vec<String>,
    },
    /// List all triggers
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

        /// Filter by tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Export triggers to a file
    Export {
        /// Destination file path
        path: Option<std::path::PathBuf>,
        /// Plaintext (no encryption)
        #[arg(short = 'p', long)]
        plain: bool,
        /// Skip interactive prompts
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Import triggers from a file
    Import {
        /// Source file path
        path: Option<std::path::PathBuf>,
        /// Collision resolution
        #[arg(short = 'c', long, value_enum)]
        conflict: Option<ImportConflictCli>,
        /// Skip interactive prompts
        #[arg(short = 'y', long)]
        yes: bool,
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
pub(crate) enum ConfigAction {
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
pub enum ShellCompletionAction {
    Bash,
    Elvish,
    Fish,
    Powershell,
    Zsh,
    Install,
    Uninstall,
}

#[derive(Subcommand, Debug)]
pub(crate) enum AiAction {
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
    /// Remove AI provider(s)
    Remove {
        /// Provider name (required unless --all is set)
        #[arg(
            long,
            value_enum,
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        provider: Option<AiProvider>,
        /// Remove all configured providers
        #[arg(short, long)]
        all: bool,
        /// Skip confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
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

    /// Display name (defaults to the trigger string)
    #[arg(long)]
    pub name: Option<String>,

    /// Description for the trigger
    #[arg(long)]
    pub description: Option<String>,

    /// Auto-case
    #[arg(long)]
    pub auto_case: bool,
}

#[derive(Subcommand, Debug)]
pub enum AddSubcommand {
    /// Add script trigger
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

        /// Display name (defaults to the trigger string)
        #[arg(long)]
        name: Option<String>,

        /// Description for the trigger
        #[arg(long)]
        description: Option<String>,

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
    pub fn to_db_str(&self) -> Option<&'static str> {
        match self {
            Self::Windows => Some(taurine_core::db::TargetOs::Windows.to_db_str()),
            Self::Macos => Some(taurine_core::db::TargetOs::MacOs.to_db_str()),
            Self::Linux => Some(taurine_core::db::TargetOs::Linux.to_db_str()),
            Self::All => Some(taurine_core::db::TargetOs::All.to_db_str()),
            Self::Android => Some(taurine_core::db::TargetOs::Android.to_db_str()),
            Self::Ios => Some(taurine_core::db::TargetOs::Ios.to_db_str()),
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
    Cmd,
}

impl From<ScriptInterpreterCli> for taurine_core::engine::shell::ScriptInterpreter {
    fn from(val: ScriptInterpreterCli) -> Self {
        match val {
            ScriptInterpreterCli::Bash => Self::Bash,
            ScriptInterpreterCli::Powershell => Self::PowerShell,
            ScriptInterpreterCli::Python => Self::Python,
            ScriptInterpreterCli::Node => Self::Node,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchTarget {
    Daemon,
    AutoUpdate,
    Tui,
    Command,
}
