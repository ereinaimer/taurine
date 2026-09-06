use super::datetime::{apply_temporal_calc, format_temporal, parse_methods};
use time::OffsetDateTime;

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
            super::datetime::Method::Utc => {
                dt = dt.to_offset(time::UtcOffset::UTC);
            }
            super::datetime::Method::Calc(args) => {
                dt = match apply_temporal_calc(dt, args) {
                    Ok(new_dt) => new_dt,
                    Err(e) => return Some(e),
                };
            }
            super::datetime::Method::Format(args) => {
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
        assert_eq!(err_no_sign, "[Error: calc needs + or -]");

        let res_date_unit = resolve("time.calc(+1d)").unwrap();
        assert!(!res_date_unit.contains("[Error"));

        let res_format = resolve("time.format(HH:mm)").unwrap();
        assert!(!res_format.contains("[Error"));

        let res_literal = resolve("time.format('Time is' HH:mm)").unwrap();
        assert!(res_literal.starts_with("Time is "));

        let res_date_token = resolve("time.format(YYYY)").unwrap();
        assert!(!res_date_token.contains("[Error"));

        let res_upper_m = resolve("time.format(HH:MM)").unwrap();
        assert!(!res_upper_m.contains("[Error"));

        let compound_calc = resolve("time.calc(+1h30m)").unwrap();
        assert!(!compound_calc.contains("[Error"));

        let method_chain = resolve("time.utc.calc(-15m).format('Time:' hh:mm A Z)");
        assert!(method_chain.unwrap().starts_with("Time: "));
    }
}
