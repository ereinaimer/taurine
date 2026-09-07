use super::datetime::{apply_temporal_calc, format_temporal, parse_methods};
use time::OffsetDateTime;

/// Resolves `date` and `date.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if key != "date" && !key.starts_with("date.") {
        return None;
    }

    let method_str = if key == "date" { "" } else { &key[5..] };
    let methods = match parse_methods(method_str) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Failed to parse date methods from '{}': {}", key, e);
            return None;
        }
    };

    let mut dt = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let mut format_str = "YYYY-MM-DD";

    for method in methods {
        match method {
            super::datetime::Method::Utc => {
                dt = dt.to_offset(time::UtcOffset::UTC);
            }
            super::datetime::Method::Calc(args) => {
                dt = match apply_temporal_calc(dt, args) {
                    Ok(new_dt) => new_dt,
                    Err(e) => {
                        tracing::warn!("Failed to calculate date offset '{}': {}", args, e);
                        return None;
                    }
                };
            }
            super::datetime::Method::Format(args) => {
                format_str = crate::engine::variables::system::strip_quotes(args.trim())
                    .unwrap_or(args.trim());
            }
        }
    }

    match format_temporal(dt, format_str) {
        Ok(formatted) => Some(formatted),
        Err(e) => {
            tracing::warn!("Failed to format date '{}': {}", format_str, e);
            None
        }
    }
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

        let res_double = resolve("date.calc(\"+1d\")");
        assert!(res_double.is_some());

        let res_single = resolve("date.calc('+1d')");
        assert!(res_single.is_some());

        let err_no_sign = resolve("date.calc(1d)");
        assert_eq!(err_no_sign, None);

        let res_time_unit = resolve("date.calc(+1h)").unwrap();
        assert!(!res_time_unit.contains("[Error"));

        let res_format = resolve("date.format(YYYY-MM-DD)").unwrap();
        assert!(!res_format.contains("[Error"));

        let res_literal = resolve("date.format('Today is' dddd)").unwrap();
        assert!(res_literal.starts_with("Today is "));

        let res_time_token = resolve("date.format(HH:mm)").unwrap();
        assert!(!res_time_token.contains("[Error"));

        let res_lower_m = resolve("date.format(YYYY-mm)").unwrap();
        assert!(!res_lower_m.contains("[Error"));

        let compound_calc = resolve("date.calc(+1w2d)").unwrap();
        assert!(!compound_calc.contains("[Error"));

        let method_chain = resolve("date.utc.calc(-1m).format('Month:' MMMM)");
        assert!(method_chain.unwrap().starts_with("Month: "));
    }
}
