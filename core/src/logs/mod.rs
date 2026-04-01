//! Shared tracing + logging utilities used by both the app and tests.

mod init_log;
mod daily_log;

pub use init_log::{
    handle_panic_info, init_tracing_for_app, init_tracing_for_tests, install_tracing_panic_hook,
};

pub(crate) const LOG_FILE_PREFIX: &str = "taurine-log-";
pub(crate) const LOG_FILE_SUFFIX: &str = ".txt";
pub(crate) const DEFAULT_RETENTION_DAYS: i64 = 7;

