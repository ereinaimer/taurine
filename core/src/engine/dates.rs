use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use regex::Regex;
use std::sync::OnceLock;
use time::{OffsetDateTime, UtcOffset};

pub fn preprocess_date_phrase(input: &str) -> String {
    // Strip leading + sign (e.g. "+13 hours" → "13 hours")
    // Convert leading - sign to "ago" suffix (e.g. "-2 days" → "2 days ago")
    let input = if let Some(rest) = input.strip_prefix('-') {
        return format!("{} ago", preprocess_date_phrase(rest.trim()));
    } else {
        input.trim_start_matches('+')
    };
    let mut normalized = input.to_lowercase();

    // Explicit string rewrites for common relative offsets
    normalized = normalized
        .replace("the day before yesterday", "2 days ago")
        .replace("day before yesterday", "2 days ago")
        .replace("the day after tomorrow", "2 days from now")
        .replace("day after tomorrow", "2 days from now")
        .replace("the week before last", "2 weeks ago")
        .replace("week before last", "2 weeks ago")
        .replace("the month before last", "2 months ago")
        .replace("month before last", "2 months ago");

    static RELATIVE_RE: OnceLock<Regex> = OnceLock::new();
    let re = RELATIVE_RE.get_or_init(|| {
        Regex::new(r"\b(a|an|the|one|two|three|four|five|six|seven|eight|nine|ten|\d+)\s+(second|seconds|sec|secs|minute|minutes|min|mins|hour|hours|hr|hrs|day|days|week|weeks|month|months|year|years)\s+(before|after|from)\s+(yesterday|today|tomorrow|now)\b").expect("valid relative date regex")
    });

    let result = re.replace_all(&normalized, |caps: &regex::Captures| {
        let amount_str = &caps[1];
        let unit_str = &caps[2];
        let dir_str = &caps[3];
        let anchor_str = &caps[4];

        let amount = match amount_str {
            "a" | "an" | "the" | "one" => 1,
            "two" => 2,
            "three" => 3,
            "four" => 4,
            "five" => 5,
            "six" => 6,
            "seven" => 7,
            "eight" => 8,
            "nine" => 9,
            "ten" => 10,
            other => other.parse::<i32>().unwrap_or(1),
        };

        let dir_sign = match dir_str {
            "before" => -1,
            _ => 1, // after, from
        };

        let is_subday = unit_str.starts_with("second")
            || unit_str.starts_with("sec")
            || unit_str.starts_with("minute")
            || unit_str.starts_with("min")
            || unit_str.starts_with("hour")
            || unit_str.starts_with("hr");

        if is_subday {
            let (anchor_offset, unit_name) =
                if unit_str.starts_with("second") || unit_str.starts_with("sec") {
                    let offset = match anchor_str {
                        "tomorrow" => 86400,
                        "yesterday" => -86400,
                        _ => 0,
                    };
                    (offset, "seconds")
                } else if unit_str.starts_with("minute") || unit_str.starts_with("min") {
                    let offset = match anchor_str {
                        "tomorrow" => 1440,
                        "yesterday" => -1440,
                        _ => 0,
                    };
                    (offset, "minutes")
                } else {
                    let offset = match anchor_str {
                        "tomorrow" => 24,
                        "yesterday" => -24,
                        _ => 0,
                    };
                    (offset, "hours")
                };

            let total = anchor_offset + (amount * dir_sign);
            if total < 0 {
                format!("{} {} ago", -total, unit_name)
            } else if total > 0 {
                format!("{} {}", total, unit_name)
            } else {
                "now".to_string()
            }
        } else {
            let unit_multiplier = match unit_str {
                "day" | "days" => 1,
                "week" | "weeks" => 7,
                "month" | "months" => 30,
                "year" | "years" => 365,
                _ => 1,
            };

            let anchor_offset = match anchor_str {
                "yesterday" => -1,
                "tomorrow" => 1,
                _ => 0, // today, now
            };

            let total_days = anchor_offset + (amount * unit_multiplier * dir_sign);
            if total_days < 0 {
                format!("{} days ago", -total_days)
            } else if total_days > 0 {
                format!("{} days", total_days)
            } else {
                "today".to_string()
            }
        }
    });

    result.into_owned()
}

pub fn classify_date_expression(expr: &str) -> (bool, bool) {
    let s = expr.to_lowercase();
    if s.trim() == "now" {
        return (true, true);
    }

    let has_time_indicator = s.contains("am")
        || s.contains("pm")
        || s.contains("noon")
        || s.contains("midnight")
        || s.contains("o'clock")
        || s.contains(':')
        || s.contains("hour")
        || s.contains("minute")
        || s.contains("second")
        || s.contains("hr")
        || s.contains("min")
        || s.contains("sec")
        || s.contains("at ");

    let has_date_indicator = s.contains("yesterday")
        || s.contains("today")
        || s.contains("tomorrow")
        || s.contains("day")
        || s.contains("week")
        || s.contains("month")
        || s.contains("year")
        || s.contains("jan")
        || s.contains("feb")
        || s.contains("mar")
        || s.contains("apr")
        || s.contains("may")
        || s.contains("jun")
        || s.contains("jul")
        || s.contains("aug")
        || s.contains("sep")
        || s.contains("oct")
        || s.contains("nov")
        || s.contains("dec")
        || s.contains("mon")
        || s.contains("tue")
        || s.contains("wed")
        || s.contains("thu")
        || s.contains("fri")
        || s.contains("sat")
        || s.contains("sun")
        || s.contains('/')
        || s.contains('-');

    let is_date = has_date_indicator || !has_time_indicator;
    let is_time = has_time_indicator;

    (is_date, is_time)
}

/// Fixed-date holidays (month, day)
const HOLIDAYS: &[(&str, u8, u8)] = &[
    ("christmas", 12, 25),
    ("christmas eve", 12, 24),
    ("new year", 1, 1),
    ("new years", 1, 1),
    ("new year's", 1, 1),
    ("new years eve", 12, 31),
    ("new year's eve", 12, 31),
    ("valentine", 2, 14),
    ("valentine's", 2, 14),
    ("st patrick", 3, 17),
    ("st patricks", 3, 17),
    ("april fool", 4, 1),
    ("april fools", 4, 1),
    ("halloween", 10, 31),
    ("thanksgiving", 11, 1), // Approximate - 4th Thursday in Nov (US)
    ("boxing day", 12, 26),
];

/// Relative anchor targets for "end of X" queries
type AnchorFn = fn(DateTime<Local>) -> DateTime<Local>;
const RELATIVE_ANCHORS: &[(&str, AnchorFn)] = &[
    ("end of day", |dt| {
        dt.with_hour(23)
            .unwrap()
            .with_minute(59)
            .unwrap()
            .with_second(59)
            .unwrap()
    }),
    ("end of today", |dt| {
        dt.with_hour(23)
            .unwrap()
            .with_minute(59)
            .unwrap()
            .with_second(59)
            .unwrap()
    }),
    ("end of week", |dt| {
        let days_until_sunday = (7 - dt.weekday().num_days_from_monday()) % 7;
        (dt + chrono::Duration::days(days_until_sunday as i64))
            .with_hour(23)
            .unwrap()
            .with_minute(59)
            .unwrap()
            .with_second(59)
            .unwrap()
    }),
    ("end of month", |dt| {
        let next_month = if dt.month() == 12 { 1 } else { dt.month() + 1 };
        let next_year = if dt.month() == 12 {
            dt.year() + 1
        } else {
            dt.year()
        };
        Local
            .with_ymd_and_hms(next_year, next_month, 1, 0, 0, 0)
            .unwrap()
            - chrono::Duration::seconds(1)
    }),
    ("end of year", |dt| {
        Local
            .with_ymd_and_hms(dt.year() + 1, 1, 1, 0, 0, 0)
            .unwrap()
            - chrono::Duration::seconds(1)
    }),
];

fn find_holiday(name: &str) -> Option<(u32, u32)> {
    let lower = name.to_lowercase();
    HOLIDAYS.iter().find_map(|(n, m, d)| {
        if lower.contains(n) {
            Some((*m as u32, *d as u32))
        } else {
            None
        }
    })
}

fn find_relative_anchor(name: &str) -> Option<fn(DateTime<Local>) -> DateTime<Local>> {
    let lower = name.to_lowercase();
    RELATIVE_ANCHORS
        .iter()
        .find_map(|(n, f)| if lower.contains(n) { Some(*f) } else { None })
}

fn chrono_to_time(dt: DateTime<Local>) -> Option<OffsetDateTime> {
    let timestamp = dt.timestamp();
    let nanoseconds = dt.timestamp_subsec_nanos();
    time::OffsetDateTime::from_unix_timestamp_nanos(
        (timestamp as i128) * 1_000_000_000 + (nanoseconds as i128),
    )
    .ok()
}

/// Parses "how many <unit> until <target>" countdown queries.
/// Returns formatted duration string like "25 days", "3 weeks", etc.
pub fn parse_countdown_query(expr: &str, preferred_dialect: &str) -> Option<String> {
    let s = expr.to_lowercase().trim().to_string();
    let s = s.trim_end_matches('?').trim();

    static COUNTDOWN_RE: OnceLock<Regex> = OnceLock::new();
    let re = COUNTDOWN_RE.get_or_init(|| {
        Regex::new(r"^how many\s+(days?|weeks?|months?|hours?|minutes?|seconds?)\s+until\s+(.+)$")
            .expect("valid countdown regex")
    });

    let caps = re.captures(s)?;
    let unit = &caps[1];
    let target = caps[2].trim();

    let now = Local::now();

    let target_dt = resolve_countdown_target(target, now, preferred_dialect)?;
    let duration = target_dt.signed_duration_since(now);

    if duration.num_seconds() <= 0 {
        return Some("0".to_string());
    }

    let total_seconds = duration.num_seconds();
    let (value, unit_str) = match unit {
        "second" | "seconds" | "sec" | "secs" => (total_seconds, "seconds"),
        "minute" | "minutes" | "min" | "mins" => (total_seconds / 60, "minutes"),
        "hour" | "hours" | "hr" | "hrs" => (total_seconds / 3600, "hours"),
        "day" | "days" => (total_seconds / 86400, "days"),
        "week" | "weeks" => (total_seconds / 604800, "weeks"),
        "month" | "months" => (total_seconds / 2592000, "months"),
        _ => (total_seconds / 86400, "days"),
    };

    Some(format!("{} {}", value, unit_str))
}

fn resolve_countdown_target(
    target: &str,
    now: DateTime<Local>,
    preferred_dialect: &str,
) -> Option<DateTime<Local>> {
    let target_lower = target.to_lowercase();

    // Holiday?
    if let Some((month, day)) = find_holiday(&target_lower) {
        let year = now.year();
        let dt = Local.with_ymd_and_hms(year, month, day, 0, 0, 0).single()?;
        // If holiday already passed this year, use next year
        if dt < now {
            return Local
                .with_ymd_and_hms(year + 1, month, day, 0, 0, 0)
                .single();
        }
        return Some(dt);
    }

    // Relative anchor (end of day/week/month/year)?
    if let Some(anchor_fn) = find_relative_anchor(&target_lower) {
        return Some(anchor_fn(now));
    }

    // Weekday? (e.g., "next friday", "friday")
    if let Some(dt) = parse_weekday_target(&target_lower, now, preferred_dialect) {
        return Some(dt);
    }

    // Try parsing as a natural date expression (e.g., "june 15", "2024-12-25")
    let cleaned = preprocess_date_phrase(target);
    let primary_dialect = match preferred_dialect {
        "us" => interim::Dialect::Us,
        _ => interim::Dialect::Uk,
    };
    let alt_dialect = match primary_dialect {
        interim::Dialect::Us => interim::Dialect::Uk,
        interim::Dialect::Uk => interim::Dialect::Us,
    };

    interim::parse_date_string(&cleaned, now, primary_dialect)
        .or_else(|_| interim::parse_date_string(&cleaned, now, alt_dialect))
        .ok()
}

fn parse_weekday_target(
    target: &str,
    now: DateTime<Local>,
    _preferred_dialect: &str,
) -> Option<DateTime<Local>> {
    let weekdays = [
        ("monday", 1),
        ("tuesday", 2),
        ("wednesday", 3),
        ("thursday", 4),
        ("friday", 5),
        ("saturday", 6),
        ("sunday", 7),
        ("mon", 1),
        ("tue", 2),
        ("wed", 3),
        ("thu", 4),
        ("fri", 5),
        ("sat", 6),
        ("sun", 7),
    ];

    let (_, weekday_num) = weekdays.iter().find(|(name, _)| target.contains(name))?;
    let is_next = target.starts_with("next ");
    let is_this = target.starts_with("this ");
    let is_coming = target.contains("coming ");

    let current_weekday = now.weekday().num_days_from_monday() as i64 + 1; // 1-7
    let target_weekday = *weekday_num as i64;

    let days_ahead = if is_next || is_coming || is_this {
        (target_weekday - current_weekday + 7) % 7
    } else {
        // Bare weekday - assume next occurrence
        (target_weekday - current_weekday + 7) % 7
    };

    let days_ahead = if days_ahead == 0 && (is_next || is_coming) {
        7
    } else {
        days_ahead
    };
    let target_date = now.date_naive() + chrono::Duration::days(days_ahead);
    Local
        .from_local_datetime(&target_date.and_hms_opt(0, 0, 0).unwrap())
        .single()
}

/// Parses "what is the date <target>" / "what date is it <target>" queries.
/// Returns formatted date string using the configured date format.
pub fn parse_date_query(expr: &str, preferred_dialect: &str) -> Option<String> {
    let s = expr.to_lowercase().trim().to_string();
    let s = s.trim_end_matches('?').trim();

    static DATE_QUERY_RE: OnceLock<Regex> = OnceLock::new();
    let re = DATE_QUERY_RE.get_or_init(|| {
        Regex::new(r"^what'?s?\s+(?:is\s+)?(?:the\s+)?date\s+(?:is it\s+)?(.+)$")
            .expect("valid date query regex")
    });

    let caps = re.captures(s)?;
    let target = caps[1].trim();

    let now = Local::now();

    let target_dt = resolve_date_query_target(target, now, preferred_dialect)?;
    let time_dt = chrono_to_time(target_dt)?;
    let pattern = crate::settings::get_cached_inline_datetime_date_format();
    Some(format_datetime(time_dt, &pattern))
}

fn resolve_date_query_target(
    target: &str,
    now: DateTime<Local>,
    preferred_dialect: &str,
) -> Option<DateTime<Local>> {
    let target_lower = target.to_lowercase();

    // Bare "today", "tomorrow", "yesterday"
    match target_lower.as_str() {
        "today" => {
            return Some(
                now.with_hour(0)
                    .unwrap()
                    .with_minute(0)
                    .unwrap()
                    .with_second(0)
                    .unwrap(),
            );
        }
        "tomorrow" => {
            return Some(
                (now + chrono::Duration::days(1))
                    .with_hour(0)
                    .unwrap()
                    .with_minute(0)
                    .unwrap()
                    .with_second(0)
                    .unwrap(),
            );
        }
        "yesterday" => {
            return Some(
                (now - chrono::Duration::days(1))
                    .with_hour(0)
                    .unwrap()
                    .with_minute(0)
                    .unwrap()
                    .with_second(0)
                    .unwrap(),
            );
        }
        "now" => return Some(now),
        _ => {}
    }

    // Holiday?
    if let Some((month, day)) = find_holiday(&target_lower) {
        let year = now.year();
        let dt = Local.with_ymd_and_hms(year, month, day, 0, 0, 0).single()?;
        if dt < now {
            return Local
                .with_ymd_and_hms(year + 1, month, day, 0, 0, 0)
                .single();
        }
        return Some(dt);
    }

    // Relative anchor?
    if let Some(anchor_fn) = find_relative_anchor(&target_lower) {
        return Some(anchor_fn(now));
    }

    // Weekday?
    if let Some(dt) = parse_weekday_target(&target_lower, now, preferred_dialect) {
        return Some(
            dt.with_hour(0)
                .unwrap()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap(),
        );
    }

    // Try natural date parsing
    let cleaned = preprocess_date_phrase(target);
    let primary_dialect = match preferred_dialect {
        "us" => interim::Dialect::Us,
        _ => interim::Dialect::Uk,
    };
    let alt_dialect = match primary_dialect {
        interim::Dialect::Us => interim::Dialect::Uk,
        interim::Dialect::Uk => interim::Dialect::Us,
    };

    interim::parse_date_string(&cleaned, now, primary_dialect)
        .or_else(|_| interim::parse_date_string(&cleaned, now, alt_dialect))
        .ok()
}

/// Returns true if the expression has an explicit direction signal and should expand.
///
/// Bare relative quantities (`2 days`, `13 hours`), bare times (`3pm`, `18:00`),
/// absolute dates (`2024-06-15`, `April 1`), and bare `now` all return false — they
/// are ambiguous or calendar-anchored and must NOT expand without a direction signal.
/// Note: `now` is allowed in prefix-triggered mode via a separate allowlist in catalog.
pub fn has_expansion_intent(expr: &str) -> bool {
    let s = expr.trim().to_lowercase();

    // Leading + or - are explicit direction signals
    let raw = expr.trim();
    if raw.starts_with('+') || raw.starts_with('-') {
        return true;
    }

    // Bare unambiguous anchor keywords ("now" is excluded: it is only allowed via
    // the trigger-prefix path, not triggerless mode)
    if matches!(s.as_str(), "today" | "yesterday" | "tomorrow") {
        return true;
    }

    // "ago" suffix — explicit past direction
    if s.ends_with(" ago") || s == "ago" {
        return true;
    }

    // "from" + anchor — explicit future direction
    if s.contains(" from now")
        || s.contains(" from today")
        || s.contains(" from tomorrow")
        || s.contains(" from yesterday")
    {
        return true;
    }

    // "before" or "after" + anchor — arithmetic relative to anchor
    let anchors = ["yesterday", "today", "tomorrow", "now", "last"];
    if (s.contains(" before ") || s.contains(" after ")) && anchors.iter().any(|a| s.contains(a)) {
        return true;
    }

    // Special compound phrases (already pre-normalized but check raw too)
    let compounds = [
        "day before yesterday",
        "the day before yesterday",
        "day after tomorrow",
        "the day after tomorrow",
        "week before last",
        "the week before last",
        "month before last",
        "the month before last",
    ];
    if compounds.iter().any(|c| s.contains(c)) {
        return true;
    }

    // "next" or "last" + weekday — has direction
    let weekdays = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
        "mon",
        "tue",
        "wed",
        "thu",
        "fri",
        "sat",
        "sun",
    ];
    if (s.starts_with("next ") || s.starts_with("last ")) && weekdays.iter().any(|d| s.contains(d))
    {
        return true;
    }

    false
}

pub fn parse_natural_date(
    expr: &str,
    preferred_dialect: &str,
) -> Option<(OffsetDateTime, bool, bool)> {
    let cleaned = preprocess_date_phrase(expr);
    let now = chrono::Local::now().fixed_offset();
    let primary_dialect = match preferred_dialect {
        "us" => interim::Dialect::Us,
        _ => interim::Dialect::Uk,
    };
    let alt_dialect = match primary_dialect {
        interim::Dialect::Us => interim::Dialect::Uk,
        interim::Dialect::Uk => interim::Dialect::Us,
    };

    // Try primary dialect first, then fallback to secondary dialect
    let parsed = interim::parse_date_string(&cleaned, now, primary_dialect)
        .or_else(|_| interim::parse_date_string(&cleaned, now, alt_dialect))
        .ok()?;

    let timestamp = parsed.timestamp();
    let nanoseconds = parsed.timestamp_subsec_nanos();
    let offset_dt = time::OffsetDateTime::from_unix_timestamp_nanos(
        (timestamp as i128) * 1_000_000_000 + (nanoseconds as i128),
    )
    .ok()?;

    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let local_dt = offset_dt.to_offset(local_offset);

    let (is_date, is_time) = classify_date_expression(expr);
    Some((local_dt, is_date, is_time))
}

pub fn format_datetime(dt: OffsetDateTime, pattern: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            continue;
        }

        let remaining = &pattern[i..];

        if remaining.starts_with("YYYY") {
            out.push_str(&format!("{:04}", dt.year()));
            i += 4;
        } else if remaining.starts_with("YY") {
            out.push_str(&format!("{:02}", dt.year() % 100));
            i += 2;
        } else if remaining.starts_with("MMMM") {
            out.push_str(&dt.month().to_string());
            i += 4;
        } else if remaining.starts_with("MMM") {
            out.push_str(&dt.month().to_string()[0..3]);
            i += 3;
        } else if remaining.starts_with("MM") {
            out.push_str(&format!("{:02}", u8::from(dt.month())));
            i += 2;
        } else if remaining.starts_with("M") {
            out.push_str(&format!("{}", u8::from(dt.month())));
            i += 1;
        } else if remaining.starts_with("DD") {
            out.push_str(&format!("{:02}", dt.day()));
            i += 2;
        } else if remaining.starts_with("D") {
            out.push_str(&format!("{}", dt.day()));
            i += 1;
        } else if remaining.starts_with("dddd") {
            out.push_str(&dt.weekday().to_string());
            i += 4;
        } else if remaining.starts_with("ddd") {
            out.push_str(&dt.weekday().to_string()[0..3]);
            i += 3;
        } else if remaining.starts_with("HH") {
            out.push_str(&format!("{:02}", dt.hour()));
            i += 2;
        } else if remaining.starts_with("H") {
            out.push_str(&format!("{}", dt.hour()));
            i += 1;
        } else if remaining.starts_with("hh") {
            let h = dt.hour() % 12;
            let h12 = if h == 0 { 12 } else { h };
            out.push_str(&format!("{:02}", h12));
            i += 2;
        } else if remaining.starts_with("h") {
            let h = dt.hour() % 12;
            let h12 = if h == 0 { 12 } else { h };
            out.push_str(&format!("{}", h12));
            i += 1;
        } else if remaining.starts_with("mm") {
            out.push_str(&format!("{:02}", dt.minute()));
            i += 2;
        } else if remaining.starts_with("m") {
            out.push_str(&format!("{}", dt.minute()));
            i += 1;
        } else if remaining.starts_with("ss") {
            out.push_str(&format!("{:02}", dt.second()));
            i += 2;
        } else if remaining.starts_with("s") {
            out.push_str(&format!("{}", dt.second()));
            i += 1;
        } else if remaining.starts_with("A") {
            out.push_str(if dt.hour() >= 12 { "PM" } else { "AM" });
            i += 1;
        } else if remaining.starts_with("a") {
            out.push_str(if dt.hour() >= 12 { "pm" } else { "am" });
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Preprocessing ────────────────────────────────────────────────────────

    #[test]
    fn test_special_phrase_rewrites() {
        assert_eq!(
            preprocess_date_phrase("the day before yesterday"),
            "2 days ago"
        );
        assert_eq!(preprocess_date_phrase("day before yesterday"), "2 days ago");
        assert_eq!(preprocess_date_phrase("the day after tomorrow"), "2 days");
        assert_eq!(preprocess_date_phrase("day after tomorrow"), "2 days");
        assert_eq!(
            preprocess_date_phrase("the week before last"),
            "2 weeks ago"
        );
        assert_eq!(preprocess_date_phrase("week before last"), "2 weeks ago");
        assert_eq!(
            preprocess_date_phrase("the month before last"),
            "2 months ago"
        );
        assert_eq!(preprocess_date_phrase("month before last"), "2 months ago");
    }

    #[test]
    fn test_relative_arithmetic_days() {
        assert_eq!(
            preprocess_date_phrase("two days before yesterday"),
            "3 days ago"
        );
        assert_eq!(
            preprocess_date_phrase("a week before yesterday"),
            "8 days ago"
        );
        assert_eq!(preprocess_date_phrase("3 weeks after tomorrow"), "22 days");
        assert_eq!(preprocess_date_phrase("day before yesterday"), "2 days ago");
        assert_eq!(preprocess_date_phrase("2 days from now"), "2 days");
        assert_eq!(preprocess_date_phrase("1 month after today"), "30 days");
        assert_eq!(preprocess_date_phrase("2 years from now"), "730 days");
        assert_eq!(
            preprocess_date_phrase("1 week before tomorrow"),
            "6 days ago"
        );
    }

    #[test]
    fn test_relative_arithmetic_subday() {
        assert_eq!(preprocess_date_phrase("11 hours from now"), "11 hours");
        assert_eq!(preprocess_date_phrase("11 hours from tomorrow"), "35 hours");
        assert_eq!(
            preprocess_date_phrase("2 hours before yesterday"),
            "26 hours ago"
        ); // -24 + (-2) = -26
        assert_eq!(preprocess_date_phrase("11 hrs from now"), "11 hours");
        assert_eq!(preprocess_date_phrase("3 hr from now"), "3 hours");
        assert_eq!(preprocess_date_phrase("25 minutes from now"), "25 minutes");
        assert_eq!(preprocess_date_phrase("25 mins from now"), "25 minutes");
        assert_eq!(preprocess_date_phrase("5 min from now"), "5 minutes");
        assert_eq!(
            preprocess_date_phrase("30 minutes before now"),
            "30 minutes ago"
        );
        assert_eq!(preprocess_date_phrase("15 seconds from now"), "15 seconds");
        assert_eq!(preprocess_date_phrase("15 secs from now"), "15 seconds");
        assert_eq!(preprocess_date_phrase("45 sec from now"), "45 seconds");
    }

    #[test]
    fn test_prefix_handling() {
        // + is stripped
        assert_eq!(preprocess_date_phrase("+2 days"), "2 days");
        assert_eq!(preprocess_date_phrase("+13 hours"), "13 hours");
        assert_eq!(preprocess_date_phrase("+25 mins from now"), "25 minutes");

        // - is converted to "ago" suffix
        assert_eq!(preprocess_date_phrase("-2 days"), "2 days ago");
        assert_eq!(preprocess_date_phrase("-13 hours"), "13 hours ago");
        assert_eq!(preprocess_date_phrase("-3 weeks"), "3 weeks ago");
    }

    // ─── has_expansion_intent ─────────────────────────────────────────────────

    #[test]
    fn test_expansion_intent_allowed() {
        // Bare anchors (now is NOT in this list — see test_expansion_intent_denied)
        assert!(has_expansion_intent("today"));
        assert!(has_expansion_intent("yesterday"));
        assert!(has_expansion_intent("tomorrow"));

        // Explicit + / - prefix
        assert!(has_expansion_intent("+2 days"));
        assert!(has_expansion_intent("+13 hours"));
        assert!(has_expansion_intent("-2 days"));
        assert!(has_expansion_intent("-3 hours"));

        // "ago" suffix
        assert!(has_expansion_intent("5 days ago"));
        assert!(has_expansion_intent("2 weeks ago"));
        assert!(has_expansion_intent("3 hours ago"));

        // "from now/tomorrow/yesterday"
        assert!(has_expansion_intent("2 days from now"));
        assert!(has_expansion_intent("11 hours from now"));
        assert!(has_expansion_intent("3 weeks from tomorrow"));
        assert!(has_expansion_intent("25 mins from now"));

        // "before/after" + anchor
        assert!(has_expansion_intent("two days before yesterday"));
        assert!(has_expansion_intent("3 weeks after tomorrow"));
        assert!(has_expansion_intent("2 hours before yesterday"));
        assert!(has_expansion_intent("1 week before tomorrow"));

        // Special compound phrases
        assert!(has_expansion_intent("day before yesterday"));
        assert!(has_expansion_intent("week before last"));
        assert!(has_expansion_intent("day after tomorrow"));
        assert!(has_expansion_intent("month before last"));

        // Weekday + direction modifier
        assert!(has_expansion_intent("next friday"));
        assert!(has_expansion_intent("last monday"));
        assert!(has_expansion_intent("next fri"));
        assert!(has_expansion_intent("last mon"));
    }

    #[test]
    fn test_expansion_intent_denied() {
        // Bare relative quantities — ambiguous direction
        assert!(!has_expansion_intent("2 days"));
        assert!(!has_expansion_intent("1 week"));
        assert!(!has_expansion_intent("three weeks"));
        assert!(!has_expansion_intent("13 hours"));
        assert!(!has_expansion_intent("25 mins"));
        assert!(!has_expansion_intent("15 seconds"));
        assert!(!has_expansion_intent("6 months"));

        // Bare times of day — absolute, no direction
        assert!(!has_expansion_intent("3pm"));
        assert!(!has_expansion_intent("3am"));
        assert!(!has_expansion_intent("18:00"));
        assert!(!has_expansion_intent("10:30am"));
        assert!(!has_expansion_intent("6.30pm"));

        // Absolute dates — calendar-anchored, no direction
        assert!(!has_expansion_intent("2024-06-15"));
        assert!(!has_expansion_intent("15/06/2024"));
        assert!(!has_expansion_intent("April 1"));
        assert!(!has_expansion_intent("25 December"));
        assert!(!has_expansion_intent("April 1, 2024"));
        assert!(!has_expansion_intent("1 April 2024"));

        // Bare weekdays/months without modifier
        assert!(!has_expansion_intent("friday"));
        assert!(!has_expansion_intent("monday"));

        // "now" alone is excluded from triggerless mode;
        // it is only allowed in prefix mode via a separate catalog allowlist.
        assert!(!has_expansion_intent("now"));
    }

    // ─── parse_natural_date ───────────────────────────────────────────────────

    #[test]
    fn test_parse_basic_relatives() {
        assert!(parse_natural_date("yesterday", "uk").is_some());
        assert!(parse_natural_date("today", "uk").is_some());
        assert!(parse_natural_date("tomorrow", "uk").is_some());
        assert!(parse_natural_date("now", "uk").is_some());
    }

    #[test]
    fn test_parse_relative_day_offsets() {
        assert!(parse_natural_date("5 days ago", "uk").is_some());
        assert!(parse_natural_date("2 days from now", "uk").is_some());
        assert!(parse_natural_date("2 days from tomorrow", "uk").is_some());
        assert!(parse_natural_date("3 weeks from tomorrow", "uk").is_some());
        assert!(parse_natural_date("2 weeks ago", "uk").is_some());
        assert!(parse_natural_date("3 months ago", "uk").is_some());
        assert!(parse_natural_date("1 year ago", "uk").is_some());
        // - prefix is handled by preprocess_date_phrase
        assert!(parse_natural_date("-2 days", "uk").is_some());
        assert!(parse_natural_date("-3 months", "uk").is_some());
    }

    #[test]
    fn test_parse_subday_time_offsets() {
        assert!(parse_natural_date("11 hours from now", "uk").is_some());
        assert!(parse_natural_date("11 hrs from now", "uk").is_some());
        assert!(parse_natural_date("3 hours ago", "uk").is_some());
        assert!(parse_natural_date("25 mins from now", "uk").is_some());
        assert!(parse_natural_date("10 minutes ago", "uk").is_some());
        assert!(parse_natural_date("15 secs from now", "uk").is_some());
        assert!(parse_natural_date("30 seconds ago", "uk").is_some());
        assert!(parse_natural_date("+13 hours", "uk").is_some());
        assert!(parse_natural_date("+2 days", "uk").is_some());
        assert!(parse_natural_date("-13 hours", "uk").is_some());
        assert!(parse_natural_date("-2 days", "uk").is_some());
    }

    #[test]
    fn test_parse_weekdays() {
        assert!(parse_natural_date("next friday", "uk").is_some());
        assert!(parse_natural_date("last friday", "uk").is_some());
        assert!(parse_natural_date("next monday", "uk").is_some());
        assert!(parse_natural_date("last monday", "uk").is_some());
        assert!(parse_natural_date("next fri", "uk").is_some());
        assert!(parse_natural_date("last mon", "uk").is_some());
        assert!(parse_natural_date("next wed", "uk").is_some());
    }

    #[test]
    fn test_parse_date_and_time_combined() {
        // Space-separated date + time (no "at" keyword support in chrono_english)
        assert!(parse_natural_date("next friday 8pm", "uk").is_some());
        assert!(parse_natural_date("tomorrow 9am", "uk").is_some());
    }

    // ─── classify_date_expression ─────────────────────────────────────────────

    #[test]
    fn test_date_classification_dates_only() {
        assert_eq!(classify_date_expression("next friday"), (true, false));
        assert_eq!(classify_date_expression("last monday"), (true, false));
        assert_eq!(
            classify_date_expression("3 weeks from tomorrow"),
            (true, false)
        );
    }

    #[test]
    fn test_date_classification_times_only() {
        assert_eq!(classify_date_expression("11 hrs from now"), (false, true));
        assert_eq!(classify_date_expression("25 mins from now"), (false, true));
        assert_eq!(classify_date_expression("15 secs from now"), (false, true));
        assert_eq!(classify_date_expression("3 minutes ago"), (false, true));
    }

    #[test]
    fn test_date_classification_both() {
        assert_eq!(classify_date_expression("now"), (true, true));
        assert_eq!(classify_date_expression("next friday 8pm"), (true, true));
        assert_eq!(classify_date_expression("tomorrow 9am"), (true, true));
    }

    // ─── parse_countdown_query ──────────────────────────────────────────────────

    #[test]
    fn test_countdown_holidays() {
        // These test that parsing works; exact values depend on current date
        assert!(parse_countdown_query("how many days until christmas", "uk").is_some());
        assert!(parse_countdown_query("how many days until new year", "uk").is_some());
        assert!(parse_countdown_query("how many days until halloween", "uk").is_some());
        assert!(parse_countdown_query("how many days until valentine", "uk").is_some());
        assert!(parse_countdown_query("how many weeks until christmas", "uk").is_some());
        assert!(parse_countdown_query("how many hours until new year", "uk").is_some());
    }

    #[test]
    fn test_countdown_relative_anchors() {
        assert!(parse_countdown_query("how many hours until end of day", "uk").is_some());
        assert!(parse_countdown_query("how many days until end of week", "uk").is_some());
        assert!(parse_countdown_query("how many days until end of month", "uk").is_some());
        assert!(parse_countdown_query("how many days until end of year", "uk").is_some());
        assert!(parse_countdown_query("how many minutes until end of today", "uk").is_some());
    }

    #[test]
    fn test_countdown_weekdays() {
        assert!(parse_countdown_query("how many days until next friday", "uk").is_some());
        assert!(parse_countdown_query("how many days until friday", "uk").is_some());
        assert!(parse_countdown_query("how many weeks until next monday", "uk").is_some());
    }

    #[test]
    fn test_countdown_natural_dates() {
        assert!(parse_countdown_query("how many days until june 15", "uk").is_some());
        assert!(parse_countdown_query("how many days until 2024-12-25", "uk").is_some());
    }

    #[test]
    fn test_countdown_case_insensitive() {
        assert!(parse_countdown_query("HOW MANY DAYS UNTIL CHRISTMAS", "uk").is_some());
        assert!(parse_countdown_query("How Many Days Until Christmas?", "uk").is_some());
        assert!(parse_countdown_query("how many days until CHRISTMAS?", "uk").is_some());
    }

    // ─── parse_date_query ───────────────────────────────────────────────────────

    #[test]
    fn test_date_query_basic() {
        assert!(parse_date_query("what is the date today", "uk").is_some());
        assert!(parse_date_query("what date is it tomorrow", "uk").is_some());
        assert!(parse_date_query("what is the date yesterday", "uk").is_some());
        assert!(parse_date_query("what date is it now", "uk").is_some());
    }

    #[test]
    fn test_date_query_holidays() {
        assert!(parse_date_query("what is the date christmas", "uk").is_some());
        assert!(parse_date_query("what date is it new year", "uk").is_some());
        assert!(parse_date_query("what is the date halloween", "uk").is_some());
    }

    #[test]
    fn test_date_query_relative_anchors() {
        assert!(parse_date_query("what is the date end of week", "uk").is_some());
        assert!(parse_date_query("what date is it end of month", "uk").is_some());
        assert!(parse_date_query("what is the date end of year", "uk").is_some());
        assert!(parse_date_query("what date is it end of today", "uk").is_some());
    }

    #[test]
    fn test_date_query_weekdays() {
        assert!(parse_date_query("what is the date next friday", "uk").is_some());
        assert!(parse_date_query("what date is it friday", "uk").is_some());
        assert!(parse_date_query("what is the date next monday", "uk").is_some());
    }

    #[test]
    fn test_date_query_natural_dates() {
        assert!(parse_date_query("what is the date june 15", "uk").is_some());
        assert!(parse_date_query("what date is it 2024-12-25", "uk").is_some());
    }

    #[test]
    fn test_date_query_case_insensitive() {
        assert!(parse_date_query("WHAT IS THE DATE TODAY", "uk").is_some());
        assert!(parse_date_query("What Date Is It Tomorrow?", "uk").is_some());
        assert!(parse_date_query("what is the date CHRISTMAS?", "uk").is_some());
    }

    #[test]
    fn test_date_query_variations() {
        assert!(parse_date_query("what's the date today", "uk").is_some());
        assert!(parse_date_query("whats the date tomorrow", "uk").is_some());
    }
}
