use time::{Duration, OffsetDateTime, UtcOffset};

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

fn apply_time_calc(mut dt: OffsetDateTime, args: &str) -> Result<OffsetDateTime, String> {
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
                .parse::<i64>()
                .map_err(|_| "[Error: Invalid number]".to_string())?;
            let val = if is_positive { val } else { -val };

            match c {
                'h' | 'H' => {
                    dt += Duration::hours(val);
                }
                'm' => {
                    dt += Duration::minutes(val);
                }
                's' | 'S' => {
                    dt += Duration::seconds(val);
                }
                'd' | 'w' | 'y' | 'Y' | 'M' => {
                    return Err(format!(
                        "[Error: '{}' is a date unit and cannot be used in time.calc]",
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

fn hour_12(dt: OffsetDateTime) -> u8 {
    let h = dt.hour() % 12;
    if h == 0 { 12 } else { h }
}

fn format_time(dt: OffsetDateTime, format_str: &str) -> Result<String, String> {
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

        if remaining.starts_with("HH") {
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
        } else if remaining.starts_with("YYYY")
            || remaining.starts_with("MMMM")
            || remaining.starts_with("dddd")
        {
            return Err(format!(
                "[Error: Date token '{}' cannot be used in time.format]",
                &remaining[0..4]
            ));
        } else if remaining.starts_with("MMM") || remaining.starts_with("ddd") {
            return Err(format!(
                "[Error: Date token '{}' cannot be used in time.format]",
                &remaining[0..3]
            ));
        } else if remaining.starts_with("YY")
            || remaining.starts_with("MM")
            || remaining.starts_with("DD")
        {
            return Err(format!(
                "[Error: Date token '{}' cannot be used in time.format]",
                &remaining[0..2]
            ));
        } else if remaining.starts_with('Y')
            || remaining.starts_with('M')
            || remaining.starts_with('D')
            || remaining.starts_with('d')
        {
            return Err(format!(
                "[Error: Date token '{}' cannot be used in time.format]",
                chars[i]
            ));
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }

    Ok(out)
}

/// Resolves `time` and `time.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if key != "time" && !key.starts_with("time.") {
        return None;
    }

    let method_str = if key == "time" { "" } else { &key[5..] };
    let methods = match parse_methods(method_str) {
        Ok(m) => m,
        Err(e) => return Some(e),
    };

    let mut dt = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let mut format_str = "HH:mm";

    for method in methods {
        match method {
            Method::Utc => {
                dt = dt.to_offset(UtcOffset::UTC);
            }
            Method::Calc(args) => {
                dt = match apply_time_calc(dt, args) {
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

    Some(format_time(dt, format_str).unwrap_or_else(|e| e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_methods() {
        assert!(resolve("time").is_some());
        assert!(resolve("time.utc").is_some());

        let res = resolve("time.calc(+1h)");
        assert!(res.is_some());
        assert!(!res.as_ref().unwrap().contains("[Error"));

        let res_double = resolve("time.calc(\"+1h\")");
        assert!(res_double.is_some());
        assert!(!res_double.as_ref().unwrap().contains("[Error"));

        let res_single = resolve("time.calc('+1h')");
        assert!(res_single.is_some());
        assert!(!res_single.as_ref().unwrap().contains("[Error"));

        let err_no_sign = resolve("time.calc(1h)").unwrap();
        assert_eq!(err_no_sign, "[Error: calc requires explicit + or - sign]");

        let err_date_unit = resolve("time.calc(+1d)").unwrap();
        assert_eq!(
            err_date_unit,
            "[Error: 'd' is a date unit and cannot be used in time.calc]"
        );

        let res_format = resolve("time.format(HH:mm)").unwrap();
        assert!(!res_format.contains("[Error"));

        let res_literal = resolve("time.format('Time is' HH:mm)").unwrap();
        assert!(res_literal.starts_with("Time is "));

        let err_date_token = resolve("time.format(YYYY)").unwrap();
        assert_eq!(
            err_date_token,
            "[Error: Date token 'YYYY' cannot be used in time.format]"
        );

        let err_upper_m = resolve("time.format(HH:MM)").unwrap();
        assert_eq!(
            err_upper_m,
            "[Error: Date token 'MM' cannot be used in time.format]"
        );

        let compound_calc = resolve("time.calc(+1h30m)").unwrap();
        assert!(!compound_calc.contains("[Error"));

        let method_chain = resolve("time.utc.calc(-15m).format('Time:' hh:mm A Z)");
        assert!(method_chain.unwrap().starts_with("Time: "));
    }
}
