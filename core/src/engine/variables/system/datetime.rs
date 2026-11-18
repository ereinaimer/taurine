use time::{Date, Duration, Month, OffsetDateTime, UtcOffset, util};

pub(crate) fn add_months_clamped(dt: OffsetDateTime, months: i32) -> OffsetDateTime {
    let date = dt.date();
    let month_index = date.year() * 12 + i32::from(u8::from(date.month())) - 1 + months;
    let year = month_index.div_euclid(12);
    let month_number = month_index.rem_euclid(12) + 1;
    let month = match Month::try_from(month_number as u8) {
        Ok(month) => month,
        Err(_) => {
            tracing::warn!("month calculation out of range; clamping to January");
            Month::January
        }
    };
    let day = date.day().min(util::days_in_month(month, year));
    let date = match Date::from_calendar_date(year, month, day) {
        Ok(date) => date,
        Err(_) => dt.date(),
    };
    dt.replace_date(date)
}

pub(crate) fn apply_temporal_calc(
    mut dt: OffsetDateTime,
    args: &str,
) -> Result<OffsetDateTime, String> {
    let args = crate::engine::variables::system::strip_quotes(args.trim()).unwrap_or(args.trim());
    if args.is_empty() {
        return Err("calc requires arguments".to_string());
    }
    let Some(first) = args.chars().next() else {
        return Err("calc requires arguments".to_string());
    };
    if first != '+' && first != '-' {
        return Err("calc needs + or -".to_string());
    }

    let mut is_positive = true;
    let mut current_num = String::new();
    let mut i = 0;
    let chars: Vec<char> = args.chars().collect();

    while i < chars.len() {
        let c = chars[i];
        if c == '+' {
            is_positive = true;
            i += 1;
        } else if c == '-' {
            is_positive = false;
            i += 1;
        } else if c.is_ascii_digit() {
            current_num.push(c);
            i += 1;
        } else if c.is_alphabetic() {
            if current_num.is_empty() {
                return Err("Missing number in calc".to_string());
            }
            let val = current_num
                .parse::<i64>()
                .map_err(|_| "Invalid number in calc".to_string())?;
            let val = if is_positive { val } else { -val };
            // No calendar use needs more than a million of any unit; larger
            // magnitudes would overflow month scaling and duration math below.
            if val.unsigned_abs() > 1_000_000 {
                return Err("calc magnitude out of range".to_string());
            }

            // Check if multi-char unit like "min"
            let rem: String = chars[i..]
                .iter()
                .take_while(|ch| ch.is_alphabetic())
                .collect();
            let unit_len = rem.len();
            if rem == "min" {
                dt += Duration::minutes(val);
                i += unit_len;
            } else {
                match c {
                    'y' | 'Y' => {
                        dt = add_months_clamped(dt, (val * 12) as i32);
                    }
                    'm' | 'M' => {
                        dt = add_months_clamped(dt, val as i32);
                    }
                    'w' | 'W' => {
                        dt += Duration::days(val * 7);
                    }
                    'd' | 'D' => {
                        dt += Duration::days(val);
                    }
                    'h' | 'H' => {
                        dt += Duration::hours(val);
                    }
                    'i' => {
                        dt += Duration::minutes(val);
                    }
                    's' | 'S' => {
                        dt += Duration::seconds(val);
                    }
                    _ => return Err(format!("Unknown unit '{}' in calc", c)),
                }
                i += 1;
            }
            current_num.clear();
        } else if c.is_whitespace() {
            i += 1;
        } else {
            return Err(format!("Invalid character '{}' in calc", c));
        }
    }
    Ok(dt)
}

fn hour_12(dt: OffsetDateTime) -> u8 {
    let h = dt.hour() % 12;
    if h == 0 { 12 } else { h }
}

pub(crate) fn format_temporal(dt: OffsetDateTime, format_str: &str) -> Result<String, String> {
    let mut out = String::new();
    let chars: Vec<char> = format_str.chars().collect();
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

        let remaining = &format_str[i..];

        // Date tokens
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
        // Time tokens
        } else if remaining.starts_with("HH") {
            out.push_str(&format!("{:02}", dt.hour()));
            i += 2;
        } else if remaining.starts_with("H") {
            out.push_str(&format!("{}", dt.hour()));
            i += 1;
        } else if remaining.starts_with("hh") {
            out.push_str(&format!("{:02}", hour_12(dt)));
            i += 2;
        } else if remaining.starts_with("h") {
            out.push_str(&format!("{}", hour_12(dt)));
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
        } else if remaining.starts_with("Z") {
            let offset = dt.offset();
            let (h, m, _) = offset.as_hms();
            out.push_str(&format!("{:+03}:{:02}", h, m.abs()));
            i += 1;
        } else if remaining.starts_with("X") {
            out.push_str(&format!("{}", dt.unix_timestamp()));
            i += 1;
        } else if remaining.starts_with("x") {
            out.push_str(&format!("{}", dt.unix_timestamp_nanos() / 1_000_000));
            i += 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    Ok(out)
}

/// Resolves the unified `date` system variable with order-independent arguments:
/// offsets (e.g. `+1d`, `tomorrow`, `yesterday`), timezones (`utc`, `local`), and format strings.
pub fn resolve_date(raw: &str) -> Option<String> {
    let tokens = crate::engine::variables::parser::tokenize_words(raw);
    let mut offset = "none".to_string();
    let mut tz = "local".to_string();
    let mut format = String::new();

    for token in tokens {
        let trimmed = crate::engine::variables::system::strip_argument_quotes(&token);
        let lower = trimmed.to_ascii_lowercase();
        if lower == "tomorrow" {
            offset = "+1d".to_string();
        } else if lower == "yesterday" {
            offset = "-1d".to_string();
        } else if lower == "today" || lower == "none" {
            offset = "none".to_string();
        } else if trimmed.starts_with('+') || trimmed.starts_with('-') {
            offset = trimmed.to_string();
        } else if lower == "utc" || lower == "local" {
            tz = lower;
        } else if format.is_empty() {
            format = token.to_string();
        } else {
            format.push(' ');
            format.push_str(&token);
        }
    }

    let default_format = "YYYY-MM-DD";
    let format_str = if format.is_empty() {
        default_format
    } else {
        &format
    };

    let mut dt = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    if tz == "utc" {
        dt = dt.to_offset(UtcOffset::UTC);
    }
    if offset != "none" {
        dt = apply_temporal_calc(dt, &offset).ok()?;
    }

    format_temporal(dt, format_str).ok()
}

/// Resolves the unified `time` system variable with order-independent arguments:
/// timezones (`utc`, `local`), offsets, and format strings.
pub fn resolve_time(raw: &str) -> Option<String> {
    let tokens = crate::engine::variables::parser::tokenize_words(raw);
    let mut offset = "none".to_string();
    let mut tz = "local".to_string();
    let mut format = String::new();

    for token in tokens {
        let trimmed = crate::engine::variables::system::strip_argument_quotes(&token);
        let lower = trimmed.to_ascii_lowercase();
        if lower == "none" {
            offset = "none".to_string();
        } else if trimmed.starts_with('+') || trimmed.starts_with('-') {
            offset = trimmed.to_string();
        } else if lower == "utc" || lower == "local" {
            tz = lower;
        } else if format.is_empty() {
            format = token.to_string();
        } else {
            format.push(' ');
            format.push_str(&token);
        }
    }

    let default_format = "HH:mm";
    let format_str = if format.is_empty() {
        default_format
    } else {
        &format
    };

    let mut dt = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    if tz == "utc" {
        dt = dt.to_offset(UtcOffset::UTC);
    }
    if offset != "none" {
        dt = apply_temporal_calc(dt, &offset).ok()?;
    }

    format_temporal(dt, format_str).ok()
}

/// Fallback compatibility resolver for legacy tests.
pub fn resolve(raw: &str) -> Option<String> {
    let raw_trimmed = raw.trim();
    if raw_trimmed.starts_with("time") {
        let rest = raw_trimmed.strip_prefix("time").unwrap().trim();
        resolve_time(rest)
    } else {
        let rest = raw_trimmed
            .strip_prefix("date")
            .unwrap_or(raw_trimmed)
            .trim();
        resolve_date(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrono_kinds_offsets_formats() {
        assert!(resolve_date("").is_some());
        assert!(resolve_date("+1d").is_some());
        assert!(resolve_date("tomorrow").is_some());
        assert!(resolve_date("yesterday").is_some());
        assert!(resolve_date("utc").is_some());
        assert!(resolve_date("+1d YYYY-MM-DD utc").is_some());
        assert!(resolve_time("").is_some());
        assert!(resolve_time("utc").is_some());
        assert!(resolve_time("HH:mm:ss").is_some());
    }

    #[test]
    fn chrono_per_type_default_formats() {
        assert_eq!(resolve_date("").unwrap().len(), 10); // YYYY-MM-DD
        assert_eq!(resolve_time("").unwrap().len(), 5); // HH:mm
    }

    #[test]
    fn chrono_offset_and_tz_validation() {
        assert!(resolve_date("+1d").is_some());
        assert!(resolve_time("+1h").is_some());
    }

    #[test]
    fn chrono_calc_units_and_literals_kept() {
        let lit = resolve_date("none \"'Today is' dddd\" local").unwrap();
        assert!(lit.starts_with("Today is "));
    }
}
