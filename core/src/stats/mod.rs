pub mod calculator;
mod home;

use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationStatKind {
    Snippet,
    Hotkey,
    Script,
    Calculation,
    InlineAi,
}

/// Returns the current date in YYYY-MM-DD format (Local time).
/// Falls back to UTC if the local offset cannot be determined.
pub fn get_current_date_string() -> String {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        now.month() as u8,
        now.day()
    )
}

pub use calculator::{
    ExpansionStats, calculate_expansion_stats, calculate_saved_keystrokes, calculate_time_saved_ms,
};
pub use home::{HomeStats, MostUsedAutomation, load_home_stats, load_home_stats_with_limit};
