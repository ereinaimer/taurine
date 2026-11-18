use super::super::{Settings, TuiCursorStyle};
use crate::error::{Error, Result};

pub fn default_setting_input(key: &str) -> Result<Option<String>> {
    let actual_key = Settings::resolve_key(key);
    let defaults = Settings::default();

    match actual_key {
        "pause_hotkey" => Ok(Some(defaults.pause_hotkey)),
        "start_on_boot" => Ok(Some(defaults.start_on_boot.to_string())),
        "auto_update" => Ok(Some(defaults.auto_update.to_string())),
        "instant_expand" => Ok(Some(defaults.instant_expand.to_string())),
        "ignore_fullscreen" => Ok(Some(defaults.ignore_fullscreen.to_string())),
        "system_tray_enabled" => Ok(Some(defaults.system_tray_enabled.to_string())),
        "inline_date_format" => Ok(Some(defaults.inline_date_format)),
        "inline_time_format" => Ok(Some(defaults.inline_time_format)),
        "inline_datetime_format" => Ok(Some(defaults.inline_datetime_format)),
        "audio_theme" => Ok(Some(defaults.audio_theme.as_str().to_string())),
        "audio_volume" => Ok(Some(defaults.audio_volume.to_string())),
        "ai_provider" => Ok(defaults.ai_provider),
        "ai_model" => Ok(defaults.ai_model),
        "ai_custom_endpoint" => Ok(defaults.ai_custom_endpoint),
        "voice_always_on" | "always_on" | "voice_listen" | "voice_wake" => Ok(None),
        "voice_ptt_hotkey" => Ok(Some(defaults.voice_ptt_hotkey)),
        "voice_handsfree_hotkey" => Ok(Some(defaults.voice_handsfree_hotkey)),
        "voice_dictionary" => Ok(Some(defaults.voice_dictionary)),
        "voice_input_device" => Ok(defaults.voice_input_device),
        "pause_media_while_dictating" => Ok(Some(defaults.pause_media_while_dictating.to_string())),
        "tui_cursor_style" => Ok(Some(match defaults.tui_cursor_style {
            TuiCursorStyle::Block => "block".to_string(),
            TuiCursorStyle::Bar => "bar".to_string(),
            TuiCursorStyle::Underscore => "underscore".to_string(),
        })),
        _ => {
            let diag = crate::diagnostic::Diagnostic::problem(format!(
                "{actual_key} is not a valid configuration setting"
            ))
            .suggest(actual_key, &Settings::ALL_KEYS)
            .help("To view all available settings and their values, run: taurine config list")
            .render();
            Err(Error::Config(diag))
        }
    }
}
