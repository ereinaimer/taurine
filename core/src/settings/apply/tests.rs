use super::*;
use crate::settings::{AudioTheme, SettingKey, Settings, SettingsManager, TuiCursorStyle};
use crate::testing::open_test_db;
use std::collections::HashSet;

#[test]
fn test_removed_settings_rejected_as_unknown() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    for key in [
        "wpm",
        "spinner_style",
        "inline_ai_enabled",
        "inline_tab_completion_enabled",
        "inline_case_transform_enabled",
        "scripts_enabled",
        "script_timeout",
        "clipboard_history_enabled",
        "inline_emoji_enabled",
        "inline_datetime_enabled",
        "inline_currency_to_words_enabled",
        "inline_dictionary_enabled",
        "inline_dictionary_mode",
    ] {
        let err = apply_setting_input_with_manager(&manager, key, Some("x")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("is not a valid configuration setting"),
            "expected unknown-setting diagnostic for {key}, got: {msg}"
        );
    }
}

#[test]
fn valid_audio_theme_arcade_is_accepted() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    apply_setting_input_with_manager(&manager, "audio_theme", Some("arcade")).unwrap();

    assert_eq!(manager.load_all().audio_theme, AudioTheme::Arcade);
}

#[test]
fn valid_tui_cursor_style_underscore_is_accepted() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    apply_setting_input_with_manager(&manager, "tui_cursor_style", Some("underscore")).unwrap();

    assert_eq!(
        manager.load_all().tui_cursor_style,
        TuiCursorStyle::Underscore
    );
}

#[test]
fn test_invalid_tui_cursor_style_diagnostic() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    let err =
        apply_setting_input_with_manager(&manager, "tui_cursor_style", Some("beam")).unwrap_err();
    let msg = err.to_string();

    assert!(
        msg.contains("beam is not an available cursor style"),
        "expected problem message, got: {msg}"
    );
    assert!(
        msg.contains("block, bar, underscore"),
        "expected options listing block, bar, underscore, got: {msg}"
    );
}

#[test]
fn ai_custom_endpoint_can_be_cleared() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting(
            "ai_custom_endpoint",
            Some("https://example.com".to_string()),
        )
        .unwrap();

    apply_setting_input_with_manager(&manager, "ai_custom_endpoint", None).unwrap();

    assert_eq!(manager.load_all().ai_custom_endpoint, None);
}

#[test]
fn pause_hotkey_uses_shared_validation() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    assert!(
        apply_setting_input_with_manager(&manager, "pause_hotkey", Some("not+a+hotkey+???"))
            .is_err()
    );
}

#[test]
fn default_setting_input_uses_canonical_defaults() {
    assert_eq!(
        default_setting_input("pause_hotkey").unwrap(),
        Some("Alt + `".to_string())
    );
    assert_eq!(
        default_setting_input("start_on_boot").unwrap(),
        Some("true".to_string())
    );
    assert_eq!(default_setting_input("ai_custom_endpoint").unwrap(), None);
}

#[test]
fn resetting_start_on_boot_restores_default() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager.update_setting("start_on_boot", false).unwrap();

    let default_value = default_setting_input("start_on_boot").unwrap();
    apply_setting_input_with_manager(&manager, "start_on_boot", default_value.as_deref()).unwrap();

    assert!(manager.load_all().start_on_boot);
}

#[test]
fn test_inline_date_time_formats_settings() {
    let _guard = crate::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    assert_eq!(
        default_setting_input("inline_date_format").unwrap(),
        Some("MMMM D, YYYY".to_string())
    );
    assert_eq!(
        default_setting_input("inline_time_format").unwrap(),
        Some("h:mm A".to_string())
    );
    assert_eq!(
        default_setting_input("inline_datetime_format").unwrap(),
        Some("MMMM D, YYYY 'at' h:mm A".to_string())
    );

    apply_setting_input_with_manager(&manager, "inline_date_format", Some("YYYY-MM-DD")).unwrap();
    apply_setting_input_with_manager(&manager, "inline_time_format", Some("HH:mm")).unwrap();
    apply_setting_input_with_manager(&manager, "inline_datetime_format", Some("YYYY-MM-DD HH:mm"))
        .unwrap();

    let loaded = manager.load_all();
    assert_eq!(loaded.inline_date_format, "YYYY-MM-DD");
    assert_eq!(loaded.inline_time_format, "HH:mm");
    assert_eq!(loaded.inline_datetime_format, "YYYY-MM-DD HH:mm");

    assert_eq!(
        crate::settings::get_cached_inline_date_format(),
        "YYYY-MM-DD"
    );
    assert_eq!(crate::settings::get_cached_inline_time_format(), "HH:mm");
    assert_eq!(
        crate::settings::get_cached_inline_datetime_format(),
        "YYYY-MM-DD HH:mm"
    );
}

#[test]
fn toggling_system_tray_enabled_persists_changed_value() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    apply_setting_input_with_manager(&manager, "system_tray_enabled", Some("false")).unwrap();

    assert!(!manager.load_all().system_tray_enabled);
}

#[test]
fn setting_key_all_has_unique_storage_keys() {
    assert_eq!(SettingKey::ALL.len(), 20);

    let mut seen = HashSet::new();
    for key in SettingKey::ALL {
        assert!(
            seen.insert(key.storage_key()),
            "duplicate storage key: {}",
            key.storage_key()
        );
    }
}

#[test]
fn every_setting_key_has_a_default_input() {
    for key in SettingKey::ALL {
        assert!(
            default_setting_input(key.storage_key()).is_ok(),
            "no default input for {}",
            key.storage_key()
        );
    }
}

#[test]
fn sweep_covers_defaults_set_and_reset_for_all_keys() {
    let _guard = crate::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    let sweep: &[(&str, &str)] = &[
        ("pause_hotkey", "Ctrl + Shift + P"),
        ("start_on_boot", "false"),
        ("auto_update", "false"),
        ("instant_expand", "true"),
        ("ignore_fullscreen", "false"),
        ("system_tray_enabled", "false"),
        ("inline_date_format", "DD/MM/YYYY"),
        ("inline_time_format", "HH:mm"),
        ("inline_datetime_format", "DD/MM/YYYY HH:mm"),
        ("audio_theme", "arcade"),
        ("audio_volume", "80"),
        ("ai_provider", "gemini"),
        ("ai_model", "gpt-test"),
        ("ai_custom_endpoint", "https://example.com"),
        ("voice_ptt_hotkey", "ctrl+space"),
        ("voice_handsfree_hotkey", "ctrl+alt+space"),
        ("voice_dictionary", "Rust, Taurine"),
        ("voice_input_device", "External Mic"),
        ("pause_media_while_dictating", "false"),
        ("tui_cursor_style", "underscore"),
    ];

    for (key, value) in sweep {
        apply_setting_input_with_manager(&manager, key, Some(value))
            .unwrap_or_else(|e| panic!("failed to set {key} to {value}: {e}"));
    }

    let expected = Settings {
        pause_hotkey: "Ctrl + Shift + P".to_string(),
        start_on_boot: false,
        auto_update: false,
        instant_expand: true,
        ignore_fullscreen: false,
        system_tray_enabled: false,
        inline_date_format: "DD/MM/YYYY".to_string(),
        inline_time_format: "HH:mm".to_string(),
        inline_datetime_format: "DD/MM/YYYY HH:mm".to_string(),
        audio_theme: AudioTheme::Arcade,
        audio_volume: 80,
        ai_provider: Some("gemini".to_string()),
        ai_model: Some("gpt-test".to_string()),
        ai_custom_endpoint: Some("https://example.com".to_string()),
        voice_ptt_hotkey: "ctrl+space".to_string(),
        voice_handsfree_hotkey: "ctrl+alt+space".to_string(),
        voice_dictionary: "Rust, Taurine".to_string(),
        voice_input_device: Some("External Mic".to_string()),
        pause_media_while_dictating: false,
        tui_cursor_style: TuiCursorStyle::Underscore,
    };
    assert_eq!(manager.load_all(), expected);

    for key in SettingKey::ALL {
        let storage_key = key.storage_key();
        let default_value = default_setting_input(storage_key)
            .unwrap_or_else(|e| panic!("no default for {storage_key}: {e}"));
        apply_setting_input_with_manager(&manager, storage_key, default_value.as_deref())
            .unwrap_or_else(|e| panic!("failed to reset {storage_key}: {e}"));

        let current = manager.load_all();
        let expected_default = Settings::default();
        let cur = serde_json::to_value(&current).unwrap();
        let exp = serde_json::to_value(&expected_default).unwrap();
        assert_eq!(
            cur[storage_key], exp[storage_key],
            "key {storage_key} was not reset to its default"
        );
    }

    assert_eq!(manager.load_all(), Settings::default());
}

#[test]
fn test_audio_theme_apply_and_defaults() {
    let defaults = Settings::default();
    assert_eq!(defaults.audio_theme, AudioTheme::Minimal);
    assert_eq!(defaults.audio_theme.as_str(), "minimal");

    let valid_themes = [
        "minimal",
        "arcade",
        "mechanical",
        "organic",
        "scifi",
        "rubber",
        "zen",
    ];

    for theme_str in valid_themes {
        let parsed: AudioTheme = theme_str.parse().expect("valid theme parse");
        assert_eq!(parsed.as_str(), theme_str);
    }

    assert!("invalid_theme".parse::<AudioTheme>().is_err());
}

#[test]
fn test_audio_volume_settings() {
    let _guard = crate::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    assert_eq!(Settings::default().audio_volume, 50);
    assert_eq!(Settings::sanitize_audio_volume(50), 50);
    assert_eq!(Settings::sanitize_audio_volume(0), 0);
    assert_eq!(Settings::sanitize_audio_volume(100), 100);
    assert_eq!(Settings::sanitize_audio_volume(150), 100);

    apply_setting_input_with_manager(&manager, "audio_volume", Some("75")).unwrap();
    assert_eq!(manager.load_all().audio_volume, 75);
    assert_eq!(crate::settings::get_cached_audio_volume(), 75);

    apply_setting_input_with_manager(&manager, "audio_volume", Some("200")).unwrap();
    assert_eq!(manager.load_all().audio_volume, 100);
    assert_eq!(crate::settings::get_cached_audio_volume(), 100);

    assert!(apply_setting_input_with_manager(&manager, "audio_volume", Some("invalid")).is_err());
}

#[test]
fn test_unknown_setting_key_diagnostic() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    let err = apply_setting_input_with_manager(&manager, "theme", Some("arcade")).unwrap_err();
    let msg = err.to_string();

    assert!(
        msg.contains("theme is not a valid configuration setting"),
        "expected problem message, got: {msg}"
    );
    assert!(
        msg.contains("Did you mean audio_theme?"),
        "expected suggestion for audio_theme, got: {msg}"
    );
    assert!(
        msg.contains("To view all available settings and their values, run: taurine config list"),
        "expected guidance to taurine config list, got: {msg}"
    );
}

#[test]
fn test_invalid_audio_theme_diagnostic() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    let err = apply_setting_input_with_manager(&manager, "audio_theme", Some("synth")).unwrap_err();
    let msg = err.to_string();

    assert!(
        msg.contains("synth is not an available audio theme"),
        "expected problem message, got: {msg}"
    );
    assert!(
        msg.contains("minimal, arcade, mechanical, organic, scifi, rubber, zen"),
        "expected options listing all 7 themes, got: {msg}"
    );
    assert!(
        msg.contains("taurine config set audio_theme"),
        "expected example command, got: {msg}"
    );
}

#[test]
fn test_invalid_boolean_diagnostic() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    let err = apply_setting_input_with_manager(&manager, "start_on_boot", Some("yes")).unwrap_err();
    let msg = err.to_string();

    assert!(
        msg.contains("yes is not a valid boolean value"),
        "expected problem message, got: {msg}"
    );
    assert!(
        msg.contains("Expected true or false (or 1 / 0)"),
        "expected help message, got: {msg}"
    );
}

#[test]
fn test_removed_rpc_keys_rejected_as_unknown() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    for key in ["rpc_mode", "rpc_host", "rpc_port"] {
        let err = apply_setting_input_with_manager(&manager, key, Some("x")).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("is not a valid configuration setting"),
            "expected unknown-setting diagnostic for {key}, got: {msg}"
        );
    }
}

#[test]
fn settings_version_bumps_on_cached_write() {
    let v0 = crate::settings::settings_version();
    crate::settings::set_cached_audio_volume(crate::settings::get_cached_audio_volume());
    assert!(
        crate::settings::settings_version() > v0,
        "cached write must bump the settings version"
    );
}

#[test]
fn test_voice_always_on_reports_removal() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    let err =
        apply_setting_input_with_manager(&manager, "voice_always_on", Some("true")).unwrap_err();
    assert!(
        err.to_string()
            .contains("superseded by on-demand voice hotkeys"),
        "expected removal message, got: {err}"
    );
}

#[test]
fn test_invalid_voice_hotkey_diagnostic() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    let err =
        apply_setting_input_with_manager(&manager, "voice_ptt_hotkey", Some("not_a_valid_hotkey"))
            .unwrap_err();
    let msg = err.to_string();

    assert!(
        msg.contains("invalid voice_ptt_hotkey"),
        "expected invalid hotkey message, got: {msg}"
    );
}
