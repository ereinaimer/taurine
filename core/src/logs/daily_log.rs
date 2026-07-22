use std::fs::{self, OpenOptions};
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};

use time::OffsetDateTime;

use super::{LOG_FILE_PREFIX, LOG_FILE_SUFFIX, LogComponent};

enum DailyLogTarget {
    File(std::fs::File),
    Stderr(std::io::Stderr),
}

pub(crate) struct DailyRotatingLogWriter {
    logs_dir: PathBuf,
    retention_days: i64,
    component: LogComponent,
    target: DailyLogTarget,
    current_date: String,
    last_cleanup_date: String,
}

impl DailyRotatingLogWriter {
    pub(crate) fn new(logs_dir: PathBuf, retention_days: i64, component: LogComponent) -> Self {
        let current_date = local_date_string();
        // Ensure the initial cleanup runs at startup.
        let last_cleanup_date = String::new();

        let target = match open_log_file_best_effort(&logs_dir, &current_date) {
            Ok(mut file) => {
                // Write the session header banner so multiple sessions within
                // the same day are clearly delineated.
                let _ = write_session_banner(&mut file, component);
                DailyLogTarget::File(file)
            }
            Err(_) => DailyLogTarget::Stderr(io::stderr()),
        };

        let mut writer = Self {
            logs_dir,
            retention_days,
            component,
            target,
            current_date,
            last_cleanup_date,
        };

        writer.cleanup_old_logs(); // best-effort
        writer
    }

    fn rotate_if_needed(&mut self) -> io::Result<()> {
        let new_date = local_date_string();
        if new_date == self.current_date {
            return Ok(());
        }

        if let Ok(mut file) = open_log_file_best_effort(&self.logs_dir, &new_date) {
            let _ = write_session_banner(&mut file, self.component);
            self.target = DailyLogTarget::File(file);
            self.current_date = new_date.clone();
        }

        let _ = self.cleanup_old_logs_for_date(&new_date);
        Ok(())
    }

    fn cleanup_old_logs(&mut self) {
        let current = self.current_date.clone();
        let _ = self.cleanup_old_logs_for_date(&current);
    }

    fn cleanup_old_logs_for_date(&mut self, date_str: &str) -> io::Result<()> {
        // Only cleanup once per day (on first rotation/open after midnight).
        if date_str == self.last_cleanup_date {
            return Ok(());
        }
        self.last_cleanup_date = date_str.to_string();

        let cutoff_date_str = cutoff_local_date_string(self.retention_days);

        // Clean up files in the component's own subdirectory.
        cleanup_dir_with_cutoff(
            &self.logs_dir,
            &cutoff_date_str,
            LOG_FILE_PREFIX,
            LOG_FILE_SUFFIX,
        );

        Ok(())
    }
}

impl Write for DailyRotatingLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.rotate_if_needed()?;
        match &mut self.target {
            DailyLogTarget::File(file) => file.write(buf),
            DailyLogTarget::Stderr(stderr) => stderr.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.target {
            DailyLogTarget::File(file) => file.flush(),
            DailyLogTarget::Stderr(stderr) => stderr.flush(),
        }
    }
}

/// Delete log files in `dir` whose embedded date is older than `cutoff_date_str`.
fn cleanup_dir_with_cutoff(dir: &Path, cutoff_date_str: &str, prefix: &str, suffix: &str) {
    let entries = match fs::read_dir(dir) {
        Ok(v) => v,
        Err(_) => return, // best-effort
    };

    for entry in entries {
        let entry = match entry {
            Ok(v) => v,
            Err(_) => continue,
        };
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();

        if !file_name.starts_with(prefix) || !file_name.ends_with(suffix) {
            continue;
        }

        let date_part = &file_name[prefix.len()..file_name.len() - suffix.len()];

        // ISO `YYYY-MM-DD` sorts lexicographically.
        if date_part < cutoff_date_str {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn open_log_file(logs_dir: &Path, date_str: &str) -> io::Result<std::fs::File> {
    let file_name = format!("{LOG_FILE_PREFIX}{date_str}{LOG_FILE_SUFFIX}");
    let mut options = OpenOptions::new();
    options.create(true).append(true);

    #[cfg(all(unix, not(target_os = "android")))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    options.open(logs_dir.join(file_name))
}

fn open_log_file_best_effort(logs_dir: &Path, date_str: &str) -> io::Result<std::fs::File> {
    // Best-effort directory creation to help in cases where the base
    // `data_dir` exists but `logs/<component>/` hasn't been created yet.
    let _ = fs::create_dir_all(logs_dir);
    open_log_file(logs_dir, date_str)
}

/// Writes a human-readable session header into the log file so that
/// multiple process starts within the same day are clearly separated.
fn write_session_banner(file: &mut std::fs::File, component: LogComponent) -> io::Result<()> {
    let now_str = local_datetime_string();
    let pid = std::process::id();
    let version = env!("CARGO_PKG_VERSION");
    let component_name = component.dir_name();
    let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH);

    writeln!(file)?;
    writeln!(
        file,
        "================================================================"
    )?;
    writeln!(file, "  Session started: {now_str}")?;
    writeln!(
        file,
        "  Component: {component_name} | PID: {pid} | Version: {version}"
    )?;
    writeln!(file, "  Platform: {platform}")?;
    writeln!(
        file,
        "================================================================"
    )?;
    file.flush()?;
    Ok(())
}

pub(crate) fn local_date_string() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let date = now.date();
    // Keep it stable for lexicographic comparison / parsing.
    date.format(&time::macros::format_description!("[year]-[month]-[day]"))
        .unwrap_or_else(|_| "1970-01-01".to_string())
}

fn local_datetime_string() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    now.format(&time::macros::format_description!(
        "[year]-[month]-[day] [hour]:[minute]:[second]"
    ))
    .unwrap_or_else(|_| "1970-01-01 00:00:00".to_string())
}

fn cutoff_local_date_string(retention_days: i64) -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let cutoff = now - time::Duration::days(retention_days);
    let date = cutoff.date();
    date.format(&time::macros::format_description!("[year]-[month]-[day]"))
        .unwrap_or_else(|_| "1970-01-01".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::DEFAULT_RETENTION_DAYS;
    use tempfile::tempdir;

    #[test]
    fn test_local_date_string_format() {
        let s = local_date_string();
        assert!(s.len() == 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
    }

    #[test]
    fn test_daily_log_file_name_and_creation() {
        let dir = tempdir().expect("tempdir");
        let logs_dir = dir.path().to_path_buf();

        let date_str = local_date_string();
        let expected = logs_dir.join(format!("{LOG_FILE_PREFIX}{date_str}{LOG_FILE_SUFFIX}"));

        let mut writer =
            DailyRotatingLogWriter::new(logs_dir, DEFAULT_RETENTION_DAYS, LogComponent::Cli);
        writer.write_all(b"hello\n").expect("write");
        writer.flush().expect("flush");

        assert!(
            expected.exists(),
            "expected log file to exist: {}",
            expected.display()
        );
    }

    #[test]
    fn test_session_banner_is_written() {
        let dir = tempdir().expect("tempdir");
        let logs_dir = dir.path().to_path_buf();

        let date_str = local_date_string();
        let log_path = logs_dir.join(format!("{LOG_FILE_PREFIX}{date_str}{LOG_FILE_SUFFIX}"));

        let mut writer =
            DailyRotatingLogWriter::new(logs_dir, DEFAULT_RETENTION_DAYS, LogComponent::Daemon);
        writer.write_all(b"after banner\n").expect("write");
        writer.flush().expect("flush");

        let contents = fs::read_to_string(&log_path).expect("read log file");
        assert!(
            contents.contains("Session started:"),
            "expected session banner in log file"
        );
        assert!(
            contents.contains("Component: service"),
            "expected component name in session banner"
        );
        assert!(
            contents.contains("after banner"),
            "expected log content after banner"
        );
    }

    #[test]
    fn test_log_retention_deletes_logs_older_than_cutoff() {
        let dir = tempdir().expect("tempdir");
        let logs_dir = dir.path().to_path_buf();

        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        let cutoff = now - time::Duration::days(DEFAULT_RETENTION_DAYS);
        let older = now - time::Duration::days(DEFAULT_RETENTION_DAYS + 1);

        let cutoff_date_str = cutoff
            .date()
            .format(&time::macros::format_description!("[year]-[month]-[day]"))
            .unwrap();
        let older_date_str = older
            .date()
            .format(&time::macros::format_description!("[year]-[month]-[day]"))
            .unwrap();
        let today_date_str = local_date_string();

        let keep_path = logs_dir.join(format!(
            "{LOG_FILE_PREFIX}{cutoff_date_str}{LOG_FILE_SUFFIX}"
        ));
        let old_path = logs_dir.join(format!(
            "{LOG_FILE_PREFIX}{older_date_str}{LOG_FILE_SUFFIX}"
        ));

        // Create two files: one older than cutoff (should be deleted),
        // one at cutoff (should remain).
        fs::create_dir_all(&logs_dir).expect("create logs dir");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&keep_path)
            .expect("create keep log");
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(&old_path)
            .expect("create old log");

        // Creating the writer triggers cleanup at startup.
        let _writer =
            DailyRotatingLogWriter::new(logs_dir, DEFAULT_RETENTION_DAYS, LogComponent::Cli);

        assert!(
            !old_path.exists(),
            "expected old log file to be deleted: {}",
            old_path.display()
        );
        assert!(
            keep_path.exists(),
            "expected cutoff log file to be kept: {}",
            keep_path.display()
        );

        // Also ensures today's log file is created.
        let today_path = dir.path().join(format!(
            "{LOG_FILE_PREFIX}{today_date_str}{LOG_FILE_SUFFIX}"
        ));
        assert!(today_path.exists(), "expected today's log file to exist");
    }
}
