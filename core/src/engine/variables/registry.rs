#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    UnknownRoot(String),
    MissingModifier {
        root: &'static str,
    },
    UnexpectedModifier {
        root: &'static str,
        modifier: String,
    },
    InvalidModifier {
        root: &'static str,
        modifier: String,
        allowed: &'static [&'static str],
    },
}

const TIME_MODIFIERS: &[&str] = &[
    "greeting", "epoch", "unix", "utc", "tz", "12h", "24h", "now", "now.12h", "now.24h", "full",
    "full.12h", "full.24h",
];

const DATE_MODIFIERS: &[&str] = &[
    "iso",
    "short",
    "long",
    "tomorrow",
    "tomorrow.iso",
    "tomorrow.short",
    "tomorrow.long",
    "yesterday",
    "yesterday.iso",
    "yesterday.short",
    "yesterday.long",
    "weekday",
    "year",
    "month",
    "month_name",
    "day",
];

const UUID_MODIFIERS: &[&str] = &["v4", "v7", "simple"];

pub fn validate_system_tag(root: &str, modifier: Option<&str>) -> Result<(), ValidationError> {
    match root {
        "cursor" => validate_no_modifier("cursor", modifier),
        "clipboard" => validate_no_modifier("clipboard", modifier),
        "time" => validate_known_modifier("time", modifier, TIME_MODIFIERS),
        "date" => validate_known_modifier("date", modifier, DATE_MODIFIERS),
        "uuid" => validate_optional_known_modifier("uuid", modifier, UUID_MODIFIERS),
        "env" => validate_env_modifier(modifier),
        "key" => validate_freeform_modifier("key", modifier),
        "delay" => validate_delay_modifier(modifier),
        _ => Err(ValidationError::UnknownRoot(root.to_string())),
    }
}

fn validate_no_modifier(root: &'static str, modifier: Option<&str>) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(modifier) => Err(ValidationError::UnexpectedModifier {
            root,
            modifier: modifier.to_string(),
        }),
    }
}

fn validate_known_modifier(
    root: &'static str,
    modifier: Option<&str>,
    allowed: &'static [&'static str],
) -> Result<(), ValidationError> {
    let modifier = normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root })?)
        .ok_or(ValidationError::MissingModifier { root })?;

    if allowed.contains(&modifier) {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root,
            modifier: modifier.to_string(),
            allowed,
        })
    }
}

fn validate_optional_known_modifier(
    root: &'static str,
    modifier: Option<&str>,
    allowed: &'static [&'static str],
) -> Result<(), ValidationError> {
    match modifier.and_then(normalize_modifier) {
        None => Ok(()),
        Some(modifier) if allowed.contains(&modifier) => Ok(()),
        Some(modifier) => Err(ValidationError::InvalidModifier {
            root,
            modifier: modifier.to_string(),
            allowed,
        }),
    }
}

fn validate_env_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    if normalize_modifier(modifier.unwrap_or_default()).is_some() {
        Ok(())
    } else {
        Err(ValidationError::MissingModifier { root: "env" })
    }
}

fn validate_freeform_modifier(
    root: &'static str,
    modifier: Option<&str>,
) -> Result<(), ValidationError> {
    if normalize_modifier(modifier.unwrap_or_default()).is_some() {
        Ok(())
    } else {
        Err(ValidationError::MissingModifier { root })
    }
}

fn validate_delay_modifier(modifier: Option<&str>) -> Result<(), ValidationError> {
    let modifier =
        normalize_modifier(modifier.ok_or(ValidationError::MissingModifier { root: "delay" })?)
            .ok_or(ValidationError::MissingModifier { root: "delay" })?;

    if parse_delay_ms(modifier).is_some() {
        Ok(())
    } else {
        Err(ValidationError::InvalidModifier {
            root: "delay",
            modifier: modifier.to_string(),
            allowed: &["<u64>ms"],
        })
    }
}

fn normalize_modifier(modifier: &str) -> Option<&str> {
    let trimmed = modifier.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn parse_delay_ms(s: &str) -> Option<u64> {
    s.trim()
        .strip_suffix("ms")
        .and_then(|n| n.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_time_modifiers_from_resolver_match_arms() {
        for modifier in TIME_MODIFIERS {
            assert_eq!(validate_system_tag("time", Some(modifier)), Ok(()));
        }
        assert_eq!(
            validate_system_tag("time", Some("india")),
            Err(ValidationError::InvalidModifier {
                root: "time",
                modifier: "india".to_string(),
                allowed: TIME_MODIFIERS,
            })
        );
    }

    #[test]
    fn validates_date_modifiers_from_resolver_match_arms() {
        for modifier in DATE_MODIFIERS {
            assert_eq!(validate_system_tag("date", Some(modifier)), Ok(()));
        }
        assert_eq!(
            validate_system_tag("date", Some("tomorrow.india")),
            Err(ValidationError::InvalidModifier {
                root: "date",
                modifier: "tomorrow.india".to_string(),
                allowed: DATE_MODIFIERS,
            })
        );
    }

    #[test]
    fn validates_uuid_modifiers_and_optional_default() {
        assert_eq!(validate_system_tag("uuid", None), Ok(()));
        for modifier in UUID_MODIFIERS {
            assert_eq!(validate_system_tag("uuid", Some(modifier)), Ok(()));
        }
        assert_eq!(
            validate_system_tag("uuid", Some("v1")),
            Err(ValidationError::InvalidModifier {
                root: "uuid",
                modifier: "v1".to_string(),
                allowed: UUID_MODIFIERS,
            })
        );
    }

    #[test]
    fn validates_roots_with_no_modifiers() {
        assert_eq!(validate_system_tag("cursor", None), Ok(()));
        assert_eq!(validate_system_tag("clipboard", None), Ok(()));
        assert_eq!(
            validate_system_tag("cursor", Some("now")),
            Err(ValidationError::UnexpectedModifier {
                root: "cursor",
                modifier: "now".to_string(),
            })
        );
    }

    #[test]
    fn validates_env_as_dynamic_key() {
        assert_eq!(validate_system_tag("env", Some("TAURINE_HOME")), Ok(()));
        assert_eq!(validate_system_tag("env", Some(" USERPROFILE ")), Ok(()));
        assert_eq!(
            validate_system_tag("env", None),
            Err(ValidationError::MissingModifier { root: "env" })
        );
    }

    #[test]
    fn validates_key_as_freeform_suffix_because_system_mod_has_no_alias_whitelist() {
        assert_eq!(validate_system_tag("key", Some("enter")), Ok(()));
        assert_eq!(validate_system_tag("key", Some("ctrl+shift+end")), Ok(()));
        assert_eq!(
            validate_system_tag("key", None),
            Err(ValidationError::MissingModifier { root: "key" })
        );
    }

    #[test]
    fn validates_delay_with_same_shape_as_system_parser() {
        assert_eq!(validate_system_tag("delay", Some("200ms")), Ok(()));
        assert_eq!(validate_system_tag("delay", Some(" 0ms ")), Ok(()));
        assert_eq!(
            validate_system_tag("delay", Some("200s")),
            Err(ValidationError::InvalidModifier {
                root: "delay",
                modifier: "200s".to_string(),
                allowed: &["<u64>ms"],
            })
        );
    }

    #[test]
    fn rejects_unknown_roots() {
        assert_eq!(
            validate_system_tag("timezone", Some("utc")),
            Err(ValidationError::UnknownRoot("timezone".to_string()))
        );
    }
}
