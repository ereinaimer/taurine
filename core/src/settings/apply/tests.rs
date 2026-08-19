use super::*;
use crate::settings::{
    InlineAiTriggerMode, RpcMode, SettingKey, Settings, SettingsManager, SpinnerStyle,
};
use crate::testing::open_test_db;
use std::collections::HashSet;

#[test]
fn invalid_wpm_is_rejected() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    assert!(apply_setting_input_with_manager(&manager, "wpm", Some("fast")).is_err());
}

#[test]
fn valid_spinner_style_arc_is_accepted() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    apply_setting_input_with_manager(&manager, "spinner_style", Some("arc")).unwrap();

    assert_eq!(manager.load_all().spinner_style, SpinnerStyle::Arc);
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
        default_setting_input("inline_tab_completion_enabled").unwrap(),
        Some("true".to_string())
    );
    assert_eq!(default_setting_input("ai_custom_endpoint").unwrap(), None);
}

#[test]
fn resetting_inline_tab_completion_restores_default() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting("inline_tab_completion_enabled", false)
        .unwrap();

    let default_value = default_setting_input("inline_tab_completion_enabled").unwrap();
    apply_setting_input_with_manager(
        &manager,
        "inline_tab_completion_enabled",
        default_value.as_deref(),
    )
    .unwrap();

    assert!(manager.load_all().inline_tab_completion_enabled);
}

#[test]
fn resetting_inline_case_transform_restores_default() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting("inline_case_transform_enabled", false)
        .unwrap();

    let default_value = default_setting_input("inline_case_transform_enabled").unwrap();
    apply_setting_input_with_manager(
        &manager,
        "inline_case_transform_enabled",
        default_value.as_deref(),
    )
    .unwrap();

    assert!(manager.load_all().inline_case_transform_enabled);
}

#[test]
fn test_apply_clipboard_restore_delay_ms() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    // Apply a valid value
    apply_setting_input_with_manager(&manager, "clipboard_restore_delay_ms", Some("1200")).unwrap();
    assert_eq!(manager.load_all().clipboard_restore_delay_ms, 1200);

    // Apply clamped value
    apply_setting_input_with_manager(&manager, "clipboard_restore_delay_ms", Some("3000")).unwrap();
    assert_eq!(manager.load_all().clipboard_restore_delay_ms, 2000);
}

#[test]
fn test_inline_emoji_settings() {
    let _guard = crate::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    // Verify default setting input
    assert_eq!(
        default_setting_input("inline_emoji_enabled").unwrap(),
        Some("true".to_string())
    );
    assert_eq!(
        default_setting_input("inline_emoji_trigger_char").unwrap(),
        Some(":".to_string())
    );

    // Apply new values
    apply_setting_input_with_manager(&manager, "inline_emoji_enabled", Some("false")).unwrap();
    apply_setting_input_with_manager(&manager, "inline_emoji_trigger_char", Some(";")).unwrap();

    let loaded = manager.load_all();
    assert!(!loaded.inline_emoji_enabled);
    assert_eq!(loaded.inline_emoji_trigger_char, ';');

    assert!(!crate::settings::get_cached_inline_emoji_enabled());
    assert_eq!(crate::settings::get_cached_inline_emoji_trigger_char(), ';');
}

#[test]
fn toggling_system_tray_enabled_persists_changed_value() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    apply_setting_input_with_manager(&manager, "system_tray_enabled", Some("false")).unwrap();

    assert!(!manager.load_all().system_tray_enabled);
}

#[test]
fn test_inline_currency_to_words_settings() {
    let _guard = crate::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    assert_eq!(
        default_setting_input("inline_currency_to_words_enabled").unwrap(),
        Some("false".to_string())
    );

    apply_setting_input_with_manager(&manager, "inline_currency_to_words_enabled", Some("true"))
        .unwrap();

    let loaded = manager.load_all();
    assert!(loaded.inline_currency_to_words_enabled);
    assert!(crate::settings::get_cached_inline_currency_to_words_enabled());
}

#[test]
fn test_inline_dictionary_settings() {
    let (_dir, conn) = crate::testing::open_test_db();
    let manager = SettingsManager::new(&conn);

    assert_eq!(
        default_setting_input("inline_dictionary_enabled").unwrap(),
        Some("true".to_string())
    );

    apply_setting_input_with_manager(&manager, "inline_dictionary_enabled", Some("false")).unwrap();

    let loaded = manager.load_all();
    assert!(!loaded.inline_dictionary_enabled);
    assert!(!crate::settings::get_cached_inline_dictionary_enabled());
}

#[test]
fn asymmetric_mode_rejects_equal_open_close() {
    let settings = Settings {
        inline_ai_trigger_open: ">".to_string(),
        inline_ai_trigger_close: ">".to_string(),
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "inline_ai_trigger_mode", "asymmetric");
    assert!(result.is_err());
}

#[test]
fn asymmetric_mode_accepts_different_open_close() {
    let settings = Settings {
        inline_ai_trigger_open: ">>".to_string(),
        inline_ai_trigger_close: "<<".to_string(),
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "inline_ai_trigger_mode", "asymmetric");
    assert!(result.is_ok());
}

#[test]
fn non_conflict_key_passes_through() {
    let settings = Settings::default();
    let result = validate_delimiter_conflicts(&settings, "wpm", "60");
    assert!(result.is_ok());
}

#[test]
fn asymmetric_mode_rejects_equal_open_close_through_apply() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting("inline_ai_trigger_open", ">".to_string())
        .unwrap();
    manager
        .update_setting("inline_ai_trigger_close", ">".to_string())
        .unwrap();
    let result =
        apply_setting_input_with_manager(&manager, "inline_ai_trigger_mode", Some("asymmetric"));
    assert!(result.is_err());
}

#[test]
fn asymmetric_mode_accepts_different_open_close_through_apply() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting("inline_ai_trigger_open", ">>".to_string())
        .unwrap();
    manager
        .update_setting("inline_ai_trigger_close", "<<".to_string())
        .unwrap();
    apply_setting_input_with_manager(&manager, "inline_ai_trigger_mode", Some("asymmetric"))
        .unwrap();
    assert_eq!(
        manager.load_all().inline_ai_trigger_mode,
        InlineAiTriggerMode::Asymmetric
    );
}

#[test]
fn setting_key_all_has_40_unique_storage_keys() {
    assert_eq!(SettingKey::ALL.len(), 40);

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
        ("pause_notifications_enabled", "true"),
        ("pause_audio_enabled", "false"),
        ("start_on_boot", "false"),
        ("inline_tab_completion_enabled", "false"),
        ("inline_case_transform_enabled", "false"),
        ("wpm", "100"),
        ("spinner_style", "arc"),
        ("ai_provider", "gemini"),
        ("ai_model", "gpt-test"),
        ("ai_custom_endpoint", "https://example.com"),
        ("inline_ai_trigger_mode", "symmetric"),
        ("inline_ai_trigger", "="),
        ("inline_ai_trigger_open", "[["),
        ("inline_ai_trigger_close", "]]"),
        ("clipboard_restore_delay_ms", "1000"),
        ("instant_expand", "true"),
        ("ignore_fullscreen", "false"),
        ("rpc_mode", "tcp"),
        ("rpc_host", "10.0.0.1"),
        ("rpc_port", "6000"),
        ("scripts_enabled", "false"),
        ("script_timeout", "30"),
        ("ai_temperature", "0.5"),
        ("ai_max_tokens", "1024"),
        ("ai_system_prompt", "test prompt"),
        ("clipboard_history_enabled", "false"),
        ("clipboard_history_retention_secs", "600"),
        ("inline_emoji_enabled", "false"),
        ("inline_emoji_trigger_char", "!"),
        ("system_tray_enabled", "false"),
        ("inline_datetime_enabled", "false"),
        ("inline_datetime_date_format", "DD/MM/YYYY"),
        ("inline_datetime_time_format", "HH:mm"),
        ("inline_datetime_datetime_format", "DD/MM/YYYY HH:mm"),
        ("inline_datetime_dialect", "us"),
        ("inline_currency_to_words_enabled", "true"),
        ("inline_dictionary_enabled", "false"),
        ("notify_on_update", "false"),
    ];

    for (key, value) in sweep {
        apply_setting_input_with_manager(&manager, key, Some(value))
            .unwrap_or_else(|e| panic!("failed to set {key} to {value}: {e}"));
    }

    let expected = Settings {
        pause_hotkey: "Ctrl + Shift + P".to_string(),
        pause_notifications_enabled: true,
        pause_audio_enabled: false,
        start_on_boot: false,
        inline_tab_completion_enabled: false,
        inline_case_transform_enabled: false,
        wpm: 100,
        spinner_style: SpinnerStyle::Arc,
        ai_provider: Some("gemini".to_string()),
        ai_model: Some("gpt-test".to_string()),
        ai_custom_endpoint: Some("https://example.com".to_string()),
        inline_ai_trigger_mode: InlineAiTriggerMode::Symmetric,
        inline_ai_trigger: "=".to_string(),
        inline_ai_trigger_open: "[[".to_string(),
        inline_ai_trigger_close: "]]".to_string(),
        clipboard_restore_delay_ms: 1000,
        instant_expand: true,
        rpc_mode: RpcMode::Tcp,
        rpc_host: "10.0.0.1".to_string(),
        rpc_port: 6000,
        ignore_fullscreen: false,
        script_timeout: 30,
        ai_temperature: Some(0.5),
        ai_max_tokens: Some(1024),
        ai_system_prompt: Some("test prompt".to_string()),
        auto_update: true,
        clipboard_history_enabled: false,
        clipboard_history_retention_secs: 600,
        inline_emoji_enabled: false,
        inline_emoji_trigger_char: '!',
        scripts_enabled: false,
        system_tray_enabled: false,
        inline_datetime_enabled: false,
        inline_datetime_date_format: "DD/MM/YYYY".to_string(),
        inline_datetime_time_format: "HH:mm".to_string(),
        inline_datetime_datetime_format: "DD/MM/YYYY HH:mm".to_string(),
        inline_datetime_dialect: "us".to_string(),
        inline_currency_to_words_enabled: true,
        inline_dictionary_enabled: false,
        notify_on_update: false,
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
