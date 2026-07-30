use chrono::{DateTime, NaiveTime, TimeZone, Timelike};
use chrono_tz::Tz;
use regex::Regex;
use std::sync::OnceLock;

fn has_time_pattern(input: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b\d{1,2}(:\d{2})?\s*(am|pm|a\.m\.|p\.m\.|noon|midnight)\b|\b\d{1,2}:\d{2}\b",
        )
        .unwrap()
    });
    re.is_match(input)
}

fn parse_time_str(s: &str) -> Option<NaiveTime> {
    let s = s.trim().to_lowercase();
    if s == "noon" || s == "12pm" || s == "12:00pm" || s == "12:00 pm" {
        return NaiveTime::from_hms_opt(12, 0, 0);
    }
    if s == "midnight" || s == "12am" || s == "12:00am" || s == "12:00 am" {
        return NaiveTime::from_hms_opt(0, 0, 0);
    }

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)^(\d{1,2})(?::(\d{2}))?\s*(am|pm|a\.m\.|p\.m\.)?\s*$").unwrap()
    });
    if let Some(caps) = re.captures(&s) {
        let hour: u32 = caps[1].parse().ok()?;
        let minute: u32 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let is_pm = caps.get(3).is_some_and(|m| {
            let m = m.as_str().to_lowercase();
            m == "pm" || m == "p.m." || m == "p.m" || m == "pm."
        });

        let h24 = if is_pm && hour != 12 {
            hour + 12
        } else if !is_pm && hour == 12 {
            0
        } else {
            hour
        };

        return NaiveTime::from_hms_opt(h24, minute, 0);
    }

    static RE24: OnceLock<Regex> = OnceLock::new();
    let re24 = RE24.get_or_init(|| Regex::new(r"^(\d{1,2}):(\d{2})\s*$").unwrap());
    if let Some(caps) = re24.captures(&s) {
        let hour: u32 = caps[1].parse().ok()?;
        let minute: u32 = caps[2].parse().ok()?;
        return NaiveTime::from_hms_opt(hour, minute, 0);
    }

    None
}

fn format_time(t: &chrono::NaiveTime, time_format: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = time_format.chars().collect();
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
        let remaining = &time_format[i..];
        if remaining.starts_with("HH") {
            out.push_str(&format!("{:02}", t.hour()));
            i += 2;
        } else if remaining.starts_with("H") {
            out.push_str(&format!("{}", t.hour()));
            i += 1;
        } else if remaining.starts_with("hh") {
            let h = t.hour() % 12;
            let h12 = if h == 0 { 12 } else { h };
            out.push_str(&format!("{:02}", h12));
            i += 2;
        } else if remaining.starts_with("h") {
            let h = t.hour() % 12;
            let h12 = if h == 0 { 12 } else { h };
            out.push_str(&format!("{}", h12));
            i += 1;
        } else if remaining.starts_with("mm") {
            out.push_str(&format!("{:02}", t.minute()));
            i += 2;
        } else if remaining.starts_with("m") {
            out.push_str(&format!("{}", t.minute()));
            i += 1;
        } else if remaining.starts_with("ss") {
            out.push_str(&format!("{:02}", t.second()));
            i += 2;
        } else if remaining.starts_with("s") {
            out.push_str(&format!("{}", t.second()));
            i += 1;
        } else if remaining.starts_with("A") {
            out.push_str(if t.hour() >= 12 { "PM" } else { "AM" });
            i += 1;
        } else if remaining.starts_with("a") {
            out.push_str(if t.hour() >= 12 { "pm" } else { "am" });
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn chrono_dt_to_formatted(dt: &DateTime<Tz>, time_format: &str) -> String {
    format_time(&dt.naive_local().time(), time_format)
}

fn resolve_to_tz(name: &str) -> Option<Tz> {
    let trimmed = name.trim().to_lowercase();
    match trimmed.as_str() {
        // Abbreviations
        "utc" => Some(chrono_tz::UTC),
        "gmt" => Some(chrono_tz::Europe::London),
        "est" | "edt" => Some(chrono_tz::America::New_York),
        "cst" | "cdt" => Some(chrono_tz::America::Chicago),
        "mst" | "mdt" => Some(chrono_tz::America::Denver),
        "pst" | "pdt" => Some(chrono_tz::America::Los_Angeles),
        "ist" => Some(chrono_tz::Asia::Kolkata),
        "jst" => Some(chrono_tz::Asia::Tokyo),
        "cet" | "cest" => Some(chrono_tz::Europe::Paris),
        "eet" | "eest" => Some(chrono_tz::Europe::Helsinki),
        "aest" | "aedt" => Some(chrono_tz::Australia::Sydney),
        "awst" => Some(chrono_tz::Australia::Perth),
        "nzst" | "nzdt" => Some(chrono_tz::Pacific::Auckland),
        "hkt" => Some(chrono_tz::Asia::Hong_Kong),
        "sgt" => Some(chrono_tz::Asia::Singapore),
        "msk" => Some(chrono_tz::Europe::Moscow),
        "gst" => Some(chrono_tz::Asia::Dubai),
        "hst" => Some(chrono_tz::Pacific::Honolulu),
        "akst" | "akdt" => Some(chrono_tz::America::Anchorage),
        "pkt" => Some(chrono_tz::Asia::Karachi),
        "bst" => Some(chrono_tz::Europe::London),
        "west" | "wet" => Some(chrono_tz::Europe::Lisbon),
        // Asia
        "tokyo" => Some(chrono_tz::Asia::Tokyo),
        "dubai" => Some(chrono_tz::Asia::Dubai),
        "mumbai" | "delhi" | "kolkata" | "bangalore" | "chennai" => Some(chrono_tz::Asia::Kolkata),
        "shanghai" | "beijing" => Some(chrono_tz::Asia::Shanghai),
        "hong kong" => Some(chrono_tz::Asia::Hong_Kong),
        "singapore" => Some(chrono_tz::Asia::Singapore),
        "seoul" => Some(chrono_tz::Asia::Seoul),
        "bangkok" => Some(chrono_tz::Asia::Bangkok),
        "jakarta" => Some(chrono_tz::Asia::Jakarta),
        "manila" => Some(chrono_tz::Asia::Manila),
        "taipei" => Some(chrono_tz::Asia::Taipei),
        "karachi" => Some(chrono_tz::Asia::Karachi),
        "dhaka" => Some(chrono_tz::Asia::Dhaka),
        "riyadh" => Some(chrono_tz::Asia::Riyadh),
        "doha" => Some(chrono_tz::Asia::Qatar),
        "kuwait" => Some(chrono_tz::Asia::Kuwait),
        "muscat" => Some(chrono_tz::Asia::Muscat),
        "istanbul" => Some(chrono_tz::Europe::Istanbul),
        "tel aviv" | "jerusalem" => Some(chrono_tz::Asia::Jerusalem),
        "hanoi" | "ho chi minh" | "saigon" => Some(chrono_tz::Asia::Ho_Chi_Minh),
        // Europe
        "london" => Some(chrono_tz::Europe::London),
        "paris" => Some(chrono_tz::Europe::Paris),
        "berlin" | "munich" | "hamburg" => Some(chrono_tz::Europe::Berlin),
        "rome" | "milan" => Some(chrono_tz::Europe::Rome),
        "madrid" | "barcelona" => Some(chrono_tz::Europe::Madrid),
        "moscow" => Some(chrono_tz::Europe::Moscow),
        "amsterdam" => Some(chrono_tz::Europe::Amsterdam),
        "stockholm" => Some(chrono_tz::Europe::Stockholm),
        "oslo" => Some(chrono_tz::Europe::Oslo),
        "copenhagen" => Some(chrono_tz::Europe::Copenhagen),
        "zurich" => Some(chrono_tz::Europe::Zurich),
        "vienna" => Some(chrono_tz::Europe::Vienna),
        "prague" => Some(chrono_tz::Europe::Prague),
        "warsaw" => Some(chrono_tz::Europe::Warsaw),
        "budapest" => Some(chrono_tz::Europe::Budapest),
        "athens" => Some(chrono_tz::Europe::Athens),
        "helsinki" => Some(chrono_tz::Europe::Helsinki),
        "lisbon" => Some(chrono_tz::Europe::Lisbon),
        "dublin" => Some(chrono_tz::Europe::Dublin),
        "brussels" => Some(chrono_tz::Europe::Brussels),
        "kyiv" | "kiev" => Some(chrono_tz::Europe::Kyiv),
        // North America
        "new york" | "nyc" | "newyork" => Some(chrono_tz::America::New_York),
        "los angeles" | "la" => Some(chrono_tz::America::Los_Angeles),
        "chicago" => Some(chrono_tz::America::Chicago),
        "toronto" => Some(chrono_tz::America::Toronto),
        "vancouver" => Some(chrono_tz::America::Vancouver),
        "san francisco" | "sf" => Some(chrono_tz::America::Los_Angeles),
        "seattle" => Some(chrono_tz::America::Los_Angeles),
        "boston" => Some(chrono_tz::America::New_York),
        "washington" | "dc" => Some(chrono_tz::America::New_York),
        "denver" => Some(chrono_tz::America::Denver),
        "phoenix" => Some(chrono_tz::America::Phoenix),
        "halifax" => Some(chrono_tz::America::Halifax),
        "anchorage" => Some(chrono_tz::America::Anchorage),
        "mexico city" => Some(chrono_tz::America::Mexico_City),
        "montreal" => Some(chrono_tz::America::Montreal),
        "miami" | "atlanta" => Some(chrono_tz::America::New_York),
        "houston" | "dallas" => Some(chrono_tz::America::Chicago),
        "las vegas" | "portland" => Some(chrono_tz::America::Los_Angeles),
        // South America
        "buenos aires" => Some(chrono_tz::America::Argentina::Buenos_Aires),
        "sao paulo" | "rio de janeiro" | "rio" => Some(chrono_tz::America::Sao_Paulo),
        "santiago" => Some(chrono_tz::America::Santiago),
        "lima" => Some(chrono_tz::America::Lima),
        "bogota" => Some(chrono_tz::America::Bogota),
        // Oceania
        "sydney" => Some(chrono_tz::Australia::Sydney),
        "melbourne" => Some(chrono_tz::Australia::Melbourne),
        "brisbane" => Some(chrono_tz::Australia::Brisbane),
        "perth" => Some(chrono_tz::Australia::Perth),
        "auckland" | "wellington" => Some(chrono_tz::Pacific::Auckland),
        "honolulu" => Some(chrono_tz::Pacific::Honolulu),
        "fiji" => Some(chrono_tz::Pacific::Fiji),
        // Africa
        "cairo" => Some(chrono_tz::Africa::Cairo),
        "nairobi" => Some(chrono_tz::Africa::Nairobi),
        "johannesburg" | "cape town" => Some(chrono_tz::Africa::Johannesburg),
        "lagos" => Some(chrono_tz::Africa::Lagos),
        "casablanca" => Some(chrono_tz::Africa::Casablanca),
        _ => trimmed.parse::<Tz>().ok(),
    }
}

pub fn resolve_timezone(name: &str) -> Option<Tz> {
    resolve_to_tz(name)
}

pub fn parse_timezone_expression(input: &str, time_format: &str, dialect: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    if has_time_pattern(trimmed) {
        parse_conversion(trimmed, time_format)
    } else if let Some(result) = parse_timezone_relative(trimmed, time_format, dialect) {
        Some(result)
    } else {
        parse_current_time(trimmed, time_format)
    }
}

fn parse_current_time(input: &str, time_format: &str) -> Option<String> {
    let lower = input.to_lowercase();

    let city = if let Some(city) = lower.strip_prefix("time in ") {
        city.trim()
    } else if let Some(city) = lower.strip_prefix("now in ") {
        city.trim()
    } else if let Some(city) = lower.strip_suffix(" time") {
        city.trim()
    } else {
        lower.strip_suffix(" now")?.trim()
    };

    if city.is_empty() {
        return None;
    }

    let tz = resolve_to_tz(city)?;
    let now: DateTime<Tz> = chrono::Utc::now().with_timezone(&tz);
    Some(chrono_dt_to_formatted(&now, time_format))
}

fn parse_conversion(input: &str, time_format: &str) -> Option<String> {
    let lower = input.to_lowercase();
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(
            r"^(\d{1,2}(?::\d{2})?\s*(?:am|pm|a\.m\.|p\.m\.|noon|midnight)?)\s+(.+?)\s+(?:to|in)\s+(.+?)$",
        )
        .unwrap()
    });
    let caps = re.captures(&lower)?;
    let time_str = caps.get(1)?.as_str().trim();
    let from_tz_str = caps.get(2)?.as_str().trim();
    let to_tz_str = caps.get(3)?.as_str().trim();

    if time_str.is_empty() || from_tz_str.is_empty() || to_tz_str.is_empty() {
        return None;
    }

    let time = parse_time_str(time_str)?;
    let from_tz = resolve_to_tz(from_tz_str)?;
    let to_tz = resolve_to_tz(to_tz_str)?;

    let today_utc = chrono::Utc::now().date_naive();
    let from_dt = from_tz
        .from_local_datetime(&today_utc.and_time(time))
        .earliest()?;

    let to_dt = from_dt.with_timezone(&to_tz);
    let formatted = chrono_dt_to_formatted(&to_dt, time_format);

    let from_date = from_dt.naive_local().date();
    let to_date = to_dt.naive_local().date();
    let day_diff = (to_date - from_date).num_days();

    let result = if day_diff == 0 {
        formatted
    } else if day_diff == 1 {
        format!("{formatted} (+1)")
    } else if day_diff == -1 {
        format!("{formatted} (-1)")
    } else if day_diff > 0 {
        format!("{formatted} (+{day_diff})")
    } else {
        format!("{formatted} ({day_diff})")
    };

    Some(result)
}

fn parse_timezone_relative(input: &str, time_format: &str, dialect: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    if let Some(idx) = lower.rfind(" in ") {
        let relative_expr = trimmed[..idx].trim();
        let city = trimmed[idx + 4..].trim();
        if !relative_expr.is_empty()
            && !city.is_empty()
            && let Some(tz) = resolve_to_tz(city)
        {
            return apply_relative_with_tz(relative_expr, tz, time_format, dialect);
        }
    }

    if let Some(space_idx) = trimmed.find(' ') {
        let first = &trimmed[..space_idx];
        let rest = trimmed[space_idx + 1..].trim();
        if !rest.is_empty()
            && let Some(tz) = resolve_to_tz(first)
        {
            return apply_relative_with_tz(rest, tz, time_format, dialect);
        }
    }

    None
}

fn apply_relative_with_tz(
    relative_expr: &str,
    tz: Tz,
    time_format: &str,
    dialect: &str,
) -> Option<String> {
    use chrono_english::parse_date_string;

    let cleaned = crate::engine::dates::preprocess_date_phrase(relative_expr);
    let now = chrono::Local::now();
    let primary_dialect = match dialect {
        "us" => chrono_english::Dialect::Us,
        _ => chrono_english::Dialect::Uk,
    };
    let parsed = parse_date_string(&cleaned, now, primary_dialect).ok()?;

    let target_dt = parsed.with_timezone(&tz);
    let formatted = chrono_dt_to_formatted(&target_dt, time_format);

    let source_date = parsed.naive_local().date();
    let target_date = target_dt.naive_local().date();
    let day_diff = (target_date - source_date).num_days();

    let result = if day_diff == 0 {
        formatted
    } else if day_diff == 1 {
        format!("{formatted} (+1)")
    } else if day_diff == -1 {
        format!("{formatted} (-1)")
    } else if day_diff > 0 {
        format!("{formatted} (+{day_diff})")
    } else {
        format!("{formatted} ({day_diff})")
    };

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::{America, Asia, Europe, UTC};

    fn fmt_opt(r: Option<String>) -> String {
        r.unwrap_or_else(|| "NONE".to_string())
    }

    // --- City/abbrev resolution ---

    #[test]
    fn test_resolve_major_cities() {
        assert_eq!(resolve_timezone("tokyo"), Some(Asia::Tokyo));
        assert_eq!(resolve_timezone("dubai"), Some(Asia::Dubai));
        assert_eq!(resolve_timezone("london"), Some(Europe::London));
        assert_eq!(resolve_timezone("paris"), Some(Europe::Paris));
        assert_eq!(resolve_timezone("new york"), Some(America::New_York));
    }

    #[test]
    fn test_resolve_abbreviations() {
        assert_eq!(resolve_timezone("pst"), Some(America::Los_Angeles));
        assert_eq!(resolve_timezone("est"), Some(America::New_York));
        assert_eq!(resolve_timezone("ist"), Some(Asia::Kolkata));
        assert_eq!(resolve_timezone("utc"), Some(UTC));
        assert_eq!(resolve_timezone("jst"), Some(Asia::Tokyo));
    }

    #[test]
    fn test_resolve_unknown_city() {
        assert_eq!(resolve_timezone("asdfgh"), None);
        assert_eq!(resolve_timezone(""), None);
    }

    // --- Expression classification ---

    #[test]
    fn test_detect_current_time_expr() {
        let out = parse_timezone_expression("time in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'time in tokyo' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("now in dubai", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'now in dubai' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("tokyo time", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'tokyo time' should be recognized: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_detect_conversion_expr() {
        let out = parse_timezone_expression("10am pst to ist", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'10am pst to ist' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("3pm est in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'3pm est in tokyo' should be recognized: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("14:00 UTC in london", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'14:00 UTC in london' should be recognized: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_invalid_expr_returns_none() {
        assert_eq!(
            parse_timezone_expression("hello world", "h:mm A", "uk"),
            None
        );
        assert_eq!(
            parse_timezone_expression("what is the weather", "h:mm A", "uk"),
            None
        );
        assert_eq!(parse_timezone_expression("", "h:mm A", "uk"), None);
    }

    // --- Conversion output format (deterministic, fixed timestamps) ---

    #[test]
    fn test_current_time_output_formatted() {
        let out = parse_timezone_expression("now in tokyo", "h:mm A", "uk");
        assert!(out.is_some(), "current time parsed: {}", fmt_opt(out));
        let s = out.unwrap();
        assert!(
            s.contains("AM") || s.contains("PM"),
            "result contains AM/PM: {s}"
        );
    }

    #[test]
    fn test_conversion_between_tzs() {
        let out = parse_timezone_expression("10am pst to ist", "h:mm A", "uk");
        assert!(out.is_some(), "conversion parsed: {}", fmt_opt(out));
        let s = out.unwrap();
        assert!(
            s.contains("AM") || s.contains("PM"),
            "result contains time: {s}"
        );
    }

    #[test]
    fn test_conversion_with_next_day_indicator() {
        let out = parse_timezone_expression("3pm est in tokyo", "h:mm A", "uk");
        assert!(out.is_some(), "conversion parsed: {}", fmt_opt(out));
        let s = out.unwrap();
        assert!(
            s.contains("(+1)") || s.contains("AM") || s.contains("PM"),
            "result contains day indicator or time: {s}"
        );
    }

    #[test]
    fn test_24h_input() {
        let out = parse_timezone_expression("14:00 UTC in london", "h:mm A", "uk");
        assert!(out.is_some(), "24h input parsed: {}", fmt_opt(out));
    }

    // --- Deterministic format matching ---

    #[test]
    fn test_format_12h() {
        let t = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
        assert_eq!(format_time(&t, "h:mm A"), "2:30 PM");
        assert_eq!(format_time(&t, "hh:mm A"), "02:30 PM");
    }

    #[test]
    fn test_format_24h() {
        let t = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
        assert_eq!(format_time(&t, "HH:mm"), "14:30");
        assert_eq!(format_time(&t, "H:mm"), "14:30");
    }

    #[test]
    fn test_format_am() {
        let t = NaiveTime::from_hms_opt(9, 5, 0).unwrap();
        assert_eq!(format_time(&t, "h:mm A"), "9:05 AM");
        assert_eq!(format_time(&t, "h:mm a"), "9:05 am");
    }

    #[test]
    fn test_format_literal() {
        let t = NaiveTime::from_hms_opt(14, 30, 0).unwrap();
        assert_eq!(format_time(&t, "'Time:' h:mm A"), "Time: 2:30 PM");
    }

    #[test]
    fn test_format_midnight_noon() {
        let midnight = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        assert_eq!(format_time(&midnight, "h:mm A"), "12:00 AM");
        let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap();
        assert_eq!(format_time(&noon, "h:mm A"), "12:00 PM");
    }

    // --- Timezone relative expressions ---

    #[test]
    fn test_relative_expr_in_city() {
        let out = parse_timezone_expression("3 hours from now in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'3 hours from now in tokyo' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_city_first() {
        let out = parse_timezone_expression("tokyo 3 hours from now", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'tokyo 3 hours from now' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_abbreviation_first() {
        let out = parse_timezone_expression("pst 3 hours from now", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'pst 3 hours from now' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_in_abbreviation() {
        let out = parse_timezone_expression("3 hours from now in pst", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'3 hours from now in pst' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_unknown_city_returns_none() {
        assert_eq!(
            parse_timezone_expression("asdfgh 3 hours from now", "h:mm A", "uk"),
            None,
            "unknown city should not expand"
        );
        assert_eq!(
            parse_timezone_expression("3 hours from now in asdfgh", "h:mm A", "uk"),
            None,
            "unknown city in suffix should not expand"
        );
    }

    #[test]
    fn test_relative_minutes_precision() {
        let out = parse_timezone_expression("30 minutes from now in berlin", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'30 minutes from now in berlin' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_minutes_precision_city_first() {
        let out = parse_timezone_expression("london 30 minutes from now", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'london 30 minutes from now' should expand: got {}",
            fmt_opt(out)
        );
    }

    #[test]
    fn test_relative_with_day_offset() {
        let out = parse_timezone_expression("11pm est in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'11pm est in tokyo' should expand: got {}",
            fmt_opt(out)
        );
        let s = out.unwrap();
        assert!(
            s.contains("(+1)") || s.contains("AM") || s.contains("PM"),
            "result contains day indicator or time: {s}"
        );
    }

    #[test]
    fn test_relative_current_time_still_works() {
        let out = parse_timezone_expression("time in tokyo", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'time in tokyo' should still expand via current_time: got {}",
            fmt_opt(out)
        );
        let out = parse_timezone_expression("now in dubai", "h:mm A", "uk");
        assert!(
            out.is_some(),
            "'now in dubai' should still expand via current_time: got {}",
            fmt_opt(out)
        );
    }
}
