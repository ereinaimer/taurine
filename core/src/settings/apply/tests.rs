use super::*;
use crate::settings::{InlineAiTriggerMode, Settings, SettingsManager, SpinnerStyle};
use crate::testing::open_test_db;

#[test]
fn toggling_inline_history_persists_changed_value() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);

    apply_setting_input_with_manager(&manager, "inline_history_enabled", Some("false")).unwrap();

    assert!(!manager.load_all().inline_history_enabled);
}

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
        default_setting_input("trigger_char").unwrap(),
        Some(">".to_string())
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
fn resetting_trigger_char_restores_default() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager.update_setting("trigger_char", ';').unwrap();

    let default_value = default_setting_input("trigger_char").unwrap();
    apply_setting_input_with_manager(&manager, "trigger_char", default_value.as_deref()).unwrap();

    assert_eq!(
        manager.load_all().trigger_char,
        Settings::default().trigger_char
    );
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
fn resetting_rpc_token_generates_new_uuid() {
    let default_val = default_setting_input("rpc_token").unwrap();
    assert!(default_val.is_some());
    let token_str = default_val.unwrap();
    assert!(!token_str.is_empty());
    assert!(uuid::Uuid::parse_str(&token_str).is_ok());
}

#[test]
fn test_inline_emoji_settings() {
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
fn trigger_char_conflicts_with_inline_ai_trigger_open() {
    let settings = Settings {
        inline_ai_trigger_open: ">".to_string(),
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "trigger_char", ">");
    assert!(result.is_err());
    match result.unwrap_err() {
        crate::error::Error::Config(msg) => assert_eq!(
            msg,
            "'trigger_char' value '>' conflicts with 'inline_ai_trigger_open' value '>'"
        ),
        _ => panic!("expected Config error"),
    }
}

#[test]
fn trigger_char_conflicts_with_inline_ai_trigger_close() {
    let settings = Settings {
        inline_ai_trigger_close: ">".to_string(),
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "trigger_char", ">");
    assert!(result.is_err());
}

#[test]
fn trigger_char_conflicts_with_symmetric_trigger() {
    let settings = Settings {
        inline_ai_trigger_mode: InlineAiTriggerMode::Symmetric,
        inline_ai_trigger: ">".to_string(),
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "trigger_char", ">");
    assert!(result.is_err());
}

#[test]
fn trigger_char_no_conflict_with_multi_char_open() {
    let settings = Settings {
        inline_ai_trigger_open: ">>".to_string(),
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "trigger_char", ">");
    assert!(result.is_ok());
}

#[test]
fn inline_ai_trigger_open_conflicts_with_trigger_char() {
    let settings = Settings::default();
    let result = validate_delimiter_conflicts(&settings, "inline_ai_trigger_open", ">");
    assert!(result.is_err());
}

#[test]
fn inline_ai_trigger_close_conflicts_with_trigger_char() {
    let settings = Settings::default();
    let result = validate_delimiter_conflicts(&settings, "inline_ai_trigger_close", ">");
    assert!(result.is_err());
}

#[test]
fn symmetric_trigger_conflicts_with_trigger_char() {
    let settings = Settings {
        inline_ai_trigger_mode: InlineAiTriggerMode::Symmetric,
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "inline_ai_trigger", ">");
    assert!(result.is_err());
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
fn trigger_char_change_with_no_conflicts_succeeds() {
    let settings = Settings {
        inline_ai_trigger_open: ">>".to_string(),
        inline_ai_trigger_close: "<<".to_string(),
        inline_ai_trigger: "^".to_string(),
        inline_ai_trigger_mode: InlineAiTriggerMode::Asymmetric,
        ..Settings::default()
    };
    let result = validate_delimiter_conflicts(&settings, "trigger_char", ";");
    assert!(result.is_ok());
}

#[test]
fn non_conflict_key_passes_through() {
    let settings = Settings::default();
    let result = validate_delimiter_conflicts(&settings, "wpm", "60");
    assert!(result.is_ok());
}

#[test]
fn trigger_char_conflict_with_open_through_apply() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting("inline_ai_trigger_open", ">".to_string())
        .unwrap();
    let result = apply_setting_input_with_manager(&manager, "trigger_char", Some(">"));
    assert!(result.is_err());
}

#[test]
fn inline_ai_trigger_open_conflict_through_apply() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting("trigger_char", ">".to_string())
        .unwrap();
    let result = apply_setting_input_with_manager(&manager, "inline_ai_trigger_open", Some(">"));
    assert!(result.is_err());
}

#[test]
fn inline_ai_trigger_conflict_through_apply() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    manager
        .update_setting("inline_ai_trigger_mode", InlineAiTriggerMode::Symmetric)
        .unwrap();
    let result = apply_setting_input_with_manager(&manager, "inline_ai_trigger", Some(">"));
    assert!(result.is_err());
}

#[test]
fn trigger_char_no_conflict_through_apply() {
    let (_dir, conn) = open_test_db();
    let manager = SettingsManager::new(&conn);
    apply_setting_input_with_manager(&manager, "trigger_char", Some("|")).unwrap();
    assert_eq!(manager.load_all().trigger_char, '|');
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
