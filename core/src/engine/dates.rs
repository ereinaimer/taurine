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
        Regex::new(r"\b(a|an|the|one|two|three|four|five|six|seven|eight|nine|ten|\d+)\s+(second|seconds|sec|secs|minute|minutes|min|mins|hour|hours|hr|hrs|day|days|week|weeks|month|months|year|years)\s+(before|after|from)\s+(yesterday|today|tomorrow|now)\b").unwrap()
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
    let now = chrono::Local::now();
    let primary_dialect = match preferred_dialect {
        "us" => chrono_english::Dialect::Us,
        _ => chrono_english::Dialect::Uk,
    };
    let alt_dialect = match primary_dialect {
        chrono_english::Dialect::Us => chrono_english::Dialect::Uk,
        chrono_english::Dialect::Uk => chrono_english::Dialect::Us,
    };

    // Try primary dialect first, then fallback to secondary dialect
    let parsed = chrono_english::parse_date_string(&cleaned, now, primary_dialect)
        .or_else(|_| chrono_english::parse_date_string(&cleaned, now, alt_dialect))
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
}
