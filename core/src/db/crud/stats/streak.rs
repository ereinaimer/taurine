use rusqlite::{Connection, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct StreakInfo {
    pub current_streak: u32,
    pub longest_streak: u32,
}

pub fn parse_date_str(s: &str) -> Option<time::Date> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return None;
    }
    let year: i32 = parts[0].parse().ok()?;
    let month_num: u8 = parts[1].parse().ok()?;
    let day: u8 = parts[2].parse().ok()?;
    let month = time::Month::try_from(month_num).ok()?;
    time::Date::from_calendar_date(year, month, day).ok()
}

/// Computes execution streak statistics by querying consecutive active dates.
///
/// An active date is any calendar day with at least one recorded action in `stats`
/// or `activity_events`.
/// - `current_streak`: Number of consecutive active days up to today or yesterday.
/// - `longest_streak`: Maximum consecutive active days across all historical records.
pub fn calculate_streaks(conn: &Connection, anchor_date: Option<time::Date>) -> Result<StreakInfo> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT date FROM stats WHERE (executions + ai_executions + voice_executions) > 0
         UNION
         SELECT DISTINCT date FROM activity_events
         ORDER BY date ASC",
    )?;

    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut active_dates: Vec<time::Date> = Vec::new();
    for r in rows {
        let date_str = r?;
        if let Some(date) = parse_date_str(&date_str)
            && active_dates.last() != Some(&date)
        {
            active_dates.push(date);
        }
    }

    if active_dates.is_empty() {
        return Ok(StreakInfo::default());
    }

    // Calculate longest streak across all history
    let mut longest_streak: u32 = 0;
    let mut current_run: u32 = 0;
    let mut prev_date: Option<time::Date> = None;

    for &date in &active_dates {
        if let Some(prev) = prev_date
            && prev.next_day() == Some(date)
        {
            current_run = current_run.saturating_add(1);
        } else {
            current_run = 1;
        }
        longest_streak = longest_streak.max(current_run);
        prev_date = Some(date);
    }

    // Calculate current streak
    let today = anchor_date.unwrap_or_else(|| {
        time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .date()
    });

    let last_active = *active_dates.last().unwrap();
    let yesterday = today.previous_day();

    let current_streak = if last_active == today || yesterday == Some(last_active) {
        let mut streak: u32 = 0;
        let mut expected = last_active;
        for &d in active_dates.iter().rev() {
            if d == expected {
                streak = streak.saturating_add(1);
                if let Some(prev) = expected.previous_day() {
                    expected = prev;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        streak
    } else {
        0
    };

    Ok(StreakInfo {
        current_streak,
        longest_streak,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::init::schema::ensure_schema;
    use time::Month;

    #[test]
    fn test_empty_database_returns_zero_streaks() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let streaks = calculate_streaks(&conn, None).unwrap();
        assert_eq!(streaks.current_streak, 0);
        assert_eq!(streaks.longest_streak, 0);
    }

    #[test]
    fn test_streak_calculation_active_today() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let today = time::Date::from_calendar_date(2026, Month::October, 26).unwrap();
        let day_minus_1 = time::Date::from_calendar_date(2026, Month::October, 25).unwrap();
        let day_minus_2 = time::Date::from_calendar_date(2026, Month::October, 24).unwrap();

        // Insert stats for Oct 24, Oct 25, Oct 26
        for d in &[day_minus_2, day_minus_1, today] {
            conn.execute(
                "INSERT INTO stats (date, executions, updated_at) VALUES (?1, 5, unixepoch())",
                [format!(
                    "{:04}-{:02}-{:02}",
                    d.year(),
                    d.month() as u8,
                    d.day()
                )],
            )
            .unwrap();
        }

        let streaks = calculate_streaks(&conn, Some(today)).unwrap();
        assert_eq!(streaks.current_streak, 3);
        assert_eq!(streaks.longest_streak, 3);
    }

    #[test]
    fn test_streak_alive_when_active_yesterday() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let today = time::Date::from_calendar_date(2026, Month::October, 26).unwrap();
        let yesterday = time::Date::from_calendar_date(2026, Month::October, 25).unwrap();
        let two_days_ago = time::Date::from_calendar_date(2026, Month::October, 24).unwrap();

        // Active 24th & 25th, not yet today (26th)
        for d in &[two_days_ago, yesterday] {
            conn.execute(
                "INSERT INTO stats (date, executions, updated_at) VALUES (?1, 5, unixepoch())",
                [format!(
                    "{:04}-{:02}-{:02}",
                    d.year(),
                    d.month() as u8,
                    d.day()
                )],
            )
            .unwrap();
        }

        let streaks = calculate_streaks(&conn, Some(today)).unwrap();
        assert_eq!(streaks.current_streak, 2);
        assert_eq!(streaks.longest_streak, 2);
    }

    #[test]
    fn test_streak_broken_when_inactive_yesterday() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let today = time::Date::from_calendar_date(2026, Month::October, 26).unwrap();
        let four_days_ago = time::Date::from_calendar_date(2026, Month::October, 22).unwrap();
        let three_days_ago = time::Date::from_calendar_date(2026, Month::October, 23).unwrap();

        // 22nd & 23rd were active, but 24th & 25th were inactive
        for d in &[four_days_ago, three_days_ago] {
            conn.execute(
                "INSERT INTO stats (date, executions, updated_at) VALUES (?1, 5, unixepoch())",
                [format!(
                    "{:04}-{:02}-{:02}",
                    d.year(),
                    d.month() as u8,
                    d.day()
                )],
            )
            .unwrap();
        }

        let streaks = calculate_streaks(&conn, Some(today)).unwrap();
        assert_eq!(streaks.current_streak, 0);
        assert_eq!(streaks.longest_streak, 2);
    }

    #[test]
    fn test_longest_streak_exceeds_current_streak() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let today = time::Date::from_calendar_date(2026, Month::October, 26).unwrap();

        // Past 5-day streak: Oct 1 - Oct 5
        for day in 1..=5 {
            let d = time::Date::from_calendar_date(2026, Month::October, day).unwrap();
            conn.execute(
                "INSERT INTO stats (date, executions, updated_at) VALUES (?1, 2, unixepoch())",
                [format!(
                    "{:04}-{:02}-{:02}",
                    d.year(),
                    d.month() as u8,
                    d.day()
                )],
            )
            .unwrap();
        }

        // Active only today Oct 26
        conn.execute(
            "INSERT INTO stats (date, executions, updated_at) VALUES (?1, 1, unixepoch())",
            [format!(
                "{:04}-{:02}-{:02}",
                today.year(),
                today.month() as u8,
                today.day()
            )],
        )
        .unwrap();

        let streaks = calculate_streaks(&conn, Some(today)).unwrap();
        assert_eq!(streaks.current_streak, 1);
        assert_eq!(streaks.longest_streak, 5);
    }
}
