// Service updater process spawning and timestamp persistence logic

use rusqlite::Connection;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info};

pub const LAST_UPDATE_CHECK_KEY: &str = "last_update_check";
pub const UPDATE_CHECK_INTERVAL_SECS: u64 = 6 * 60 * 60; // 6 hours

/// Returns current Unix time in seconds.
pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Spawns the current executable in a background process with `--auto-update`.
pub fn spawn_updater_process() {
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--auto-update");

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        if let Err(e) = cmd.spawn() {
            error!("Failed to spawn auto-updater process: {}", e);
        }
    }
}

/// Reads the `last_update_check` timestamp (in Unix seconds) from the settings table.
pub fn get_last_update_check(conn: &Connection) -> Option<u64> {
    match crate::db::crud::get_setting_value(conn, LAST_UPDATE_CHECK_KEY) {
        Ok(Some(val)) => val.trim().parse::<u64>().ok(),
        Ok(None) => None,
        Err(e) => {
            error!("Failed to query {}: {}", LAST_UPDATE_CHECK_KEY, e);
            None
        }
    }
}

/// Persists the `last_update_check` timestamp (in Unix seconds) into the settings table.
pub fn set_last_update_check(conn: &Connection, timestamp: u64) -> crate::error::Result<()> {
    crate::db::crud::upsert_setting(conn, LAST_UPDATE_CHECK_KEY, &timestamp.to_string())
        .map_err(crate::error::Error::Database)
}

/// Determines whether an update check should run based on the `auto_update` setting
/// and elapsed time since the last check.
pub fn should_check_for_updates(conn: &Connection, auto_update_enabled: bool, now: u64) -> bool {
    if !auto_update_enabled {
        return false;
    }

    match get_last_update_check(conn) {
        Some(last_check) => {
            if now < last_check {
                // Clock skew / timestamp in future: trigger check to recover
                true
            } else {
                now.saturating_sub(last_check) >= UPDATE_CHECK_INTERVAL_SECS
            }
        }
        None => true,
    }
}

/// Performs startup check for updates: if `auto_update` is enabled, spawns the
/// updater process and updates the `last_update_check` timestamp.
pub fn check_on_startup(conn: &Connection, auto_update_enabled: bool) {
    if !auto_update_enabled {
        debug!("Auto-update is disabled; skipping startup update check.");
        return;
    }

    let now = now_unix_secs();
    info!("Running startup update check...");
    if let Err(e) = set_last_update_check(conn, now) {
        error!(
            "Failed to update {} on startup: {}",
            LAST_UPDATE_CHECK_KEY, e
        );
    }
    spawn_updater_process();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("failed to open memory DB");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (
                key        TEXT    PRIMARY KEY,
                value      JSON    NOT NULL,
                version    INTEGER DEFAULT 1,
                updated_at INTEGER NOT NULL
            );",
        )
        .expect("setup schema failed");
        conn
    }

    #[test]
    fn test_get_and_set_last_update_check() {
        let conn = open_test_conn();
        assert_eq!(get_last_update_check(&conn), None);

        set_last_update_check(&conn, 1700000000).unwrap();
        assert_eq!(get_last_update_check(&conn), Some(1700000000));

        set_last_update_check(&conn, 1700005000).unwrap();
        assert_eq!(get_last_update_check(&conn), Some(1700005000));
    }

    #[test]
    fn test_should_check_for_updates_disabled() {
        let conn = open_test_conn();
        assert!(!should_check_for_updates(&conn, false, 1700000000));
    }

    #[test]
    fn test_should_check_for_updates_never_checked() {
        let conn = open_test_conn();
        assert!(should_check_for_updates(&conn, true, 1700000000));
    }

    #[test]
    fn test_should_check_for_updates_recent_check() {
        let conn = open_test_conn();
        let last_check = 1700000000;
        set_last_update_check(&conn, last_check).unwrap();

        // 1 hour later (less than 6 hours)
        let now = last_check + 3600;
        assert!(!should_check_for_updates(&conn, true, now));

        // 5 hours 59 mins later
        let now = last_check + (6 * 3600 - 60);
        assert!(!should_check_for_updates(&conn, true, now));
    }

    #[test]
    fn test_should_check_for_updates_elapsed_check() {
        let conn = open_test_conn();
        let last_check = 1700000000;
        set_last_update_check(&conn, last_check).unwrap();

        // Exactly 6 hours later
        let now = last_check + (6 * 3600);
        assert!(should_check_for_updates(&conn, true, now));

        // 7 hours later
        let now = last_check + (7 * 3600);
        assert!(should_check_for_updates(&conn, true, now));
    }

    #[test]
    fn test_should_check_for_updates_clock_skew() {
        let conn = open_test_conn();
        let last_check = 1700000000;
        set_last_update_check(&conn, last_check).unwrap();

        // Current time is before recorded time
        let now = last_check - 1000;
        assert!(should_check_for_updates(&conn, true, now));
    }

    #[test]
    fn test_now_unix_secs_is_non_zero_and_reasonable() {
        let now = now_unix_secs();
        // Greater than Jan 1, 2024 (1704067200)
        assert!(now > 1704067200);
    }

    #[test]
    fn test_get_last_update_check_malformed_value_returns_none() {
        let conn = open_test_conn();
        // Insert non-integer value into settings table
        crate::db::crud::upsert_setting(&conn, LAST_UPDATE_CHECK_KEY, "invalid-timestamp").unwrap();
        assert_eq!(get_last_update_check(&conn), None);
    }

    #[test]
    fn test_check_on_startup_disabled_does_not_set_timestamp() {
        let conn = open_test_conn();
        check_on_startup(&conn, false);
        assert_eq!(get_last_update_check(&conn), None);
    }

    #[test]
    fn test_check_on_startup_enabled_updates_timestamp() {
        let conn = open_test_conn();
        assert_eq!(get_last_update_check(&conn), None);
        check_on_startup(&conn, true);
        assert!(get_last_update_check(&conn).is_some());
    }
}
