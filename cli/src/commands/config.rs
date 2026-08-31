use taurine_core::db::init;
use taurine_core::settings::{
    SettingKey, Settings, SettingsManager, apply_setting_input, reset_setting_to_default,
};

pub fn execute_list(json: bool) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let settings = manager.load_all();

    if json {
        println!("{}", format_settings_json(&settings));
        return Ok(());
    }

    // Helper to build each line as (key, value) pair
    let pairs: Vec<(&str, String)> = vec![
        ("pause_hotkey", settings.pause_hotkey.clone()),
        (
            "pause_notifications_enabled",
            settings.pause_notifications_enabled.to_string(),
        ),
        (
            "pause_audio_enabled",
            settings.pause_audio_enabled.to_string(),
        ),
        ("audio_theme", settings.audio_theme.as_str().to_string()),
        ("audio_volume", settings.audio_volume.to_string()),
        ("start_on_boot", settings.start_on_boot.to_string()),
        ("auto_update", settings.auto_update.to_string()),
        ("notify_on_update", settings.notify_on_update.to_string()),
        (
            "system_tray_enabled",
            settings.system_tray_enabled.to_string(),
        ),
        (
            "inline_tab_completion_enabled",
            settings.inline_tab_completion_enabled.to_string(),
        ),
        (
            "inline_case_transform_enabled",
            settings.inline_case_transform_enabled.to_string(),
        ),
        ("wpm", settings.wpm.to_string()),
        (
            "spinner_style",
            format!("{:?}", settings.spinner_style).to_lowercase(),
        ),
        (
            "ai_provider",
            render_optional_setting(settings.ai_provider.as_deref()).to_string(),
        ),
        (
            "ai_model",
            render_optional_setting(settings.ai_model.as_deref()).to_string(),
        ),
        (
            "ai_temperature",
            render_optional_setting(settings.ai_temperature.map(|v| v.to_string()).as_deref())
                .to_string(),
        ),
        (
            "ai_max_tokens",
            render_optional_setting(settings.ai_max_tokens.map(|v| v.to_string()).as_deref())
                .to_string(),
        ),
        (
            "ai_system_prompt",
            render_optional_setting(settings.ai_system_prompt.as_deref()).to_string(),
        ),
        (
            "ai_custom_endpoint",
            render_optional_setting(settings.ai_custom_endpoint.as_deref()).to_string(),
        ),
        (
            "inline_ai_trigger_mode",
            format!("{:?}", settings.inline_ai_trigger_mode).to_lowercase(),
        ),
        ("inline_ai_trigger", settings.inline_ai_trigger.clone()),
        (
            "inline_ai_trigger_open",
            settings.inline_ai_trigger_open.clone(),
        ),
        (
            "inline_ai_trigger_close",
            settings.inline_ai_trigger_close.clone(),
        ),
        (
            "clipboard_restore_delay_ms",
            settings.clipboard_restore_delay_ms.to_string(),
        ),
        (
            "clipboard_history_enabled",
            settings.clipboard_history_enabled.to_string(),
        ),
        (
            "clipboard_history_retention_secs",
            settings.clipboard_history_retention_secs.to_string(),
        ),
        ("scripts_enabled", settings.scripts_enabled.to_string()),
        ("instant_expand", settings.instant_expand.to_string()),
        ("ignore_fullscreen", settings.ignore_fullscreen.to_string()),
        (
            "inline_emoji_enabled",
            settings.inline_emoji_enabled.to_string(),
        ),
        (
            "inline_emoji_trigger_char",
            settings.inline_emoji_trigger_char.to_string(),
        ),
        (
            "inline_datetime_enabled",
            settings.inline_datetime_enabled.to_string(),
        ),
        (
            "inline_datetime_date_format",
            settings.inline_datetime_date_format.clone(),
        ),
        (
            "inline_datetime_time_format",
            settings.inline_datetime_time_format.clone(),
        ),
        (
            "inline_datetime_datetime_format",
            settings.inline_datetime_datetime_format.clone(),
        ),
        (
            "inline_datetime_dialect",
            settings.inline_datetime_dialect.clone(),
        ),
        (
            "inline_currency_to_words_enabled",
            settings.inline_currency_to_words_enabled.to_string(),
        ),
        (
            "inline_dictionary_enabled",
            settings.inline_dictionary_enabled.to_string(),
        ),
        (
            "inline_dictionary_mode",
            format!("{:?}", settings.inline_dictionary_mode).to_lowercase(),
        ),
        (
            "rpc_mode",
            format!("{:?}", settings.rpc_mode).to_lowercase(),
        ),
    ];

    let show_tcp_settings = settings.rpc_mode == taurine_core::settings::RpcMode::Tcp;

    // Calculate key column width
    let max_key_len = pairs.iter().map(|(k, _)| k.len()).max().unwrap_or(10);
    let pad = 2;

    for (key, value) in &pairs {
        println!(
            "{:<kw$}{:pad$}{}",
            key,
            "",
            value,
            kw = max_key_len,
            pad = pad
        );
    }

    if show_tcp_settings {
        println!(
            "{:<kw$}{:pad$}{}",
            "rpc_host",
            "",
            settings.rpc_host,
            kw = max_key_len,
            pad = pad
        );
        println!(
            "{:<kw$}{:pad$}{}",
            "rpc_port",
            "",
            settings.rpc_port,
            kw = max_key_len,
            pad = pad
        );
    }
    println!(
        "{:<kw$}{:pad$}{}",
        "script_timeout",
        "",
        settings.script_timeout,
        kw = max_key_len,
        pad = pad
    );

    Ok(())
}

pub fn execute_set(key: String, value: String, json: bool) -> taurine_core::error::Result<()> {
    let actual_key = Settings::resolve_key(&key);
    apply_setting_input(actual_key, Some(&value))?;

    if json {
        println!(
            "{}",
            serde_json::json!({"status": "updated", "key": actual_key})
        );
    }
    Ok(())
}
pub fn execute_reset(key: String, json: bool) -> taurine_core::error::Result<()> {
    let actual_key = Settings::resolve_key(&key);
    reset_setting_to_default(actual_key)?;

    if json {
        println!(
            "{}",
            serde_json::json!({"status": "reset", "key": actual_key})
        );
    }
    Ok(())
}

pub fn execute_reset_all(json: bool) -> taurine_core::error::Result<()> {
    for key in SettingKey::ALL {
        reset_setting_to_default(key.storage_key())?;
    }

    if json {
        println!("{}", serde_json::json!({"status": "reset_all"}));
    }
    Ok(())
}

fn render_optional_setting(value: Option<&str>) -> &str {
    value.filter(|v| !v.is_empty()).unwrap_or("<unset>")
}

pub fn format_settings_json(settings: &Settings) -> String {
    let val = serde_json::to_value(settings).unwrap_or_default();
    serde_json::to_string(&val).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDbEnv;

    impl Drop for TestDbEnv {
        fn drop(&mut self) {
            // SAFETY: test-only; access to TAURINE_DB_PATH is serialized by
            // crate::commands::TEST_LOCK, so no other thread mutates the
            // variable concurrently with this drop.
            unsafe { std::env::remove_var("TAURINE_DB_PATH") };
        }
    }

    fn with_test_db<R>(f: impl FnOnce() -> R) -> R {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().expect("temp dir");
        let _env = TestDbEnv;
        // SAFETY: test-only; serialized by crate::commands::TEST_LOCK, so
        // exactly one thread manipulates TAURINE_DB_PATH at a time.
        unsafe { std::env::set_var("TAURINE_DB_PATH", dir.path().join("taurine.db")) };
        f()
    }

    #[test]
    fn reset_auto_update_restores_default() {
        let restored = with_test_db(|| -> taurine_core::error::Result<bool> {
            execute_set("auto_update".to_string(), "false".to_string(), false)?;
            execute_reset("auto_update".to_string(), false)?;
            let conn = init::setup()?;
            let manager = SettingsManager::new(&conn);
            Ok(manager.load_all().auto_update)
        });
        assert!(restored.unwrap());
    }

    #[test]
    fn reset_all_restores_every_setting() {
        let restored = with_test_db(|| -> taurine_core::error::Result<(bool, bool, bool)> {
            let conn = init::setup()?;
            let manager = SettingsManager::new(&conn);
            manager.update_setting("auto_update", false)?;
            manager.update_setting("inline_emoji_trigger_char", '!')?;
            manager.update_setting("scripts_enabled", false)?;
            execute_reset_all(false)?;
            let conn = init::setup()?;
            let manager = SettingsManager::new(&conn);
            let settings = manager.load_all();
            Ok((
                settings.auto_update,
                settings.inline_emoji_trigger_char == ':',
                settings.scripts_enabled,
            ))
        });
        let (auto_update, emoji_trigger_char, scripts_enabled) = restored.unwrap();
        assert!(auto_update);
        assert!(emoji_trigger_char);
        assert!(scripts_enabled);
    }

    #[test]
    fn set_inline_ai_trigger_keys_persist() {
        let persisted = with_test_db(
            || -> taurine_core::error::Result<(bool, bool, bool, bool)> {
                execute_set(
                    "inline_ai_trigger_mode".to_string(),
                    "symmetric".to_string(),
                    false,
                )?;
                execute_set("inline_ai_trigger".to_string(), "=".to_string(), false)?;
                execute_set(
                    "inline_ai_trigger_open".to_string(),
                    "[[".to_string(),
                    false,
                )?;
                execute_set(
                    "inline_ai_trigger_close".to_string(),
                    "]]".to_string(),
                    false,
                )?;
                let conn = init::setup()?;
                let manager = SettingsManager::new(&conn);
                let settings = manager.load_all();
                Ok((
                    settings.inline_ai_trigger_mode
                        == taurine_core::settings::InlineAiTriggerMode::Symmetric,
                    settings.inline_ai_trigger == "=",
                    settings.inline_ai_trigger_open == "[[",
                    settings.inline_ai_trigger_close == "]]",
                ))
            },
        );
        let (mode, trigger, open, close) = persisted.unwrap();
        assert!(mode);
        assert!(trigger);
        assert!(open);
        assert!(close);
    }

    #[test]
    fn set_unknown_key_returns_error() {
        let is_err =
            with_test_db(|| execute_set("bogus_key".to_string(), "x".to_string(), false).is_err());
        assert!(is_err);
    }

    #[test]
    fn reset_unknown_key_returns_error() {
        let is_err = with_test_db(|| execute_reset("bogus_key".to_string(), false).is_err());
        assert!(is_err);
    }

    #[test]
    fn render_optional_setting_uses_unset_placeholder() {
        assert_eq!(render_optional_setting(None), "<unset>");
        assert_eq!(render_optional_setting(Some("")), "<unset>");
        assert_eq!(render_optional_setting(Some("openai")), "openai");
    }

    #[test]
    fn core_ai_provider_parser_validates_config_value() {
        assert_eq!(
            taurine_core::ai::AiProvider::try_from("gemini")
                .expect("gemini should parse")
                .as_str(),
            "gemini"
        );
        assert!(
            taurine_core::ai::AiProvider::try_from("unknown").is_err(),
            "invalid provider must be rejected"
        );
    }

    #[test]
    fn test_settings_json_serializes_all_keys() {
        let settings = Settings::default();
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let map = value.as_object().unwrap();

        assert!(map.contains_key("pause_hotkey"));
        assert!(map.contains_key("wpm"));
        assert!(map.contains_key("spinner_style"));
        assert!(map.contains_key("rpc_mode"));
        assert!(map.contains_key("script_timeout"));
        assert!(map.contains_key("system_tray_enabled"));

        assert_eq!(map["wpm"], 60);
        assert_eq!(map["rpc_mode"], "socket");
        assert_eq!(map["system_tray_enabled"], true);
    }

    #[test]
    fn test_settings_json_optional_fields() {
        let settings = Settings {
            ai_provider: Some("openai".to_string()),
            ai_model: Some("gpt-4".to_string()),
            ai_temperature: Some(0.7),
            ai_max_tokens: Some(2048),
            ai_custom_endpoint: None,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["ai_provider"], "openai");
        assert_eq!(value["ai_model"], "gpt-4");
        assert_eq!(value["ai_custom_endpoint"], serde_json::Value::Null);
    }

    #[test]
    fn test_settings_json_with_unset_optionals() {
        let settings = Settings {
            ai_provider: None,
            ai_model: None,
            ai_temperature: None,
            ai_max_tokens: None,
            ai_system_prompt: None,
            ai_custom_endpoint: None,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["ai_provider"], serde_json::Value::Null);
        assert_eq!(value["ai_model"], serde_json::Value::Null);
        assert_eq!(value["ai_temperature"], serde_json::Value::Null);
    }

    #[test]
    fn test_settings_all_enum_variants_serialize() {
        let settings = Settings {
            spinner_style: taurine_core::settings::SpinnerStyle::Arc,
            inline_ai_trigger_mode: taurine_core::settings::InlineAiTriggerMode::Symmetric,
            rpc_mode: taurine_core::settings::RpcMode::Tcp,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["spinner_style"], "arc");
        assert_eq!(value["inline_ai_trigger_mode"], "symmetric");
        assert_eq!(value["rpc_mode"], "tcp");
    }

    #[test]
    fn test_set_audio_theme_persists() {
        let persisted = with_test_db(
            || -> taurine_core::error::Result<taurine_core::settings::AudioTheme> {
                execute_set("audio_theme".to_string(), "arcade".to_string(), false)?;
                let conn = init::setup()?;
                let manager = SettingsManager::new(&conn);
                Ok(manager.load_all().audio_theme)
            },
        );
        assert_eq!(
            persisted.unwrap(),
            taurine_core::settings::AudioTheme::Arcade
        );
    }

    #[test]
    fn test_set_audio_volume_persists() {
        let persisted = with_test_db(|| -> taurine_core::error::Result<u32> {
            execute_set("audio_volume".to_string(), "65".to_string(), false)?;
            let conn = init::setup()?;
            let manager = SettingsManager::new(&conn);
            Ok(manager.load_all().audio_volume)
        });
        assert_eq!(persisted.unwrap(), 65);
    }
}
