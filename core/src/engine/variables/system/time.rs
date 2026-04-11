use time::{OffsetDateTime, UtcOffset};

/// Resolves `time.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("time.") {
        return None;
    }

    let sub_key = &key[5..];
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());

    match sub_key {
        "greeting" => Some(get_greeting(now)),
        "epoch" | "unix" => Some(now.unix_timestamp().to_string()),
        "utc" => {
            let utc = now.to_offset(UtcOffset::UTC);
            Some(format_time(utc, false, true))
        }
        "tz" => {
            let offset = now.offset();
            let (h, m, _) = offset.as_hms();
            Some(format!("{:+03}:{:02}", h, m.abs()))
        }
        "12h" => Some(format_time(now, true, true)),
        "24h" => Some(format_time(now, false, true)),
        _ => {
            // Check for modifiers (e.g., time.now.12h)
            let parts: Vec<&str> = sub_key.split('.').collect();
            match parts.as_slice() {
                ["now"] => Some(format_time(now, false, true)),
                ["now", "12h"] => Some(format_time(now, true, true)),
                ["now", "24h"] => Some(format_time(now, false, true)),
                ["full"] => Some(format_time(now, false, false)),
                ["full", "12h"] => Some(format_time(now, true, false)),
                ["full", "24h"] => Some(format_time(now, false, false)),
                _ => None,
            }
        }
    }
}

fn get_greeting(dt: OffsetDateTime) -> String {
    let hour = dt.hour();
    if hour < 12 {
        "Good morning".to_string()
    } else if hour < 17 {
        "Good afternoon".to_string()
    } else {
        "Good evening".to_string()
    }
}

fn format_time(dt: OffsetDateTime, is_12h: bool, hide_seconds: bool) -> String {
    let hour = dt.hour();
    let minute = dt.minute();
    let second = dt.second();

    if is_12h {
        let is_pm = hour >= 12;
        let h12 = match hour % 12 {
            0 => 12,
            h => h,
        };
        let am_pm = if is_pm { "PM" } else { "AM" };

        if hide_seconds {
            format!("{:02}:{:02} {}", h12, minute, am_pm)
        } else {
            format!("{:02}:{:02}:{:02} {}", h12, minute, second, am_pm)
        }
    } else {
        if hide_seconds {
            format!("{:02}:{:02}", hour, minute)
        } else {
            format!("{:02}:{:02}:{:02}", hour, minute, second)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greeting_logic() {
        // Morning
        let morning = OffsetDateTime::now_utc().replace_time(time::macros::time!(08:00));
        assert_eq!(get_greeting(morning), "Good morning");

        // Afternoon
        let afternoon = OffsetDateTime::now_utc().replace_time(time::macros::time!(14:00));
        assert_eq!(get_greeting(afternoon), "Good afternoon");

        // Evening
        let evening = OffsetDateTime::now_utc().replace_time(time::macros::time!(19:00));
        assert_eq!(get_greeting(evening), "Good evening");
    }

    #[test]
    fn test_format_time() {
        let dt = OffsetDateTime::now_utc().replace_time(time::macros::time!(14:30:45));

        assert_eq!(format_time(dt, false, true), "14:30");
        assert_eq!(format_time(dt, false, false), "14:30:45");
        assert_eq!(format_time(dt, true, true), "02:30 PM");
        assert_eq!(format_time(dt, true, false), "02:30:45 PM");
    }

    #[test]
    fn test_resolve_base_keys() {
        assert!(resolve("time.greeting").is_some());
        assert!(resolve("time.epoch").is_some());
        assert!(resolve("time.unix").is_some());
        assert!(resolve("time.utc").is_some());
        assert!(resolve("time.tz").is_some());
        assert!(resolve("time.now").is_some());
        assert!(resolve("time.full").is_some());
    }

    #[test]
    fn test_resolve_modifiers() {
        assert!(resolve("time.now.12h").is_some());
        assert!(resolve("time.now.24h").is_some());
        assert!(resolve("time.full.12h").is_some());
        assert!(resolve("time.full.24h").is_some());
    }
}
