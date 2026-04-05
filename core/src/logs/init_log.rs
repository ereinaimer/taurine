use crate::logs::daily_log::{DailyRotatingLogWriter, local_date_string};
use crate::paths::logs_dir;
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

use super::{DEFAULT_RETENTION_DAYS, LOG_FILE_PREFIX, LOG_FILE_SUFFIX, LogComponent};

static TRACING_INIT: std::sync::Once = std::sync::Once::new();
static TEST_TRACING_INIT: OnceLock<()> = OnceLock::new();
static PANIC_HOOK_INIT: OnceLock<()> = OnceLock::new();
static QUIET_CONSOLE: OnceLock<bool> = OnceLock::new();
static NO_LOG_FILE: OnceLock<bool> = OnceLock::new();
static ACTIVE_COMPONENT: OnceLock<LogComponent> = OnceLock::new();

/// Initialize tracing for normal application runtime.
///
/// - Console verbosity is controlled by CLI `-v/-vv/-vvv` unless `RUST_LOG` is set.
/// - `-q` or `--quiet` disables console output.
/// - `--no-log-file` disables file logging.
/// - File logs default to `debug` unless `RUST_LOG` overrides.
/// - Logs are written into `.../logs/<component>/taurine-YYYY-MM-DD.log` with
///   7-day retention.
pub fn init_tracing_for_app(
    verbosity: u8,
    quiet: bool,
    no_log_file: bool,
    no_color: bool,
    component: LogComponent,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let mut returned_guard = None;

    TRACING_INIT.call_once(|| {
        let _ = QUIET_CONSOLE.set(quiet);
        let _ = NO_LOG_FILE.set(no_log_file);
        let _ = ACTIVE_COMPONENT.set(component);

        let component_logs_dir = logs_dir().join(component.dir_name());
        let retention_days = DEFAULT_RETENTION_DAYS;

        let env_filter = EnvFilter::try_from_default_env().ok();

        if quiet && no_log_file {
            // Silent mode: no console layer and no file layer.

            // Tracing is initialized here because the tracing crate allows global subscriber
            // to be set only once per application lifecycle and we want to ensure that tracing
            // is initialized only here so that no other part of the application can
            // initialize tracing and ignore the quiet flag.

            let subscriber = tracing_subscriber::registry();
            let _ = tracing::subscriber::set_global_default(subscriber);
        } else if quiet {
            // Quiet mode: only file logging.
            let file_filter = env_filter.clone().unwrap_or_else(|| {
                EnvFilter::new("debug,h2=warn,hyper=warn,tower=warn,tonic=warn")
            });

            let file_writer =
                DailyRotatingLogWriter::new(component_logs_dir, retention_days, component);
            let (non_blocking, guard) = tracing_appender::non_blocking(file_writer);
            returned_guard = Some(guard);

            let file_timer = tracing_subscriber::fmt::time::LocalTime::new(
                time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
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
                .with(file_layer)
                .with(ErrorLayer::default());
            let _ = tracing::subscriber::set_global_default(subscriber);
        } else if no_log_file {
            // No log file: only console logging.
            let console_level = match verbosity {
                0 => "info,h2=error,hyper=error,tower=error,tonic=error",
                1 => "debug,h2=warn,hyper=warn,tower=warn,tonic=warn",
                _ => "trace,h2=info,hyper=info,tower=info,tonic=info",
            };
            let console_filter = env_filter.unwrap_or_else(|| EnvFilter::new(console_level));

            let use_timestamp = verbosity > 0;
            if use_timestamp {
                let console_timer = tracing_subscriber::fmt::time::LocalTime::new(
                    time::macros::format_description!(
                        "[year]-[month]-[day] [hour]:[minute]:[second]"
                    ),
                );
                let console_layer = tracing_subscriber::fmt::layer()
                    .with_timer(console_timer)
                    .with_ansi(!no_color)
                    .with_target(false)
                    .with_file(false)
                    .with_line_number(false)
                    .with_filter(console_filter);

                let subscriber = tracing_subscriber::registry()
                    .with(console_layer)
                    .with(ErrorLayer::default());
                let _ = tracing::subscriber::set_global_default(subscriber);
            } else {
                let console_layer = tracing_subscriber::fmt::layer()
                    .compact()
                    .with_ansi(!no_color)
                    .with_target(false)
                    .with_file(false)
                    .with_line_number(false)
                    .without_time()
                    .with_filter(console_filter);

                let subscriber = tracing_subscriber::registry()
                    .with(console_layer)
                    .with(ErrorLayer::default());
                let _ = tracing::subscriber::set_global_default(subscriber);
            }
        } else {
            // Normal mode: console and file logging.
            let file_filter = env_filter.clone().unwrap_or_else(|| {
                EnvFilter::new("debug,h2=warn,hyper=warn,tower=warn,tonic=warn")
            });

            let file_writer =
                DailyRotatingLogWriter::new(component_logs_dir, retention_days, component);
            let (non_blocking, guard) = tracing_appender::non_blocking(file_writer);
            returned_guard = Some(guard);

            let console_level = match verbosity {
                0 => "info,h2=error,hyper=error,tower=error,tonic=error",
                1 => "debug,h2=warn,hyper=warn,tower=warn,tonic=warn",
                _ => "trace,h2=info,hyper=info,tower=info,tonic=info",
            };
            let console_filter = env_filter.unwrap_or_else(|| EnvFilter::new(console_level));

            let use_timestamp = verbosity > 0;
            if use_timestamp {
                let console_timer = tracing_subscriber::fmt::time::LocalTime::new(
                    time::macros::format_description!(
                        "[year]-[month]-[day] [hour]:[minute]:[second]"
                    ),
                );
                let console_layer = tracing_subscriber::fmt::layer()
                    .with_timer(console_timer)
                    .with_ansi(!no_color)
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
                    .with_ansi(!no_color)
                    .with_target(false)
                    .with_file(false)
                    .with_line_number(false)
                    .without_time()
                    .with_filter(console_filter);

                // File logs are always on debug level
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
        }
    });

    returned_guard
}

/// Initialize tracing for tests.
pub fn init_tracing_for_tests() {
    let _ = TEST_TRACING_INIT.get_or_init(|| {
        let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("error"));

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
    let quiet_console = QUIET_CONSOLE.get().copied().unwrap_or(false);
    let no_log_file = NO_LOG_FILE.get().copied().unwrap_or(false);

    if quiet_console && no_log_file {
        return;
    }

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
        "Application panicked"
    );

    // Also synchronously write the panic to the log file so we keep useful
    // diagnostics even if the non-blocking writer hasn't flushed yet.
    let no_log_file = NO_LOG_FILE.get().copied().unwrap_or(false);
    if !no_log_file {
        let _ = write_panic_to_log_file(&payload, &location, &backtrace);
    }
}

fn write_panic_to_log_file(payload: &str, location: &str, backtrace: &Backtrace) -> io::Result<()> {
    let component = ACTIVE_COMPONENT.get().copied().unwrap_or(LogComponent::Cli);
    let component_logs_dir = logs_dir().join(component.dir_name());
    fs::create_dir_all(&component_logs_dir)?;
    let date_str = local_date_string();
    let file_name = format!("{LOG_FILE_PREFIX}{date_str}{LOG_FILE_SUFFIX}");
    let path = component_logs_dir.join(file_name);

    let mut options = OpenOptions::new();
    options.create(true).append(true);

    #[cfg(all(unix, not(target_os = "android")))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    writeln!(file, "==================== PANIC ====================")?;
    writeln!(file, "panic_payload: {payload}")?;
    writeln!(file, "panic_location: {location}")?;
    writeln!(file, "backtrace:\n{backtrace:?}")?;
    writeln!(file, "================================================")?;
    file.flush()?;
    Ok(())
}
