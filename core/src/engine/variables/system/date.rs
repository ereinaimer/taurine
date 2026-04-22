use time::{Date, Duration, Month, OffsetDateTime, util};

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
        _ => {
            let parts: Vec<&str> = sub_key.split('.').collect();
            match parts.as_slice() {
                ["tomorrow"] => Some(format_date_iso(now + Duration::days(1))),
                ["tomorrow", "iso"] => Some(format_date_iso(now + Duration::days(1))),
                ["tomorrow", "short"] => Some(format_date_short(now + Duration::days(1))),
                ["tomorrow", "long"] => Some(format_date_long(now + Duration::days(1))),
                ["yesterday"] => Some(format_date_iso(now - Duration::days(1))),
                ["yesterday", "iso"] => Some(format_date_iso(now - Duration::days(1))),
                ["yesterday", "short"] => Some(format_date_short(now - Duration::days(1))),
                ["yesterday", "long"] => Some(format_date_long(now - Duration::days(1))),
                ["next_week"] => Some(format_date_iso(now + Duration::days(7))),
                ["next_week", "iso"] => Some(format_date_iso(now + Duration::days(7))),
                ["next_week", "short"] => Some(format_date_short(now + Duration::days(7))),
                ["next_week", "long"] => Some(format_date_long(now + Duration::days(7))),
                ["last_week"] => Some(format_date_iso(now - Duration::days(7))),
                ["last_week", "iso"] => Some(format_date_iso(now - Duration::days(7))),
                ["last_week", "short"] => Some(format_date_short(now - Duration::days(7))),
                ["last_week", "long"] => Some(format_date_long(now - Duration::days(7))),
                ["next_month"] => Some(format_date_iso(add_months_clamped(now, 1))),
                ["next_month", "iso"] => Some(format_date_iso(add_months_clamped(now, 1))),
                ["next_month", "short"] => Some(format_date_short(add_months_clamped(now, 1))),
                ["next_month", "long"] => Some(format_date_long(add_months_clamped(now, 1))),
                ["last_month"] => Some(format_date_iso(add_months_clamped(now, -1))),
                ["last_month", "iso"] => Some(format_date_iso(add_months_clamped(now, -1))),
                ["last_month", "short"] => Some(format_date_short(add_months_clamped(now, -1))),
                ["last_month", "long"] => Some(format_date_long(add_months_clamped(now, -1))),
                ["weekday"] => Some(now.weekday().to_string()),
                ["year"] => Some(now.year().to_string()),
                ["month"] => Some(format!("{:02}", u8::from(now.month()))),
                ["month_name"] => Some(now.month().to_string()),
                ["day"] => Some(format!("{:02}", now.day())),
                ["week"] => Some(now.iso_week().to_string()),
                ["quarter"] => Some(format!("Q{}", quarter(now.month()))),
                ["day_of_year"] => Some(now.ordinal().to_string()),
                ["days_in_month"] => Some(util::days_in_month(now.month(), now.year()).to_string()),
                ["ordinal"] => Some(format_ordinal(now.day())),
                ["is_leap_year"] => Some(util::is_leap_year(now.year()).to_string()),
                ["century"] => Some(century(now.year()).to_string()),
                _ => None,
            }
        }
    }
}

fn quarter(month: Month) -> u8 {
    (u8::from(month) - 1) / 3 + 1
}

fn format_ordinal(day: u8) -> String {
    let suffix = match day % 100 {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };

    format!("{day}{suffix}")
}

fn century(year: i32) -> i32 {
    (year - 1).div_euclid(100) + 1
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
    fn test_resolve_modifiers() {
        assert!(resolve("date.tomorrow.iso").is_some());
        assert!(resolve("date.tomorrow.short").is_some());
        assert!(resolve("date.tomorrow.long").is_some());
        assert!(resolve("date.yesterday.iso").is_some());
        assert!(resolve("date.yesterday.short").is_some());
        assert!(resolve("date.yesterday.long").is_some());
        assert!(resolve("date.next_week.iso").is_some());
        assert!(resolve("date.next_week.short").is_some());
        assert!(resolve("date.next_week.long").is_some());
        assert!(resolve("date.last_week.iso").is_some());
        assert!(resolve("date.last_week.short").is_some());
        assert!(resolve("date.last_week.long").is_some());
        assert!(resolve("date.next_month.iso").is_some());
        assert!(resolve("date.next_month.short").is_some());
        assert!(resolve("date.next_month.long").is_some());
        assert!(resolve("date.last_month.iso").is_some());
        assert!(resolve("date.last_month.short").is_some());
        assert!(resolve("date.last_month.long").is_some());
    }

    #[test]
    fn test_ordinal_suffixes() {
        let cases = [
            (1, "1st"),
            (2, "2nd"),
            (3, "3rd"),
            (11, "11th"),
            (12, "12th"),
            (13, "13th"),
            (21, "21st"),
            (22, "22nd"),
        ];

        for (day, expected) in cases {
            assert_eq!(format_ordinal(day), expected);
        }
    }

    #[test]
    fn test_next_month_clamps_across_boundaries() {
        let jan_31_2025 =
            OffsetDateTime::now_utc().replace_date(time::macros::date!(2025 - 01 - 31));
        let jan_31_2024 =
            OffsetDateTime::now_utc().replace_date(time::macros::date!(2024 - 01 - 31));
        let dec_31_2025 =
            OffsetDateTime::now_utc().replace_date(time::macros::date!(2025 - 12 - 31));

        assert_eq!(
            format_date_iso(add_months_clamped(jan_31_2025, 1)),
            "2025-02-28"
        );
        assert_eq!(
            format_date_iso(add_months_clamped(jan_31_2024, 1)),
            "2024-02-29"
        );
        assert_eq!(
            format_date_iso(add_months_clamped(dec_31_2025, 1)),
            "2026-01-31"
        );
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
