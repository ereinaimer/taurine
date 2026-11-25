use super::{Settings, SettingsManager, SpinnerStyle, TuiCursorStyle};
use crate::{
    ai::AiProvider,
    diagnostic::Diagnostic,
    error::{Error, Result},
    keys::{Modifier, ModifierInsertError, Modifiers, parse_hotkey},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApplySettingOutcome {
    pub sync_boot: Option<bool>,
}

pub fn reset_setting_to_default(key: &str) -> Result<()> {
    let default_value = default_setting_input(key)?;
    apply_setting_input(key, default_value.as_deref())
}

pub fn apply_setting_input_with_manager(
    manager: &SettingsManager<'_>,
    key: &str,
    value: Option<&str>,
) -> Result<ApplySettingOutcome> {
    let actual_key = Settings::resolve_key(key);

    let outcome = match actual_key {
        "pause_hotkey" => {
            let hotkey = require_non_empty(value, actual_key)?;
            parse_hotkey(hotkey).map_err(|error| {
                Error::Config(format!("invalid pause_hotkey '{hotkey}': {error}"))
            })?;
            manager.update_setting(actual_key, hotkey.to_string())?;
            ApplySettingOutcome::default()
        }
        "ignore_fullscreen" | "system_tray_enabled" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value_with_key(
                    actual_key,
                    require_non_empty(value, actual_key)?,
                )?,
            )?;
            ApplySettingOutcome::default()
        }
        "audio_theme" => {
            let theme_str = require_non_empty(value, actual_key)?;
            let theme = parse_audio_theme(theme_str)?;
            crate::settings::set_cached_audio_theme(theme);
            manager.update_setting(actual_key, theme)?;
            ApplySettingOutcome::default()
        }
        "audio_volume" => {
            let raw_value = require_non_empty(value, actual_key)?;
            let parsed = raw_value
                .parse::<u32>()
                .map_err(|_| Error::Config(format!("bad audio_volume value: {raw_value}")))?;
            let sanitized = Settings::sanitize_audio_volume(parsed);
            crate::settings::set_cached_audio_volume(sanitized);
            manager.update_setting(actual_key, sanitized)?;
            ApplySettingOutcome::default()
        }
        "start_on_boot" => {
            let enabled = parse_boolean_setting_value_with_key(
                actual_key,
                require_non_empty(value, actual_key)?,
            )?;
            manager.update_setting(actual_key, enabled)?;
            ApplySettingOutcome {
                sync_boot: Some(enabled),
            }
        }
        "ai_provider" => {
            let provider = if let Some(provider) = value.map(str::trim).filter(|v| !v.is_empty()) {
                Some(AiProvider::try_from(provider)?.as_str().to_string())
            } else {
                None
            };
            manager.update_setting(actual_key, provider)?;
            ApplySettingOutcome::default()
        }
        "ai_model" => {
            let model = value
                .map(str::trim)
                .filter(|model| !model.is_empty())
                .map(str::to_string);
            manager.update_setting(actual_key, model)?;
            ApplySettingOutcome::default()
        }
        "ai_custom_endpoint" => {
            let endpoint = value
                .map(str::trim)
                .filter(|endpoint| !endpoint.is_empty())
                .map(str::to_string);
            manager.update_setting(actual_key, endpoint)?;
            ApplySettingOutcome::default()
        }
        "instant_expand" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value_with_key(
                    actual_key,
                    require_non_empty(value, actual_key)?,
                )?,
            )?;
            ApplySettingOutcome::default()
        }
        "auto_update" => {
            manager.update_setting(
                actual_key,
                parse_boolean_setting_value_with_key(
                    actual_key,
                    require_non_empty(value, actual_key)?,
                )?,
            )?;
            ApplySettingOutcome::default()
        }
        "pause_media_while_dictating" => {
            let enabled = parse_boolean_setting_value_with_key(
                actual_key,
                require_non_empty(value, actual_key)?,
            )?;
            crate::settings::set_cached_pause_media_while_dictating(enabled);
            manager.update_setting(actual_key, enabled)?;
            ApplySettingOutcome::default()
        }
        "tui_cursor_style" => {
            manager.update_setting(
                actual_key,
                parse_tui_cursor_style(require_non_empty(value, actual_key)?)?,
            )?;
            ApplySettingOutcome::default()
        }
        "inline_date_format" => {
            let val = require_non_empty(value, actual_key)?.to_string();
            crate::settings::set_cached_inline_date_format(val.clone());
            manager.update_setting(actual_key, val)?;
            ApplySettingOutcome::default()
        }
        "inline_time_format" => {
            let val = require_non_empty(value, actual_key)?.to_string();
            crate::settings::set_cached_inline_time_format(val.clone());
            manager.update_setting(actual_key, val)?;
            ApplySettingOutcome::default()
        }
        "inline_datetime_format" => {
            let val = require_non_empty(value, actual_key)?.to_string();
            crate::settings::set_cached_inline_datetime_format(val.clone());
            manager.update_setting(actual_key, val)?;
            ApplySettingOutcome::default()
        }
        "voice_always_on" | "always_on" | "voice_listen" | "voice_wake" => {
            return Err(Error::Config(
                "voice_always_on has been removed: always-on ambient listening is superseded by on-demand voice hotkeys and voice triggers"
                    .to_string(),
            ));
        }
        "voice_ptt_hotkey" => {
            let hotkey = require_non_empty(value, actual_key)?;
            let canonical = validate_voice_hotkey(hotkey).map_err(|error| {
                let diag =
                    Diagnostic::problem(format!("invalid voice_ptt_hotkey '{hotkey}': {error}"))
                        .help("Specify a valid hotkey (e.g. 'ctrl+space') or modifier chord (e.g. 'win+lctrl')")
                        .example("nite config set voice_ptt_hotkey win+lctrl")
                        .render();
                Error::Config(diag)
            })?;
            crate::settings::set_cached_voice_ptt_hotkey(canonical.clone());
            manager.update_setting(actual_key, canonical)?;
            ApplySettingOutcome::default()
        }
        "voice_handsfree_hotkey" => {
            let hotkey = require_non_empty(value, actual_key)?;
            let canonical = validate_voice_hotkey(hotkey).map_err(|error| {
                let diag = Diagnostic::problem(format!(
                    "invalid voice_handsfree_hotkey '{hotkey}': {error}"
                ))
                .help("Specify a valid hotkey (e.g. 'ctrl+shift+v') or modifier chord (e.g. 'win+lctrl+lalt')")
                .example("nite config set voice_handsfree_hotkey win+lctrl+lalt")
                .render();
                Error::Config(diag)
            })?;
            crate::settings::set_cached_voice_handsfree_hotkey(canonical.clone());
            manager.update_setting(actual_key, canonical)?;
            ApplySettingOutcome::default()
        }
        "voice_dictionary" => {
            let val = value.unwrap_or_default().trim();
            let words: Vec<&str> = val
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let normalized = words.join(", ");
            crate::settings::set_cached_voice_dictionary(normalized.clone());
            manager.update_setting(actual_key, normalized)?;
            ApplySettingOutcome::default()
        }
        "voice_preferred_languages" => {
            let raw = value.unwrap_or_default();
            let langs: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
                .collect();
            let normalized = if langs.is_empty() || langs.iter().any(|s| s == "auto") {
                "auto".to_string()
            } else {
                langs.join(",")
            };
            crate::settings::set_cached_voice_preferred_languages(normalized.clone());
            manager.update_setting(actual_key, normalized)?;
            ApplySettingOutcome::default()
        }
        "voice_input_device" => {
            let parsed = match value {
                Some(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
                _ => None,
            };
            crate::settings::set_cached_voice_input_device(parsed.clone());
            manager.update_setting(actual_key, parsed)?;
            ApplySettingOutcome::default()
        }
        _ => {
            let diag =
                Diagnostic::problem(format!("{actual_key} is not a valid configuration setting"))
                    .suggest(actual_key, &Settings::ALL_KEYS)
                    .help("To view all available settings and their values, run: nite config list")
                    .render();
            return Err(Error::Config(diag));
        }
    };

    Ok(outcome)
}

pub fn parse_boolean_setting_value(value: &str) -> Result<bool> {
    parse_boolean_setting_value_with_key("setting_name", value)
}

pub fn parse_boolean_setting_value_with_key(key: &str, value: &str) -> Result<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => {
            let diag = Diagnostic::problem(format!("{value} is not a valid boolean value"))
                .help("Expected true or false (or 1 / 0)")
                .example(format!("nite config set {key} true"))
                .render();
            Err(Error::Config(diag))
        }
    }
}

pub fn parse_audio_theme(value: &str) -> Result<super::AudioTheme> {
    match value.trim().to_ascii_lowercase().as_str() {
        "minimal" | "default" => Ok(super::AudioTheme::Minimal),
        "arcade" => Ok(super::AudioTheme::Arcade),
        "mechanical" | "typewriter" => Ok(super::AudioTheme::Mechanical),
        "organic" => Ok(super::AudioTheme::Organic),
        "scifi" | "sci-fi" => Ok(super::AudioTheme::Scifi),
        "rubber" => Ok(super::AudioTheme::Rubber),
        "zen" => Ok(super::AudioTheme::Zen),
        other => {
            let diag = Diagnostic::problem(format!("{other} is not an available audio theme"))
                .suggest(other, &super::AudioTheme::ALL_NAMES)
                .options("Available themes", &super::AudioTheme::ALL_NAMES)
                .example("nite config set audio_theme minimal")
                .render();
            Err(Error::Config(diag))
        }
    }
}

pub fn parse_spinner_style(value: &str) -> Result<SpinnerStyle> {
    match value.trim().to_ascii_lowercase().as_str() {
        "classic" => Ok(SpinnerStyle::Classic),
        "braille" => Ok(SpinnerStyle::Braille),
        "arc" => Ok(SpinnerStyle::Arc),
        other => {
            let diag = Diagnostic::problem(format!("{other} is not an available spinner style"))
                .suggest(other, &["classic", "braille", "arc"])
                .options("Available styles", &["classic", "braille", "arc"])
                .example("nite config set spinner_style braille")
                .render();
            Err(Error::Config(diag))
        }
    }
}

pub fn parse_tui_cursor_style(value: &str) -> Result<TuiCursorStyle> {
    match value.trim().to_ascii_lowercase().as_str() {
        "block" => Ok(TuiCursorStyle::Block),
        "bar" => Ok(TuiCursorStyle::Bar),
        "underscore" => Ok(TuiCursorStyle::Underscore),
        other => {
            let diag = Diagnostic::problem(format!("{other} is not an available cursor style"))
                .suggest(other, &["block", "bar", "underscore"])
                .options("Available styles", &["block", "bar", "underscore"])
                .help("Pick the caret shape in TUI settings under Cursor Style")
                .render();
            Err(Error::Config(diag))
        }
    }
}

fn require_non_empty<'a>(value: Option<&'a str>, key: &str) -> Result<&'a str> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Config(format!("{key} must not be empty")))
}

pub fn validate_voice_hotkey(input: &str) -> std::result::Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("hotkey cannot be empty".to_string());
    }

    if let Ok(hk) = parse_hotkey(trimmed) {
        return Ok(hk.canonical_string());
    }

    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return Err("hotkey cannot be empty".to_string());
    }

    let mut canonical_parts = Vec::new();
    let mut bitset = Modifiers::new();

    for part in parts {
        let token = part.to_ascii_lowercase();
        if token.is_empty() {
            return Err("hotkey contains empty segments between '+'".to_string());
        }
        if let Some(modifier) = Modifier::from_alias(&token) {
            bitset.insert(modifier).map_err(|e| match e {
                ModifierInsertError::Duplicate(m) => {
                    format!("duplicate modifier '{}'", m.canonical_name())
                }
                ModifierInsertError::Conflict { existing, incoming } => {
                    format!(
                        "conflicting modifiers '{}' and '{}'",
                        existing.canonical_name(),
                        incoming.canonical_name()
                    )
                }
            })?;
            canonical_parts.push(token);
        } else {
            return Err(format!("unknown or invalid modifier '{part}'"));
        }
    }

    if canonical_parts.is_empty() {
        return Err("hotkey must contain at least one key or modifier".to_string());
    }

    Ok(canonical_parts.join("+"))
}

mod defaults;
mod sync;

pub use defaults::default_setting_input;
pub use sync::apply_setting_input;
#[cfg(test)]
mod tests;
