pub mod calculator;
mod home;

use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerStatKind {
    Snippet,
    Hotkey,
    Script,
    Calculation,
    InlineAi,
}

/// Returns the current date in YYYY-MM-DD format (Local time).
/// Falls back to UTC if the local offset cannot be determined.
pub fn get_current_date_string() -> String {
    // One-second cache mirroring crate::logs::daily_log: the value only needs
    // day precision, so skipping the timezone lookup per stats event is exact
    // except within 1s of midnight.
    static LAST_SECS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static LAST_STR: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if LAST_SECS.load(std::sync::atomic::Ordering::Relaxed) == now_secs
        && let Some(cached) = LAST_STR.get().and_then(|m| m.lock().ok())
        && !cached.is_empty()
    {
        return cached.clone();
    }
    let fresh = get_current_date_string_uncached();
    LAST_SECS.store(now_secs, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut guard) = LAST_STR
        .get_or_init(|| std::sync::Mutex::new(String::new()))
        .lock()
    {
        *guard = fresh.clone();
    }
    fresh
}

fn get_current_date_string_uncached() -> String {
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
pub use home::{HomeStats, MostUsedTrigger, load_home_stats, load_home_stats_with_limit};

#[cfg(test)]
mod tests {
    use super::get_current_date_string;

    #[test]
    fn date_cache_returns_consistent_value_within_ttl() {
        let a = get_current_date_string();
        let b = get_current_date_string();
        assert_eq!(a, b);
        assert_eq!(a.len(), 10);
    }
}
