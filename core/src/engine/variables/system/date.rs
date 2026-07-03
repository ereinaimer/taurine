use time::{Date, Duration, Month, OffsetDateTime, util};

#[derive(Debug)]
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
                return Err("[Error: Unclosed parenthesis in calc]".to_string());
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
                return Err("[Error: Unclosed parenthesis in format]".to_string());
            }
            methods.push(Method::Format(&key[7..end]));
            key = &key[end + 1..];
        } else {
            return Err(format!("[Error: Unknown method starting at '{}']", key));
        }

        if !key.is_empty() {
            if !key.starts_with('.') {
                return Err(format!(
                    "[Error: Expected '.' before next method, found '{}']",
                    key
                ));
            }
            key = &key[1..];
        }
    }
    Ok(methods)
}

fn add_months_clamped(dt: OffsetDateTime, months: i32) -> OffsetDateTime {
    let date = dt.date();
    let month_index = date.year() * 12 + i32::from(u8::from(date.month())) - 1 + months;
    let year = month_index.div_euclid(12);
    let month_number = month_index.rem_euclid(12) + 1;
    let month = Month::try_from(month_number as u8).expect("month number is always 1..=12");
    let day = date.day().min(util::days_in_month(month, year));
    let date = Date::from_calendar_date(year, month, day).expect("clamped date is valid");
    dt.replace_date(date)
}

fn apply_date_calc(mut dt: OffsetDateTime, args: &str) -> Result<OffsetDateTime, String> {
    let args = crate::engine::variables::system::strip_quotes(args.trim()).unwrap_or(args.trim());
    if args.is_empty() {
        return Err("[Error: calc requires arguments]".to_string());
    }
    let first = args.chars().next().unwrap();
    if first != '+' && first != '-' {
        return Err("[Error: calc requires explicit + or - sign]".to_string());
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
                .parse::<i32>()
                .map_err(|_| "[Error: Invalid number]".to_string())?;
            let val = if is_positive { val } else { -val };

            match c {
                'd' => {
                    dt += Duration::days(val as i64);
                }
                'w' => {
                    dt += Duration::days((val * 7) as i64);
                }
                'm' | 'M' => {
                    dt = add_months_clamped(dt, val);
                }
                'y' | 'Y' => {
                    dt = add_months_clamped(dt, val * 12);
                }
                'h' | 'H' | 's' | 'S' => {
                    return Err(format!(
                        "[Error: '{}' is a time unit and cannot be used in date.calc]",
                        c
                    ));
                }
                _ => return Err(format!("[Error: Unknown unit '{}' in calc]", c)),
            }
            current_num.clear();
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else {
            return Err(format!("[Error: Invalid character '{}' in calc]", c));
        }
    }
    Ok(dt)
}

fn format_date(dt: OffsetDateTime, format_str: &str) -> Result<String, String> {
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
        } else if remaining.starts_with("HH")
            || remaining.starts_with("hh")
            || remaining.starts_with("ss")
        {
            return Err(format!(
                "[Error: Time token '{}' cannot be used in date.format]",
                &remaining[0..2]
            ));
        } else if remaining.starts_with("mm") {
            return Err("[Error: Time token 'mm' cannot be used in date.format]".to_string());
        } else if remaining.starts_with('H')
            || remaining.starts_with('h')
            || remaining.starts_with('m')
            || remaining.starts_with('s')
            || remaining.starts_with('A')
            || remaining.starts_with('a')
            || remaining.starts_with('Z')
            || remaining.starts_with('X')
            || remaining.starts_with('x')
        {
            return Err(format!(
                "[Error: Time token '{}' cannot be used in date.format]",
                chars[i]
            ));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    Ok(out)
}

/// Resolves `date` and `date.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if key != "date" && !key.starts_with("date.") {
        return None;
    }

    let method_str = if key == "date" { "" } else { &key[5..] };
    let methods = match parse_methods(method_str) {
        Ok(m) => m,
        Err(e) => return Some(e),
    };

    let mut dt = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let mut format_str = "YYYY-MM-DD";

    for method in methods {
        match method {
            Method::Utc => {
                dt = dt.to_offset(time::UtcOffset::UTC);
            }
            Method::Calc(args) => {
                dt = match apply_date_calc(dt, args) {
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

    Some(format_date(dt, format_str).unwrap_or_else(|e| e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_methods() {
        assert!(resolve("date").is_some());
        assert!(resolve("date.utc").is_some());

        let res = resolve("date.calc(+1d)");
        assert!(res.is_some());
        assert!(!res.as_ref().unwrap().contains("[Error"));

        let res_double = resolve("date.calc(\"+1d\")");
        assert!(res_double.is_some());
        assert!(!res_double.as_ref().unwrap().contains("[Error"));

        let res_single = resolve("date.calc('+1d')");
        assert!(res_single.is_some());
        assert!(!res_single.as_ref().unwrap().contains("[Error"));

        let err_no_sign = resolve("date.calc(1d)").unwrap();
        assert_eq!(err_no_sign, "[Error: calc requires explicit + or - sign]");

        let err_time_unit = resolve("date.calc(+1h)").unwrap();
        assert_eq!(
            err_time_unit,
            "[Error: 'h' is a time unit and cannot be used in date.calc]"
        );

        let res_format = resolve("date.format(YYYY-MM-DD)").unwrap();
        assert!(!res_format.contains("[Error"));

        let res_literal = resolve("date.format('Today is' dddd)").unwrap();
        assert!(res_literal.starts_with("Today is "));

        let err_time_token = resolve("date.format(HH:mm)").unwrap();
        assert_eq!(
            err_time_token,
            "[Error: Time token 'HH' cannot be used in date.format]"
        );

        let err_lower_m = resolve("date.format(YYYY-mm)").unwrap();
        assert_eq!(
            err_lower_m,
            "[Error: Time token 'mm' cannot be used in date.format]"
        );

        let compound_calc = resolve("date.calc(+1w2d)").unwrap();
        assert!(!compound_calc.contains("[Error"));

        let method_chain = resolve("date.utc.calc(-1m).format('Month:' MMMM)");
        assert!(method_chain.unwrap().starts_with("Month: "));
    }
}
