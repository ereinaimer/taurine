use time::{Duration, OffsetDateTime};

/// Resolves `date.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("date.") {
        return None;
    }

    let sub_key = &key[5..];
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

    match sub_key {
        "iso" => Some(format_date_iso(now)),
        "short" => Some(format_date_short(now)),
        "long" => Some(format_date_long(now)),
        "tomorrow" => {
            let tomorrow = now + Duration::days(1);
            Some(format_date_iso(tomorrow))
        }
        "yesterday" => {
            let yesterday = now - Duration::days(1);
            Some(format_date_iso(yesterday))
        }
        "weekday" => Some(now.weekday().to_string()),
        "year" => Some(now.year().to_string()),
        "month" => Some(format!("{:02}", u8::from(now.month()))),
        "month_name" => Some(now.month().to_string()),
        "day" => Some(format!("{:02}", now.day())),
        _ => None,
    }
}

fn format_date_iso(dt: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}",
        dt.year(),
        u8::from(dt.month()),
        dt.day()
    )
}

fn format_date_short(dt: OffsetDateTime) -> String {
    format!(
        "{:02}/{:02}/{:04}",
        u8::from(dt.month()),
        dt.day(),
        dt.year()
    )
}

fn format_date_long(dt: OffsetDateTime) -> String {
    format!("{} {:02}, {:04}", dt.month(), dt.day(), dt.year())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_base_keys() {
        assert!(resolve("date.iso").is_some());
        assert!(resolve("date.short").is_some());
        assert!(resolve("date.long").is_some());
        assert!(resolve("date.tomorrow").is_some());
        assert!(resolve("date.yesterday").is_some());
        assert!(resolve("date.weekday").is_some());
        assert!(resolve("date.year").is_some());
        assert!(resolve("date.month").is_some());
        assert!(resolve("date.month_name").is_some());
        assert!(resolve("date.day").is_some());
    }

    #[test]
    fn test_date_iso_format() {
        let dt = OffsetDateTime::now_utc().replace_date(time::macros::date!(2026 - 04 - 11));
        assert_eq!(format_date_iso(dt), "2026-04-11");
    }

    #[test]
    fn test_date_short_format() {
        let dt = OffsetDateTime::now_utc().replace_date(time::macros::date!(2026 - 04 - 11));
        assert_eq!(format_date_short(dt), "04/11/2026");
    }

    #[test]
    fn test_date_long_format() {
        let dt = OffsetDateTime::now_utc().replace_date(time::macros::date!(2026 - 04 - 11));
        assert_eq!(format_date_long(dt), "April 11, 2026");
    }
}
