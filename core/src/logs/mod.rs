//! Shared tracing + logging utilities used by both the app and tests.

mod daily_log;
mod init_log;

pub use init_log::{
    handle_panic_info, init_tracing_for_app, init_tracing_for_tests, install_tracing_panic_hook,
};

/// Identifies which part of the application is logging.
///
/// Each component writes to its own subdirectory under `logs/`, keeping
/// CLI invocations and the long-running daemon cleanly separated.
#[derive(Debug, Clone, Copy)]
pub enum LogComponent {
    Cli,
    Daemon,
}

impl LogComponent {
    /// Returns the subdirectory name under `logs/`.
    pub fn dir_name(&self) -> &'static str {
        match self {
            LogComponent::Cli => "cli",
            LogComponent::Daemon => "daemon",
        }
    }
}

pub(crate) const LOG_FILE_PREFIX: &str = "taurine-";
pub(crate) const LOG_FILE_SUFFIX: &str = ".log";
pub(crate) const DEFAULT_RETENTION_DAYS: i64 = 7;
