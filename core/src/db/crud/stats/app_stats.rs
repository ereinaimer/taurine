use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppStatRow {
    pub app_key: String,
    pub date: String,
    pub executions: u64,
    pub words_spoken: u64,
    pub words_expanded: u64,
    pub time_saved_ms: u64,
    pub version: u64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopAppStat {
    pub app_key: String,
    pub display_name: String,
    pub executions: u64,
    pub words_spoken: u64,
    pub words_expanded: u64,
    pub time_saved_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppStatsSortBy {
    #[default]
    Executions,
    TimeSaved,
    WordsExpanded,
    WordsSpoken,
}

impl AppStatsSortBy {
    pub const ALL: [Self; 4] = [
        Self::Executions,
        Self::TimeSaved,
        Self::WordsExpanded,
        Self::WordsSpoken,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Executions => "executions",
            Self::TimeSaved => "time_saved",
            Self::WordsExpanded => "words_expanded",
            Self::WordsSpoken => "words_spoken",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Executions => "Executions",
            Self::TimeSaved => "Time Saved",
            Self::WordsExpanded => "Words Expanded",
            Self::WordsSpoken => "Words Spoken",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "executions" | "exec" | "execs" => Some(Self::Executions),
            "time_saved" | "time" => Some(Self::TimeSaved),
            "words_expanded" | "expanded" | "words" => Some(Self::WordsExpanded),
            "words_spoken" | "spoken" => Some(Self::WordsSpoken),
            _ => None,
        }
    }
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
    words_spoken: u64,
    words_expanded: u64,
    time_saved_ms: u64,
) -> Result<()> {
    let now = crate::db::now_unix_secs();
    conn.execute(
        "INSERT INTO app_stats (
            app_key, date, executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, 1, ?)
        ON CONFLICT(app_key, date) DO UPDATE SET
            executions       = app_stats.executions + excluded.executions,
            words_spoken     = app_stats.words_spoken + excluded.words_spoken,
            words_expanded   = app_stats.words_expanded + excluded.words_expanded,
            time_saved_ms    = app_stats.time_saved_ms + excluded.time_saved_ms,
            version          = app_stats.version + 1,
            updated_at       = excluded.updated_at",
        params![
            app_key,
            date,
            executions as i64,
            words_spoken as i64,
            words_expanded as i64,
            time_saved_ms as i64,
            now
        ],
    )?;

    let entity_id = format!("{app_key}:{date}");
    let _ = crate::db::crud::enqueue_outbox(
        conn,
        "app_stat",
        &entity_id,
        "upsert",
        serde_json::json!({ "app_key": app_key, "date": date }),
    );

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
        AppStatsSortBy::WordsExpanded => "SUM(words_expanded) DESC",
        AppStatsSortBy::WordsSpoken => "SUM(words_spoken) DESC",
    };

    let sql = format!(
        "SELECT 
            app_key,
            SUM(executions) as executions,
            SUM(words_spoken) as words_spoken,
            SUM(words_expanded) as words_expanded,
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
        let words_spoken: i64 = row.get(2)?;
        let words_expanded: i64 = row.get(3)?;
        let time_saved_ms: i64 = row.get(4)?;

        let display_name = format_app_display_name(&app_key);

        Ok(TopAppStat {
            app_key,
            display_name,
            executions: executions.max(0) as u64,
            words_spoken: words_spoken.max(0) as u64,
            words_expanded: words_expanded.max(0) as u64,
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
    use crate::db::init::schema::ensure_schema;

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
        ensure_schema(&conn).unwrap();

        upsert_app_stat_with_conn(&conn, "google-chrome.exe", "2026-08-12", 10, 0, 100, 5000)
            .unwrap();
        upsert_app_stat_with_conn(&conn, "google-chrome.exe", "2026-08-12", 5, 0, 50, 2500)
            .unwrap();
        upsert_app_stat_with_conn(&conn, "code.exe", "2026-08-12", 20, 0, 200, 1000).unwrap();

        // Query by executions DESC -> code.exe (20) first, google-chrome.exe (15) second
        let top_execs = get_top_app_stats_with_conn(&conn, AppStatsSortBy::Executions, 10).unwrap();
        assert_eq!(top_execs.len(), 2);
        assert_eq!(top_execs[0].app_key, "code.exe");
        assert_eq!(top_execs[0].display_name, "Code");
        assert_eq!(top_execs[0].executions, 20);

        assert_eq!(top_execs[1].app_key, "google-chrome.exe");
        assert_eq!(top_execs[1].display_name, "Google Chrome");
        assert_eq!(top_execs[1].executions, 15);
        assert_eq!(top_execs[1].words_expanded, 150);
        assert_eq!(top_execs[1].time_saved_ms, 7500);

        // Query by time_saved DESC -> google-chrome.exe (7500ms) first, code.exe (1000ms) second
        let top_time = get_top_app_stats_with_conn(&conn, AppStatsSortBy::TimeSaved, 10).unwrap();
        assert_eq!(top_time[0].app_key, "google-chrome.exe");
        assert_eq!(top_time[1].app_key, "code.exe");
    }

    #[test]
    fn test_app_stats_sort_by_roundtrip() {
        for sort_by in AppStatsSortBy::ALL {
            let label = sort_by.as_str();
            assert_eq!(AppStatsSortBy::parse_str(label), Some(sort_by));
        }
    }

    #[test]
    fn test_app_stats_sort_by_parse_aliases() {
        assert_eq!(
            AppStatsSortBy::parse_str("exec"),
            Some(AppStatsSortBy::Executions)
        );
        assert_eq!(
            AppStatsSortBy::parse_str("time"),
            Some(AppStatsSortBy::TimeSaved)
        );
        assert_eq!(
            AppStatsSortBy::parse_str("words"),
            Some(AppStatsSortBy::WordsExpanded)
        );
        assert_eq!(
            AppStatsSortBy::parse_str("spoken"),
            Some(AppStatsSortBy::WordsSpoken)
        );
        assert_eq!(AppStatsSortBy::parse_str("invalid"), None);
    }
}
