use time::{Date, Duration, Month, OffsetDateTime, UtcOffset, util};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Method<'a> {
    Utc,
    Calc(&'a str),
    Format(&'a str),
}

pub(crate) fn parse_methods(mut key: &str) -> Result<Vec<Method<'_>>, String> {
    let mut methods = Vec::new();
    while !key.is_empty() {
        if key.starts_with("utc") {
            methods.push(Method::Utc);
            key = &key[3..];
        } else if key.starts_with("calc(") {
            let mut end = 0;
            let mut depth = 1;
            let bytes = key.as_bytes();
            for (i, &b) in bytes.iter().enumerate().skip(5) {
                if b == b'(' {
                    depth += 1;
                } else if b == b')' {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
            }
            if end == 0 {
                return Err("[Error: unclosed paren in calc]".to_string());
            }
            methods.push(Method::Calc(&key[5..end]));
            key = &key[end + 1..];
        } else if key.starts_with("format(") {
            let mut end = 0;
            let mut depth = 1;
            let mut in_quote = false;
            let bytes = key.as_bytes();
            for (i, &b) in bytes.iter().enumerate().skip(7) {
                match b {
                    b'\'' => in_quote = !in_quote,
                    b'(' if !in_quote => depth += 1,
                    b')' if !in_quote => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if end == 0 {
                return Err("[Error: unclosed paren in format]".to_string());
            }
            methods.push(Method::Format(&key[7..end]));
            key = &key[end + 1..];
        } else {
            return Err(format!("[Error: unknown method '{}']", key));
        }

        if !key.is_empty() {
            if !key.starts_with('.') {
                return Err(format!(
                    "[Error: expected '.' before method, got '{}']",
                    key
                ));
            }
            key = &key[1..];
        }
    }
    Ok(methods)
}

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
        return Err("[Error: calc requires arguments]".to_string());
    }
    let Some(first) = args.chars().next() else {
        return Err("[Error: calc requires arguments]".to_string());
    };
    if first != '+' && first != '-' {
        return Err("[Error: calc needs + or -]".to_string());
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
                return Err("[Error: Missing number in calc]".to_string());
            }
            let val = current_num
                .parse::<i64>()
                .map_err(|_| "[Error: Invalid number]".to_string())?;
            let val = if is_positive { val } else { -val };

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
                    _ => return Err(format!("[Error: Unknown unit '{}' in calc]", c)),
                }
                i += 1;
            }
            current_num.clear();
        } else if c.is_whitespace() {
            i += 1;
        } else {
            return Err(format!("[Error: Invalid character '{}' in calc]", c));
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

/// Resolves `datetime` and `datetime.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if key != "datetime" && !key.starts_with("datetime.") {
        return None;
    }

    let method_str = if key == "datetime" { "" } else { &key[9..] };
    let methods = match parse_methods(method_str) {
        Ok(m) => m,
        Err(e) => return Some(e),
    };

    let mut dt = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let mut format_str = "YYYY-MM-DDTHH:mm:ss";

    for method in methods {
        match method {
            Method::Utc => {
                dt = dt.to_offset(UtcOffset::UTC);
            }
            Method::Calc(args) => {
                dt = match apply_temporal_calc(dt, args) {
                    Ok(new_dt) => new_dt,
                    Err(e) => return Some(e),
                };
            }
            Method::Format(args) => {
                format_str = crate::engine::variables::system::strip_quotes(args.trim())
                    .unwrap_or(args.trim());
            }
        }
    }

    Some(format_temporal(dt, format_str).unwrap_or_else(|e| e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_datetime_default_format() {
        let res = resolve("datetime").unwrap();
        assert!(res.contains('T'));
        assert_eq!(res.len(), 19); // YYYY-MM-DDTHH:mm:ss
    }

    #[test]
    fn test_datetime_utc() {
        let res = resolve("datetime.utc").unwrap();
        assert!(res.contains('T'));
    }

    #[test]
    fn test_datetime_calc_and_format() {
        let res = resolve("datetime.calc(+1d2h).format('Date:' YYYY-MM-DD 'Time:' HH:mm)").unwrap();
        assert!(res.starts_with("Date: "));
        assert!(res.contains("Time: "));
    }

    #[test]
    fn test_datetime_all_calc_units() {
        let res = resolve("datetime.calc(+1y1m1w1d1h1min1s)").unwrap();
        assert!(!res.contains("[Error"));
    }
}
