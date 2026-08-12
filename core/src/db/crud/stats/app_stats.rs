use rusqlite::{Connection, Result, params};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AppStatRow {
    pub app_key: String,
    pub date: String,
    pub executions: u64,
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
    pub version: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TopAppStat {
    pub app_key: String,
    pub display_name: String,
    pub executions: u64,
    pub keystrokes_saved: u64,
    pub time_saved_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppStatsSortBy {
    #[default]
    Executions,
    TimeSaved,
    KeystrokesSaved,
}

/// Formats an application display name from an executable key.
///
/// Strips `.exe` or `.app` file extensions, replaces hyphens (`-`) and underscores (`_`)
/// with spaces, and title-cases each word.
///
/// # Examples
/// - `"google-chrome.exe"` $\rightarrow$ `"Google Chrome"`
/// - `"taurine-startup.exe"` $\rightarrow$ `"Taurine Startup"`
/// - `"discord.exe"` $\rightarrow$ `"Discord"`
/// - `"my_custom_app.exe"` $\rightarrow$ `"My Custom App"`
pub fn format_app_display_name(app_key: &str) -> String {
    let stem = std::path::Path::new(app_key)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(app_key);

    let cleaned = stem.replace(['-', '_'], " ");
    cleaned
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn upsert_app_stat_with_conn(
    conn: &Connection,
    app_key: &str,
    date: &str,
    executions: u64,
    keystrokes_saved: u64,
    time_saved_ms: u64,
) -> Result<()> {
    let now = crate::db::now_unix_secs();
    conn.execute(
        "INSERT INTO app_stats (
            app_key, date, executions, keystrokes_saved, time_saved_ms, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?)
        ON CONFLICT(app_key, date) DO UPDATE SET
            executions = app_stats.executions + excluded.executions,
            keystrokes_saved = app_stats.keystrokes_saved + excluded.keystrokes_saved,
            time_saved_ms = app_stats.time_saved_ms + excluded.time_saved_ms,
            updated_at = excluded.updated_at",
        params![
            app_key,
            date,
            executions as i64,
            keystrokes_saved as i64,
            time_saved_ms as i64,
            now
        ],
    )?;
    Ok(())
}

pub fn get_top_app_stats_with_conn(
    conn: &Connection,
    sort_by: AppStatsSortBy,
    limit: usize,
) -> Result<Vec<TopAppStat>> {
    let order_clause = match sort_by {
        AppStatsSortBy::Executions => "SUM(executions) DESC",
        AppStatsSortBy::TimeSaved => "SUM(time_saved_ms) DESC",
        AppStatsSortBy::KeystrokesSaved => "SUM(keystrokes_saved) DESC",
    };

    let sql = format!(
        "SELECT 
            app_key,
            SUM(executions) as executions,
            SUM(keystrokes_saved) as keystrokes_saved,
            SUM(time_saved_ms) as time_saved_ms
        FROM app_stats
        GROUP BY app_key
        ORDER BY {order_clause}
        LIMIT ?"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([limit as i64], |row| {
        let app_key: String = row.get(0)?;
        let executions: i64 = row.get(1)?;
        let keystrokes_saved: i64 = row.get(2)?;
        let time_saved_ms: i64 = row.get(3)?;

        let display_name = format_app_display_name(&app_key);

        Ok(TopAppStat {
            app_key,
            display_name,
            executions: executions.max(0) as u64,
            keystrokes_saved: keystrokes_saved.max(0) as u64,
            time_saved_ms: time_saved_ms.max(0) as u64,
        })
    })?;

    let mut result = Vec::new();
    for r in rows {
        result.push(r?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init::migrate::run_migrations;

    #[test]
    fn test_format_app_display_name_option1() {
        assert_eq!(
            format_app_display_name("google-chrome.exe"),
            "Google Chrome"
        );
        assert_eq!(
            format_app_display_name("taurine-startup.exe"),
            "Taurine Startup"
        );
        assert_eq!(format_app_display_name("discord.exe"), "Discord");
        assert_eq!(
            format_app_display_name("my_custom_app.exe"),
            "My Custom App"
        );
        assert_eq!(format_app_display_name("code.exe"), "Code");
    }

    #[test]
    fn test_upsert_and_get_top_app_stats() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        upsert_app_stat_with_conn(&conn, "google-chrome.exe", "2026-08-12", 10, 100, 5000).unwrap();
        upsert_app_stat_with_conn(&conn, "google-chrome.exe", "2026-08-12", 5, 50, 2500).unwrap();
        upsert_app_stat_with_conn(&conn, "code.exe", "2026-08-12", 20, 200, 1000).unwrap();

        // Query by executions DESC -> code.exe (20) first, google-chrome.exe (15) second
        let top_execs = get_top_app_stats_with_conn(&conn, AppStatsSortBy::Executions, 10).unwrap();
        assert_eq!(top_execs.len(), 2);
        assert_eq!(top_execs[0].app_key, "code.exe");
        assert_eq!(top_execs[0].display_name, "Code");
        assert_eq!(top_execs[0].executions, 20);

        assert_eq!(top_execs[1].app_key, "google-chrome.exe");
        assert_eq!(top_execs[1].display_name, "Google Chrome");
        assert_eq!(top_execs[1].executions, 15);
        assert_eq!(top_execs[1].keystrokes_saved, 150);
        assert_eq!(top_execs[1].time_saved_ms, 7500);

        // Query by time_saved DESC -> google-chrome.exe (7500ms) first, code.exe (1000ms) second
        let top_time = get_top_app_stats_with_conn(&conn, AppStatsSortBy::TimeSaved, 10).unwrap();
        assert_eq!(top_time[0].app_key, "google-chrome.exe");
        assert_eq!(top_time[1].app_key, "code.exe");
    }
}
