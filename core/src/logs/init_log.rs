use crate::logs::daily_log::{local_date_string, logs_dir, DailyRotatingLogWriter};
use std::backtrace::Backtrace;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write;
use std::panic::PanicHookInfo;
use std::sync::OnceLock;

use tracing_error::ErrorLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::prelude::*;

use super::{DEFAULT_RETENTION_DAYS, LOG_FILE_PREFIX, LOG_FILE_SUFFIX};

static TRACING_INIT: OnceLock<()> = OnceLock::new();
static TEST_TRACING_INIT: OnceLock<()> = OnceLock::new();
static PANIC_HOOK_INIT: OnceLock<()> = OnceLock::new();
static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Initialize tracing for normal application runtime.
///
/// - Console formatting matches the CLI verbose flag.
/// - File logs default to `debug` unless `RUST_LOG` overrides.
/// - Logs are written into `.../logs/taurine-log-YYYY-MM-DD.txt` with
///   7-day retention.
pub fn init_tracing_for_app(verbose_console: bool) {
    let _ = TRACING_INIT.get_or_init(|| {
        let logs_dir = logs_dir();
        let retention_days = DEFAULT_RETENTION_DAYS;

        // If `RUST_LOG` is set, use it for both console and file.
        // Otherwise, default console differs based on `verbose_console`,
        // but file logs remain at `debug`.
        let (console_filter, file_filter) = if std::env::var_os("RUST_LOG").is_some() {
            let filter =
                EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
            (filter.clone(), filter)
        } else {
            let default_console = if verbose_console { "debug" } else { "info" };
            (EnvFilter::new(default_console), EnvFilter::new("debug"))
        };

        let file_writer = DailyRotatingLogWriter::new(logs_dir, retention_days);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_writer);
        let _ = LOG_GUARD.set(guard);

        // Build subscriber in two branches so we can keep the original
        // compact/no-time console formatting in non-verbose mode.
        if verbose_console {
            let console_timer = tracing_subscriber::fmt::time::LocalTime::new(
                time::macros::format_description!(
                    "[year]-[month]-[day] [hour]:[minute]:[second]"
                ),
            );

            let console_layer = tracing_subscriber::fmt::layer()
                .with_timer(console_timer)
                .with_target(false)
                .with_file(false)
                .with_line_number(false)
                .with_filter(console_filter);

            let file_timer = tracing_subscriber::fmt::time::LocalTime::new(
                time::macros::format_description!(
                    "[year]-[month]-[day] [hour]:[minute]:[second]"
                ),
            );
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_timer(file_timer)
                .with_ansi(false)
                .with_target(false)
                .with_file(false)
                .with_line_number(false)
                .with_filter(file_filter);

            let subscriber = tracing_subscriber::registry()
                .with(console_layer)
                .with(file_layer)
                .with(ErrorLayer::default());

            let _ = tracing::subscriber::set_global_default(subscriber);
        } else {
            let console_layer = tracing_subscriber::fmt::layer()
                .compact()
                .with_target(false)
                .with_file(false)
                .with_line_number(false)
                .without_time()
                .with_filter(console_filter);

            let file_timer = tracing_subscriber::fmt::time::LocalTime::new(
                time::macros::format_description!(
                    "[year]-[month]-[day] [hour]:[minute]:[second]"
                ),
            );
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_timer(file_timer)
                .with_ansi(false)
                .with_target(false)
                .with_file(false)
                .with_line_number(false)
                .with_filter(file_filter);

            let subscriber = tracing_subscriber::registry()
                .with(console_layer)
                .with(file_layer)
                .with(ErrorLayer::default());

            let _ = tracing::subscriber::set_global_default(subscriber);
        }
    });
}

/// Initialize tracing for tests.
pub fn init_tracing_for_tests() {
    let _ = TEST_TRACING_INIT.get_or_init(|| {
        let filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("error"));

        // Tests log to console only; no file writer for clean app logs.
        let _ = tracing_subscriber::fmt()
            .compact()
            .with_target(false)
            .with_file(false)
            .with_line_number(false)
            .without_time()
            .with_env_filter(filter)
            .try_init();
    });
}

/// Install a panic hook that logs the panic to tracing and writes a
/// synchronous copy into the current daily log file.
pub fn install_tracing_panic_hook() {
    let _ = PANIC_HOOK_INIT.get_or_init(|| {
        std::panic::set_hook(Box::new(|panic_info| {
            handle_panic_info(panic_info);
        }));
    });
}

/// Handle a panic by emitting structured tracing logs and also writing a
/// synchronous, best-effort panic report to the current daily log file.
///
/// This is safe to call from a panic hook.
pub fn handle_panic_info(panic_info: &PanicHookInfo<'_>) {
    let location = panic_info
        .location()
        .map(|l| format!("{}:{}", l.file(), l.line()))
        .unwrap_or_else(|| "<unknown>".to_string());

    let payload = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    };

    // Capture backtrace early; it might be empty depending on build flags.
    let backtrace = Backtrace::capture();

    tracing::error!(
        panic_payload = %payload,
        panic_location = %location,
        backtrace = ?backtrace,
        "application panicked"
    );

    // Also synchronously write the panic to the log file so we keep useful
    // diagnostics even if the non-blocking writer hasn't flushed yet.
    let _ = write_panic_to_log_file(&payload, &location, &backtrace);
}

fn write_panic_to_log_file(payload: &str, location: &str, backtrace: &Backtrace) -> io::Result<()> {
    let logs_dir = logs_dir();
    fs::create_dir_all(&logs_dir)?;
    let date_str = local_date_string();
    let file_name = format!("{LOG_FILE_PREFIX}{date_str}{LOG_FILE_SUFFIX}");
    let path = logs_dir.join(file_name);

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "==================== PANIC ====================")?;
    writeln!(file, "panic_payload: {payload}")?;
    writeln!(file, "panic_location: {location}")?;
    writeln!(file, "backtrace:\n{backtrace:?}")?;
    writeln!(file, "================================================")?;
    file.flush()?;
    Ok(())
}

