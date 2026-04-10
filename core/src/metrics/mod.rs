pub mod calculator;

use time::OffsetDateTime;

/// Returns the current date in YYYY-MM-DD format (UTC).
pub fn get_current_date_string() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        now.month() as u8,
        now.day()
    )
}

pub use calculator::calculate_saved_keystrokes;
