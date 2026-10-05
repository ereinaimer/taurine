use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use taurine_core::db::crud::{InvocationType, TriggerAliasRow, TriggerListItem, TriggerType};

fn invocation_for(trigger_type: TriggerType) -> InvocationType {
    match trigger_type {
        TriggerType::Word => InvocationType::Word,
        TriggerType::Hotkey => InvocationType::Hotkey,
        TriggerType::Regex => InvocationType::Regex,
    }
}

fn alias_row(trigger_id: &str, trigger_type: TriggerType, trigger: &str) -> TriggerAliasRow {
    TriggerAliasRow {
        id: format!("alias-{trigger}"),
        trigger_id: trigger_id.to_string(),
        invocation: trigger.to_string(),
        invocation_type: invocation_for(trigger_type),
        require_confirmation: false,
        strict_threshold: None,
        created_at: 0,
    }
}

#[allow(clippy::too_many_arguments)]
fn list_item(
    id: &str,
    description: Option<&str>,
    trigger_type: TriggerType,
    trigger: &str,
    output: &str,
    action_type: &str,
    target_os: &str,
    usage_count: i64,
    script_content: Option<&str>,
) -> TriggerListItem {
    TriggerListItem {
        id: id.to_string(),
        name: String::new(),
        description: description.map(str::to_string),
        invocations: vec![alias_row(id, trigger_type, trigger)],
        display: trigger.to_string(),
        output: output.to_string(),
        action_type: action_type.to_string(),
        target_os: target_os.to_string(),
        only_apps: None,
        except_apps: None,
        auto_case: false,
        is_enabled: true,
        usage_count,
        last_used_at: None,
        created_at: 0,
        tags: "[]".to_string(),
        script_content: script_content.map(str::to_string),
        interpreter: None,
        behavior: None,
    }
}

fn sample_state() -> LibraryPageState {
    let mut state = LibraryPageState::default();
    state.replace_items(vec![
        LibraryTrigger::single(list_item(
            "id-gm",
            None,
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        )),
        LibraryTrigger::single(list_item(
            "id-deploy",
            None,
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "linux",
            4,
            Some("npm run build && npm publish"),
        )),
        LibraryTrigger::single(list_item(
            "id-alt+r",
            Some("Open Reddit"),
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        )),
    ]);
    state
}

#[test]
fn word_text_maps_to_text_trigger() {
    let item = LibraryTrigger::single(list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ));
    assert_eq!(item.kind_label(), "text trigger");
}

#[test]
fn word_script_maps_to_text_script() {
    let item = LibraryTrigger::single(list_item(
        "id-deploy",
        None,
        TriggerType::Word,
        "deploy",
        "[Script: bash]",
        "script",
        "all",
        4,
        Some("npm publish"),
    ));
    assert_eq!(item.kind_label(), "text script");
}

#[test]
fn hotkey_text_maps_to_hotkey_trigger() {
    let item = LibraryTrigger::single(list_item(
        "id-thanks",
        None,
        TriggerType::Hotkey,
        "alt+t",
        "Thanks!",
        "text",
        "all",
        12,
        None,
    ));
    assert_eq!(item.kind_label(), "hotkey trigger");
}

#[test]
fn all_invocation_action_combos_map_to_distinct_labels() {
    use taurine_core::db::crud::InvocationType;

    let cases = [
        (InvocationType::Word, "text", "text trigger"),
        (InvocationType::Hotkey, "text", "hotkey trigger"),
        (InvocationType::Regex, "text", "regex trigger"),
        (InvocationType::Voice, "text", "voice trigger"),
        (InvocationType::Word, "script", "text script"),
        (InvocationType::Hotkey, "script", "hotkey script"),
        (InvocationType::Regex, "script", "regex script"),
        (InvocationType::Voice, "script", "voice script"),
    ];
    for (invocation, action, label) in cases {
        assert_eq!(
            LibraryKind::from_invocation(invocation, action).label(),
            label
        );
    }
}

#[test]
fn hotkey_script_maps_to_hotkey_script() {
    let item = LibraryTrigger::single(list_item(
        "id-alt+r",
        None,
        TriggerType::Hotkey,
        "alt+r",
        "[Script: powershell]",
        "script",
        "win",
        6,
        Some("Start-Process https://reddit.com"),
    ));
    assert_eq!(item.kind_label(), "hotkey script");
}

#[test]
fn preview_prefers_description_before_other_content() {
    let item = LibraryTrigger::single(list_item(
        "id-alt+r",
        Some("Open Reddit"),
        TriggerType::Hotkey,
        "alt+r",
        "[Script: powershell]",
        "script",
        "win",
        6,
        Some("Start-Process https://reddit.com"),
    ));

    assert_eq!(item.preview(), "Open Reddit");
}

#[test]
fn placeholder_script_description_does_not_block_real_script_preview() {
    let item = LibraryTrigger::single(list_item(
        "id-alt+r",
        Some("Shell script (CLI argument)"),
        TriggerType::Hotkey,
        "alt+r",
        "[Script: powershell]",
        "script",
        "win",
        6,
        Some("Start-Process https://reddit.com"),
    ));

    assert_eq!(item.preview(), "Start-Process https://reddit.com");
}

#[test]
fn preview_falls_back_to_text_output_when_description_is_empty() {
    let item = LibraryTrigger::single(list_item(
        "id-gm",
        Some("   "),
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ));

    assert_eq!(item.preview(), "Good Morning");
}

#[test]
fn preview_falls_back_to_script_content_when_description_is_empty() {
    let item = LibraryTrigger::single(list_item(
        "id-alt+r",
        None,
        TriggerType::Hotkey,
        "alt+r",
        "[Script: powershell]",
        "script",
        "win",
        6,
        Some("Start-Process https://reddit.com"),
    ));

    assert_eq!(item.preview(), "Start-Process https://reddit.com");
}

#[test]
fn script_preview_does_not_use_script_language_placeholder() {
    let item = LibraryTrigger::single(list_item(
        "id-deploy",
        None,
        TriggerType::Word,
        "deploy",
        "[Script: bash]",
        "script",
        "all",
        4,
        Some("npm run build && npm publish"),
    ));

    assert_ne!(item.preview(), "[Script: bash]");
    assert_eq!(item.preview(), "npm run build && npm publish");
}

#[test]
fn script_preview_does_not_use_shell_script_description_placeholder() {
    let item = LibraryTrigger::single(list_item(
        "id-deploy",
        Some("Shell script (CLI argument)"),
        TriggerType::Word,
        "deploy",
        "[Script: bash]",
        "script",
        "all",
        4,
        Some("npm run build && npm publish"),
    ));

    assert_ne!(item.preview(), "Shell script (CLI argument)");
    assert_eq!(item.preview(), "npm run build && npm publish");
}

#[test]
fn empty_script_content_falls_back_safely() {
    let item = LibraryTrigger::single(list_item(
        "id-deploy",
        Some("Shell script (CLI argument)"),
        TriggerType::Word,
        "deploy",
        "[Script: bash]",
        "script",
        "all",
        4,
        Some("   "),
    ));

    assert_eq!(item.preview(), "Script content unavailable.");
}

#[test]
fn search_matches_trigger() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");

    let mut item = list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    );
    item.invocations
        .push(alias_row("id-gm", TriggerType::Word, "goodmorning"));
    let mut state = LibraryPageState::default();
    state.replace_items(LibraryTrigger::expand(item));
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "goodmorning".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "goodmorning");
}

#[test]
fn search_matches_preview() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "publish".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "deploy");
}

#[test]
fn search_matches_description_when_available() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "open".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
}

#[test]
fn search_matches_name_when_it_differs_from_trigger() {
    let mut state = LibraryPageState::default();
    state.replace_items(vec![LibraryTrigger::single(TriggerListItem {
        id: "id-alt+r".to_string(),
        name: "Reddit opener".to_string(),
        description: Some("Open Reddit".to_string()),
        invocations: vec![alias_row("id-alt+r", TriggerType::Hotkey, "alt+r")],
        display: "alt+r".to_string(),
        output: "[Script: powershell]".to_string(),
        action_type: "script".to_string(),
        target_os: "win".to_string(),
        only_apps: None,
        except_apps: None,
        auto_case: false,
        is_enabled: true,
        usage_count: 6,
        last_used_at: None,
        created_at: 0,
        tags: "[]".to_string(),
        script_content: Some("Start-Process https://reddit.com".to_string()),
        interpreter: None,
        behavior: None,
    })]);
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "reddit opener".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
}

#[test]
fn search_matches_script_content_even_when_description_is_visible() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "start-process".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
}

#[test]
fn search_matches_kind_label() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "hotkey".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "alt+r");
}

#[test]
fn search_matches_target_os() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "windows".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().target_os, "windows");
}

#[test]
fn search_is_case_insensitive() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "GOOD".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::SHIFT));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
}

#[test]
fn selection_clamps_at_bounds() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(state.selected_index(), Some(0));

    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(state.selected_index(), Some(2));
}

#[test]
fn jk_keys_filter_instead_of_moving() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

    assert!(state.is_search_active());
    assert_eq!(state.search_query(), "j");
    assert_eq!(state.filtered_len(), 0);
}

#[test]
fn selection_moves_to_first_match_when_filter_removes_selected_item() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "good".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.selected_index(), Some(0));
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
}

#[test]
fn empty_list_reports_empty_state() {
    let state = LibraryPageState::default();
    assert_eq!(state.empty_state_message(), Some("No triggers yet."));
}

#[test]
fn no_match_search_reports_no_match_state() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "zzz".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(
        state.empty_state_message(),
        Some("No triggers match your search.")
    );
}

#[test]
fn pressing_enter_is_reserved_while_editor_is_removed() {
    let mut state = sample_state();

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.pending_delete().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn pressing_n_types_into_search() {
    let mut state = sample_state();
    assert!(!state.is_search_active());

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    assert!(interaction.pending_delete().is_none());
    assert!(state.modal().is_none());
    assert!(state.is_search_active());
    assert_eq!(state.search_query(), "n");
}

#[test]
fn pressing_x_types_into_search() {
    let mut state = sample_state();
    assert!(!state.is_search_active());

    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    assert!(state.modal().is_none());
    assert!(state.is_search_active());
    assert_eq!(state.search_query(), "x");
}

#[test]
fn pressing_i_types_into_search() {
    let mut state = sample_state();
    assert!(!state.is_search_active());

    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    assert!(state.modal().is_none());
    assert!(state.is_search_active());
    assert_eq!(state.search_query(), "i");
}

#[test]
fn import_modal_defaults_match_current_behavior() {
    let mut state = sample_state();
    state.open_import_modal();

    let Some(LibraryModal::Import(modal)) = state.modal() else {
        panic!("expected import modal");
    };
    assert_eq!(modal.path(), "");
    assert_eq!(modal.password_display_value(), "");
    assert_eq!(modal.conflict_mode(), LibraryImportConflictMode::Skip);
}

#[test]
fn import_modal_detects_passwordless_file_from_header() {
    let path = std::env::temp_dir().join(format!("taurine-detect-nopw-{}.tau", std::process::id()));
    let blob = taurine_core::exchange::encode_exchange_blob(
        &taurine_core::exchange::ExchangePayload::new(vec![]),
        None,
    )
    .unwrap();
    std::fs::write(&path, &blob).unwrap();

    let state = LibraryImportModalState::with_path(path.to_string_lossy().into_owned());

    std::fs::remove_file(&path).ok();
    assert_eq!(state.is_encrypted(), Some(false));
}

#[test]
fn import_modal_detects_password_protected_file_from_header() {
    let path = std::env::temp_dir().join(format!("taurine-detect-pw-{}.tau", std::process::id()));
    let blob = taurine_core::exchange::encode_exchange_blob(
        &taurine_core::exchange::ExchangePayload::new(vec![]),
        Some("hunter22"),
    )
    .unwrap();
    std::fs::write(&path, &blob).unwrap();

    let state = LibraryImportModalState::with_path(path.to_string_lossy().into_owned());

    std::fs::remove_file(&path).ok();
    assert_eq!(state.is_encrypted(), Some(true));
}

#[test]
fn import_modal_treats_foreign_file_as_unknown() {
    let path =
        std::env::temp_dir().join(format!("taurine-detect-foreign-{}.tau", std::process::id()));
    std::fs::write(&path, b"definitely not a taurine file").unwrap();

    let state = LibraryImportModalState::with_path(path.to_string_lossy().into_owned());

    std::fs::remove_file(&path).ok();
    assert_eq!(state.is_encrypted(), None);
}

#[test]
fn import_modal_requires_non_empty_path() {
    let mut state = sample_state();
    state.open_import_modal();

    let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
        panic!("expected import modal");
    };
    // Tab through all fields to ActionButton: Path -> Password -> ConflictMode -> ActionButton
    for _ in 0..3 {
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    }
    modal.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.pending_import_prepare().is_none());
    assert_eq!(modal.error(), Some("Import path is required."));
}

#[test]
fn import_modal_password_field_accepts_input_and_stays_masked() {
    let mut state = sample_state();
    state.open_import_modal();

    let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
        panic!("expected import modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for ch in "secret".chars() {
        modal.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(modal.password_display_value(), "******");
}

#[test]
fn import_modal_conflict_selector_uses_safe_modes() {
    let mut state = sample_state();
    state.open_import_modal();

    let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
        panic!("expected import modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

    let selector = modal.selector().expect("conflict selector");
    assert_eq!(selector.title(), "Select Conflict Mode");
    assert_eq!(selector.options, vec!["skip", "overwrite"]);
}

#[test]
fn import_modal_owns_input_and_keeps_search_inactive() {
    let mut state = sample_state();
    state.open_import_modal();

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(matches!(state.modal(), Some(LibraryModal::Import(_))));
}

#[test]
fn import_result_modal_uses_reliable_result_lines() {
    let outcome = LibraryImportOutcome::new(12);
    let mut state = sample_state();

    state.open_import_result_modal(&outcome);

    let Some(LibraryModal::ImportResult(modal)) = state.modal() else {
        panic!("expected import result modal");
    };
    assert_eq!(modal.lines()[0], "Imported 12 trigger(s).");
}

#[test]
fn import_result_modal_closes_on_enter() {
    let outcome = LibraryImportOutcome::new(3);
    let mut state = sample_state();
    state.open_import_result_modal(&outcome);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn import_result_modal_closes_on_escape() {
    let outcome = LibraryImportOutcome::new(3);
    let mut state = sample_state();
    state.open_import_result_modal(&outcome);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn import_result_modal_owns_input_and_keeps_search_inactive() {
    let outcome = LibraryImportOutcome::new(3);
    let mut state = sample_state();
    state.open_import_result_modal(&outcome);

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(matches!(state.modal(), Some(LibraryModal::ImportResult(_))));
}

#[test]
fn export_result_modal_body_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers are exported to: backup.tau as an encrypted export."
    );
}

#[test]
fn export_result_modal_closes_on_enter() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");
    state.open_export_result_modal(&path);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn export_result_modal_closes_on_escape() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");
    state.open_export_result_modal(&path);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn export_result_modal_owns_input_and_keeps_search_inactive() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");
    state.open_export_result_modal(&path);

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(matches!(state.modal(), Some(LibraryModal::ExportResult(_))));
}

#[test]
fn export_modal_defaults_match_cli_behavior() {
    let mut state = sample_state();
    state.open_export_modal();

    let Some(LibraryModal::Export(modal)) = state.modal() else {
        panic!("expected export modal");
    };
    assert!(modal.path().ends_with(".tau"));
    assert_eq!(modal.password_display_value(), "");
}

#[test]
fn export_modal_tab_moves_through_all_fields() {
    let mut state = sample_state();
    state.open_export_modal();

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryExportModalField::Password);

    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryExportModalField::ActionButton);
}

#[test]
fn export_modal_rejects_short_password() {
    let mut state = sample_state();
    state.open_export_modal();

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    // Tab to Password: Path -> Password -> ActionButton
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for ch in ['s', 'h', 'o', 'r', 't'] {
        modal.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.pending_export().is_none());
    assert_eq!(
        modal.error(),
        Some("Encryption password must be at least 8 characters long")
    );
}

#[test]
fn export_modal_without_password_creates_passwordless_export() {
    let mut state = sample_state();
    state.open_export_modal();

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    // Tab to ActionButton without typing a password: Path -> Password -> ActionButton
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let pending = interaction.pending_export().expect("pending export");
    assert_eq!(pending.password, None);
}

#[test]
fn export_modal_password_field_stores_typed_characters() {
    let mut state = sample_state();
    state.open_export_modal();

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    assert_eq!(modal.password_display_value(), "***");
}

#[test]
fn enter_on_confirm_creates_pending_export_with_password() {
    let mut state = sample_state();
    state.open_export_modal();

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    for ch in "hunter222".chars() {
        modal.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    // Tab to ActionButton
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let pending = interaction.pending_export().expect("pending export");
    assert_eq!(pending.password.as_deref(), Some("hunter222"));
}

#[test]
fn export_modal_owns_input_and_keeps_search_inactive() {
    let mut state = sample_state();
    state.open_export_modal();

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(matches!(state.modal(), Some(LibraryModal::Export(_))));
}

#[test]
fn pressing_d_types_into_search() {
    let mut state = sample_state();
    assert!(!state.is_search_active());

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert!(state.modal().is_none());
    assert!(state.is_search_active());
    assert_eq!(state.search_query(), "d");
}

#[test]
fn pressing_escape_closes_open_modal() {
    let mut state = sample_state();
    state.open_delete_modal_for_selected();

    state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(state.modal().is_none());
}

#[test]
fn delete_confirmation_owns_input_and_keeps_search_inactive() {
    let mut state = sample_state();
    state.open_delete_modal_for_selected();

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(matches!(
        state.modal(),
        Some(LibraryModal::ConfirmDelete(_))
    ));
}

#[test]
fn delete_confirmation_cancel_closes_modal() {
    let mut state = sample_state();
    state.open_delete_modal_for_selected();

    state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    assert!(state.modal().is_none());
}

#[test]
fn delete_confirmation_enter_creates_pending_delete() {
    let mut state = sample_state();
    state.open_delete_modal_for_selected();

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let pending = interaction.pending_delete().expect("pending delete");
    assert_eq!(pending.trigger_id, "id-alt+r");
    assert_eq!(pending.restore_index(), 0);
}

#[test]
fn select_after_delete_chooses_nearest_remaining_item() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.replace_items(vec![
        LibraryTrigger::single(list_item(
            "id-gm",
            None,
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        )),
        LibraryTrigger::single(list_item(
            "id-deploy",
            None,
            TriggerType::Word,
            "deploy",
            "[Script: bash]",
            "script",
            "linux",
            4,
            Some("npm run build && npm publish"),
        )),
    ]);

    state.select_after_delete(2);

    assert_eq!(state.selected_index(), Some(1));
    assert_eq!(state.item_at_filtered(1).unwrap().trigger(), "gm");
}

#[test]
fn modal_keeps_library_selection_stable_after_close() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let selected_before = state.selected_index();

    state.open_delete_modal_for_selected();
    state.clear_modal();

    assert_eq!(state.selected_index(), selected_before);
    assert_eq!(state.search_query(), "");
}

fn alias_fixture(
    trigger_id: &str,
    invocation: &str,
    invocation_type: InvocationType,
    require_confirmation: bool,
) -> TriggerAliasRow {
    TriggerAliasRow {
        id: format!("alias-{invocation}"),
        trigger_id: trigger_id.to_string(),
        invocation: invocation.to_string(),
        invocation_type,
        require_confirmation,
        strict_threshold: None,
        created_at: 0,
    }
}

fn multi_alias_list_item() -> TriggerListItem {
    TriggerListItem {
        id: "id-multi".to_string(),
        name: String::new(),
        description: None,
        invocations: vec![
            alias_fixture("id-multi", "gs", InvocationType::Word, false),
            alias_fixture("id-multi", "gst", InvocationType::Word, false),
            alias_fixture("id-multi", "ctrl+g", InvocationType::Hotkey, false),
        ],
        display: "gs".to_string(),
        output: "git status".to_string(),
        action_type: "text".to_string(),
        target_os: "all".to_string(),
        only_apps: None,
        except_apps: None,
        auto_case: false,
        is_enabled: true,
        usage_count: 3,
        last_used_at: None,
        created_at: 0,
        tags: "[]".to_string(),
        script_content: None,
        interpreter: None,
        behavior: None,
    }
}

#[test]
fn expanded_rows_carry_all_aliases_and_usage() {
    let rows = LibraryTrigger::expand(multi_alias_list_item());

    assert!(
        rows.iter()
            .all(|row| row.aliases().join(", ") == "gs, gst, ctrl+g")
    );
    assert_eq!(rows[0].usage_count(), 3);
    assert_eq!(rows[0].last_used_at(), None);
    assert!(rows[0].tags().is_empty());
    assert!(!rows[0].auto_case());
    assert_eq!(rows[0].interpreter(), None);
}

#[test]
fn expanded_rows_carry_auto_case_and_script_meta() {
    use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

    let mut item = multi_alias_list_item();
    item.auto_case = true;
    item.interpreter = Some(ScriptInterpreter::Bash);
    item.behavior = Some(ScriptBehavior::Silent);
    let rows = LibraryTrigger::expand(item);

    assert!(rows.iter().all(|row| row.auto_case()));
    assert_eq!(rows[0].interpreter(), Some(ScriptInterpreter::Bash));
    assert_eq!(rows[0].behavior(), Some(ScriptBehavior::Silent));
}

#[test]
fn alias_row_hides_selected_trigger() {
    let aliases = vec!["gs".to_string(), "gst".to_string(), "ctrl+g".to_string()];
    assert_eq!(
        detail::sibling_aliases(&aliases, "gst"),
        vec!["gs", "ctrl+g"]
    );
    assert_eq!(
        detail::sibling_aliases(&aliases, "gs"),
        vec!["gst", "ctrl+g"]
    );
    assert!(detail::sibling_aliases(&["solo".to_string()], "solo").is_empty());
}

#[test]
fn parse_tags_handles_stored_shapes() {
    assert!(parse_tags("[]").is_empty());
    assert_eq!(parse_tags(r#"["a", "b"]"#), vec!["a", "b"]);
    assert_eq!(parse_tags("bogus"), Vec::<String>::new());
    assert_eq!(parse_tags(""), Vec::<String>::new());
}

#[test]
fn header_hit_region_for_toggle() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    // Center content at x=25 width=31: [ON ] toggle owns the last 5 cells.
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            51,
            1
        ),
        Some(detail::DetailHit::EnableToggle)
    );
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            50,
            1
        ),
        None
    );
}

#[test]
fn name_edit_click_places_caret_and_arrows_move_it() {
    let mut state = sample_state();
    state.start_name_edit_at(2);
    assert_eq!(state.name_edit().expect("editing").field().cursor(), 2);

    state.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(state.name_edit().expect("editing").field().text(), "alXt+r");

    state.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(state.name_edit().expect("editing").field().text(), "lXt+r");

    state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(state.name_edit().expect("editing").field().text(), "lXtr");
}

#[test]
fn header_toggle_hit_for_text_trigger_row() {
    let mut state = LibraryPageState::default();
    state.replace_items(vec![LibraryTrigger::single(list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))]);
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            52,
            1
        ),
        Some(detail::DetailHit::EnableToggle)
    );
}

#[test]
fn info_rows_carry_properties_and_raw_usage() {
    let item = LibraryTrigger::single(list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ));
    let rows = props::info_rows(&item);
    // Base properties first, raw usage appended; nothing derived.
    assert_eq!(rows[0].0, "Auto case");
    assert!(rows.iter().any(|(key, _)| *key == "Usage"));
    assert!(rows.iter().any(|(key, _)| *key == "Last used"));
    assert!(rows.iter().any(|(key, _)| *key == "Created"));
    assert!(rows.iter().all(|(key, _)| *key != "Frequency"));
    assert!(rows.iter().all(|(key, _)| *key != "Keystrokes saved"));
    assert!(rows.iter().all(|(key, _)| *key != "Time saved"));
}

#[test]
fn pack_tag_chips_fits_and_breaks() {
    let tags = vec!["work".to_string(), "powershell".to_string()];
    assert_eq!(
        props::pack_tag_chips(&tags, 30),
        vec!["#work".to_string(), "#powershell".to_string()]
    );
    assert_eq!(props::pack_tag_chips(&tags, 6), vec!["#work".to_string()]);
    assert!(props::pack_tag_chips(&tags, 2).is_empty());
    assert!(props::pack_tag_chips(&[], 30).is_empty());
}

#[test]
fn usage_toggle_starts_collapsed_and_flips() {
    let mut state = sample_state();
    assert!(!state.usage_expanded());
    state.toggle_usage();
    assert!(state.usage_expanded());
    state.toggle_usage();
    assert!(!state.usage_expanded());
}

#[test]
fn usage_toggle_hit_only_on_toggle_row() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Selected hotkey script carries five base rows plus the Tags row:
    // toggle at pane top + 14, whatever the pane widths are.
    let item = state
        .item_at_filtered(state.selected_index().unwrap())
        .unwrap();
    assert_eq!(props::usage_toggle_offset(item), 14);
    let content = props::props_content(area, state.split_ratio(), state.detail_ratio());
    let (x, toggle_y) = (content.x, content.y + 14);
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            x,
            toggle_y
        ),
        Some(props::PropsHit::UsageToggle)
    );
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            x,
            toggle_y - 1
        ),
        None
    );
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            66,
            1
        ),
        Some(detail::DetailHit::EnableToggle)
    );
}

#[test]
fn description_edit_hits_row_and_commits() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    // Description row at content.y(1) + 2.
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            30,
            3
        ),
        Some(detail::DetailHit::DescriptionEdit)
    );

    let mut state = sample_state();
    let interaction = state.start_description_edit();
    assert!(interaction.pending_edit().is_none());
    assert!(state.edit().is_some());
    for ch in " hi".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let interaction = state.commit_edit();
    let pending = interaction.pending_edit().expect("pending edit");
    assert!(matches!(
        &pending.field,
        crate::widgets::library::actions::EditedField::Description(Some(text))
            if text.ends_with(" hi")
    ));
    assert!(state.edit().is_none());
}

#[test]
fn description_edit_blank_clears_to_none() {
    let mut state = sample_state();
    state.start_description_edit();
    for _ in 0..64 {
        state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    // Blank is a valid unset (not an error): persists as no description,
    // silently, with no warning anywhere.
    let interaction = state.commit_edit();
    let pending = interaction.pending_edit().expect("pending edit");
    assert!(matches!(
        &pending.field,
        crate::widgets::library::actions::EditedField::Description(None)
    ));
    assert!(state.edit().is_none());
    assert!(state.status_message().is_none());
}

#[test]
fn content_edit_typing_newline_arrows_and_tab_commit() {
    let mut state = sample_state();
    let interaction = state.start_content_edit_at(0, 0);
    assert!(interaction.pending_edit().is_none());
    assert!(state.edit().is_some());

    state.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('Y'), KeyModifiers::NONE));
    let (row, col) = state.edit().expect("editing").body().cursor();
    assert_eq!((row, col), (1, 1));

    state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(state.edit().expect("editing").body().cursor(), (0, 1));
    // Selection never moves on arrows inside the body editor.
    assert_eq!(state.selected_index(), Some(0));

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    let pending = interaction.pending_edit().expect("pending edit");
    assert!(matches!(
        &pending.field,
        crate::widgets::library::actions::EditedField::Content(_)
    ));
    assert!(state.edit().is_none());
}

#[test]
fn content_edit_esc_discards() {
    let mut state = sample_state();
    state.start_content_edit_at(0, 0);
    state.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(state.edit().is_none());
}

#[test]
fn content_edit_hit_maps_wrapped_rows_to_source() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    // Box text starts at (25, 8): first text row hits source row 0.
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            27,
            8
        ),
        Some(detail::DetailHit::ContentEditAt { row: 0, col: 0 })
    );
    // Border frame itself is not editable.
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            23,
            7
        ),
        None
    );
}

#[test]
fn content_source_cell_folds_wrapped_chunks() {
    let item = LibraryTrigger::single(list_item(
        "id-long",
        None,
        TriggerType::Word,
        "long",
        &"x".repeat(100),
        "text",
        "all",
        0,
        None,
    ));
    // 100 chars at width 33 wrap to four visual rows of one source row.
    assert_eq!(detail::content_source_cell(&item, 33, 0, 5), (0, 5));
    assert_eq!(detail::content_source_cell(&item, 33, 3, 0), (0, 99));
    // Past the end lands at the end of the last source row.
    assert_eq!(detail::content_source_cell(&item, 33, 9, 0), (0, 100));
}

#[test]
fn content_visual_cursor_round_trips_wrapped_rows() {
    let lines = vec!["x".repeat(100)];
    // Chunk boundaries at width 33: visual rows 0..4.
    assert_eq!(detail::content_visual_cursor(&lines, 0, 5, 33), (0, 5));
    assert_eq!(detail::content_visual_cursor(&lines, 0, 33, 33), (1, 0));
    assert_eq!(detail::content_visual_cursor(&lines, 0, 99, 33), (3, 0));
    // Second source row offsets by the first row's chunk count.
    let lines = vec!["ab".to_string(), "cdef".to_string()];
    assert_eq!(detail::content_visual_cursor(&lines, 1, 2, 33), (1, 2));
    // Empty input stays at the origin.
    let empty: Vec<String> = Vec::new();
    assert_eq!(detail::content_visual_cursor(&empty, 0, 0, 33), (0, 0));
}

#[test]
fn display_name_falls_back_to_trigger_when_unnamed() {
    let item = LibraryTrigger::single(list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ));
    assert_eq!(item.display_name(), "gm");

    let mut named = multi_alias_list_item();
    named.name = "Git status".to_string();
    let row = LibraryTrigger::single(named);
    assert_eq!(row.display_name(), "Git status");
}

#[test]
fn content_section_uses_output_not_description() {
    let item = LibraryTrigger::single(list_item(
        "id-gm",
        Some("A greeting"),
        TriggerType::Word,
        "gm",
        "Good Morning\nSecond line",
        "text",
        "all",
        9,
        None,
    ));
    assert!(item.content().contains("Good Morning"));
    assert!(!item.content().contains("A greeting"));
}

#[test]
fn content_section_uses_script_source_for_scripts() {
    let item = LibraryTrigger::single(list_item(
        "id-deploy",
        None,
        TriggerType::Word,
        "deploy",
        "[Script: bash]",
        "script",
        "linux",
        4,
        Some("npm run build\nnpm publish"),
    ));
    assert!(item.content().contains("npm run build"));
}

#[test]
fn property_rows_use_border_token_for_empty_values() {
    let item = LibraryTrigger::single(list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ));
    let rows = detail::property_rows(&item);
    let get = |label: &str| {
        rows.iter()
            .find(|(key, _)| *key == label)
            .map(|(_, value)| value.clone())
            .expect("row present")
    };
    assert_eq!(get("Auto case"), "off");
    assert_eq!(get("Allow on"), detail::EMPTY_TOKEN);
    assert_eq!(get("Block on"), detail::EMPTY_TOKEN);
    assert_eq!(get("Alias"), detail::EMPTY_TOKEN);
    // Usage rows live in the usage section now, not base properties.
    assert!(rows.iter().all(|(key, _)| *key != "Usage"));
    assert!(rows.iter().all(|(key, _)| *key != "Last used"));
    // Text triggers carry no Confirm row.
    assert!(rows.iter().all(|(key, _)| *key != "Confirm"));

    let usage = props::usage_rows(&item);
    let get_usage = |label: &str| {
        usage
            .iter()
            .find(|(key, _)| *key == label)
            .map(|(_, value)| value.clone())
            .expect("row present")
    };
    assert_eq!(get_usage("Usage"), "9 times");
    assert_eq!(get_usage("Last used"), detail::EMPTY_TOKEN);
}

#[test]
fn property_rows_show_confirm_for_voice_only() {
    let mut item = multi_alias_list_item();
    item.invocations = vec![alias_fixture(
        "id-v",
        "email me",
        InvocationType::Voice,
        true,
    )];
    item.action_type = "text".to_string();
    let row = LibraryTrigger::single(item);
    let rows = detail::property_rows(&row);
    assert_eq!(
        rows.iter()
            .find(|(key, _)| *key == "Confirm")
            .map(|(_, value)| value.as_str()),
        Some("on")
    );
}

#[test]
fn toggle_selected_enabled_disables_by_default() {
    let state = sample_state();
    let interaction = state.toggle_selected_enabled();
    let pending = interaction.pending_toggle().expect("toggle pending");
    assert!(!pending.enabled);
    assert_eq!(pending.restore_index, 0);
}

#[test]
fn disabled_rows_carry_flag_and_toggle_back_on() {
    let mut item = list_item(
        "id-old",
        None,
        TriggerType::Word,
        "old",
        "Old output",
        "text",
        "all",
        0,
        None,
    );
    item.is_enabled = false;
    let mut state = LibraryPageState::default();
    state.replace_items(LibraryTrigger::expand(item));
    assert!(!state.item_at_filtered(0).unwrap().is_enabled());

    let interaction = state.toggle_selected_enabled();
    let pending = interaction.pending_toggle().expect("toggle pending");
    assert!(pending.enabled);
    assert_eq!(pending.restore_index, 0);
}

#[test]
fn content_scroll_max_counts_overflow_lines() {
    let item = LibraryTrigger::single(list_item(
        "id-big",
        None,
        TriggerType::Word,
        "big",
        "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16",
        "text",
        "all",
        0,
        None,
    ));
    // Text trigger in a 30-row pane fits all 16 lines, nothing scrolls.
    assert_eq!(detail::content_scroll_max(30, 37, &item), 0);
}

#[test]
fn long_content_line_wraps_instead_of_clipping() {
    let long = "x".repeat(200);
    let rows = detail::wrap_content_lines(&long, 37);
    assert_eq!(rows.len(), 6);
    assert!(rows.iter().all(|row| row.chars().count() <= 37));
    assert_eq!(rows.concat(), long);

    let item = LibraryTrigger::single(list_item(
        "id-long",
        None,
        TriggerType::Word,
        "long",
        &long,
        "text",
        "all",
        0,
        None,
    ));
    // Six wrapped rows fit the 16-row window, so nothing scrolls.
    assert_eq!(detail::content_scroll_max(30, 37, &item), 0);
    // Narrow box wraps harder: 200 chars at text width 6 need 34 rows,
    // 16 visible leaves 18 scrollable.
    assert_eq!(detail::content_scroll_max(30, 10, &item), 18);
}

#[test]
fn wrap_keeps_blank_lines_and_tabs() {
    let rows = detail::wrap_content_lines("a\n\n\tb", 37);
    assert_eq!(
        rows,
        vec!["a".to_string(), String::new(), "  b".to_string()]
    );
}

#[test]
fn wrap_moves_whole_words_to_next_row() {
    let text = "even though AGENTS.md prevents you from making commits";
    let rows = detail::wrap_content_lines(text, 20);
    assert!(rows.iter().all(|row| row.chars().count() <= 20));
    // honey: rejoining with the consumed break spaces restores the input.
    assert_eq!(rows.join(" "), text);
    assert_eq!(
        rows,
        vec![
            "even though".to_string(),
            "AGENTS.md prevents".to_string(),
            "you from making".to_string(),
            "commits".to_string(),
        ]
    );
}

#[test]
fn wrap_hard_breaks_only_overlong_words() {
    let path = "C:\\Projects\\rmod\\target\\debug\\build\\output";
    let rows = detail::wrap_content_lines(path, 10);
    assert!(rows.iter().all(|row| row.chars().count() <= 10));
    assert_eq!(rows.concat(), path);
}

#[test]
fn wrap_cursor_round_trips_word_rows() {
    let lines = vec!["ab cd ef".to_string()];
    // Spans at width 5: `ab` 0..2, `cd ef` 3..8.
    assert_eq!(detail::content_visual_cursor(&lines, 0, 4, 5), (1, 1));
    assert_eq!(detail::content_visual_cursor(&lines, 0, 2, 5), (1, 0));
    assert_eq!(
        detail::content_source_cell(
            &LibraryTrigger::single(list_item(
                "id-wrap",
                None,
                TriggerType::Word,
                "wrap",
                "ab cd ef",
                "text",
                "all",
                0,
                None
            )),
            5,
            1,
            1
        ),
        (0, 4)
    );
}

#[test]
fn multi_alias_entry_expands_to_one_row_per_alias() {
    let rows = LibraryTrigger::expand(multi_alias_list_item());

    let triggers: Vec<&str> = rows.iter().map(|row| row.trigger()).collect();
    assert_eq!(triggers, vec!["gs", "gst", "ctrl+g"]);
    assert!(rows.iter().all(|row| row.id() == "id-multi"));
    assert_eq!(rows[0].kind_label(), "text trigger");
    assert_eq!(rows[2].kind_label(), "hotkey trigger");
}

#[test]
fn named_entry_still_expands_per_alias_without_count_suffix() {
    let mut item = multi_alias_list_item();
    item.name = "Git status".to_string();
    item.display = "Git status".to_string();

    let rows = LibraryTrigger::expand(item);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| !row.trigger().contains("(+")));
    assert_eq!(rows[0].trigger(), "gs");
}

#[test]
fn alias_search_matches_only_its_own_row() {
    let mut state = LibraryPageState::default();
    state.replace_items(LibraryTrigger::expand(multi_alias_list_item()));
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "gst".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gst");
}

#[test]
fn entry_without_invocations_falls_back_to_display_row() {
    let mut item = multi_alias_list_item();
    item.invocations.clear();
    item.display = "Git status".to_string();

    let rows = LibraryTrigger::expand(item);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].trigger(), "Git status");
}

#[test]
fn detail_lists_each_invocation_as_type_colon_invocation() {
    assert_eq!(alias_line("word", "hi"), "word: hi");
    assert_eq!(alias_line("voice", "say hi"), "voice: say hi");
}

#[test]
fn arrows_move_selection_while_searching() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(state.selected_index(), Some(1));
    assert!(state.is_search_active());

    state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(state.selected_index(), Some(0));
    assert!(state.is_search_active());
}

#[test]
fn unbound_character_starts_search_immediately() {
    let mut state = sample_state();
    assert!(!state.is_search_active());

    state.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));

    assert!(state.is_search_active());
    assert_eq!(state.search_query(), "g");
}

#[test]
fn reserved_keys_never_start_library_search() {
    for ch in ['1', '2'] {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        assert!(!state.is_search_active());
        assert_eq!(state.search_query(), "");
    }
}

#[test]
fn q_types_into_search() {
    let mut state = sample_state();
    assert!(!state.is_search_active());

    state.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));

    assert!(state.is_search_active());
    assert_eq!(state.search_query(), "q");
}

#[test]
fn click_selects_without_opening() {
    let mut state = sample_state();

    let interaction = state.click_item(1, 0);
    assert_eq!(state.selected_index(), Some(1));
    assert!(interaction.pending_delete().is_none());

    let interaction = state.click_item(1, 0);
    assert_eq!(state.selected_index(), Some(1));
    assert!(interaction.pending_delete().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn search_caret_moves_and_edits_mid_text() {
    let mut state = LibraryPageState::default();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "gmt".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(state.search_query(), "gmt");
    assert_eq!(state.search_field().cursor(), 3);

    state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(state.search_field().cursor(), 1);
    state.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(state.search_query(), "gXmt");

    state.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
    assert_eq!(state.search_field().cursor(), 0);
    state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(state.search_query(), "Xmt");

    state.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    assert_eq!(state.search_field().cursor(), 3);
    state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(state.search_query(), "Xm");
}

#[test]
fn search_bar_click_places_caret() {
    let mut state = LibraryPageState::default();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    for ch in "gm".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    // Search box line starts at the search area plus border and padding.
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    let content = left_content(area, state.split_ratio(), state.detail_ratio());
    let (_, search_area) = content_sections(content, false);
    let line_x = search_area.x + 2;
    assert_eq!(
        list::hit_test(area, &state, line_x, search_area.y),
        Some(list::LibraryHit::SearchAt(0))
    );
    assert_eq!(
        list::hit_test(area, &state, line_x + 1, search_area.y),
        Some(list::LibraryHit::SearchAt(1))
    );
    assert_eq!(
        list::hit_test(area, &state, line_x + 10, search_area.y),
        Some(list::LibraryHit::SearchAt(2))
    );
}

#[test]
fn hit_test_finds_bottom_anchored_rows_and_search() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);

    assert_eq!(list::hit_test(area, &state, 5, 2), None);
    assert_eq!(
        list::hit_test(area, &state, 5, 15),
        Some(list::LibraryHit::Item(0))
    );
    assert_eq!(
        list::hit_test(area, &state, 5, 19),
        Some(list::LibraryHit::Item(1))
    );
    assert_eq!(
        list::hit_test(area, &state, 5, 23),
        Some(list::LibraryHit::Item(2))
    );
    assert_eq!(
        list::hit_test(area, &state, 5, 28),
        Some(list::LibraryHit::SearchAt(0))
    );
    assert_eq!(list::hit_test(area, &state, 79, 5), None);
}

#[test]
fn select_first_window_bottom_pins_cursor_to_first_page() {
    let mut state = sample_state();
    state.select_first_window_bottom(2);
    assert_eq!(state.selected_index(), Some(1));
    assert_eq!(state.visible_window(2), (0, 2));
}

#[test]
fn select_first_window_bottom_on_empty_state_stays_zero() {
    let mut state = LibraryPageState::default();
    state.select_first_window_bottom(5);
    assert_eq!(state.selected_index(), None);
}

fn script_state() -> LibraryPageState {
    use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

    let mut item = list_item(
        "id-sh",
        None,
        TriggerType::Word,
        "sh",
        "[Script: bash]",
        "script",
        "all",
        1,
        Some("echo hi"),
    );
    item.interpreter = Some(ScriptInterpreter::Bash);
    item.behavior = Some(ScriptBehavior::Inline);
    let mut state = LibraryPageState::default();
    state.replace_items(vec![LibraryTrigger::single(item)]);
    state
}

fn open_menu(state: &mut LibraryPageState, kind: HeaderMenuKind) -> LibraryHeaderMenuState {
    state.open_header_menu(kind);
    let Some(LibraryModal::HeaderMenu(menu)) = state.modal() else {
        panic!("header menu open");
    };
    menu.clone()
}

#[test]
fn header_menu_options_carry_descriptions() {
    let mut state = sample_state();
    let menu = open_menu(&mut state, HeaderMenuKind::InvocationType);
    assert_eq!(menu.options().len(), menu.details().len());
    assert!(menu.details().iter().all(|detail| !detail.is_empty()));
}

#[test]
fn overlay_option_hit_maps_two_line_rows() {
    use crate::widgets::util::overlay_option_hit;

    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let counts = [2u16, 2];
    // Popup 56x14 centered: rows run full-bleed from column 22,
    // options start at row 11.
    assert_eq!(overlay_option_hit(area, &counts, 25, 9), None);
    assert_eq!(overlay_option_hit(area, &counts, 25, 10), None);
    assert_eq!(overlay_option_hit(area, &counts, 25, 11), Some(0));
    assert_eq!(overlay_option_hit(area, &counts, 25, 12), Some(0));
    assert_eq!(overlay_option_hit(area, &counts, 25, 13), Some(1));
    assert_eq!(overlay_option_hit(area, &counts, 22, 11), Some(0));
    assert_eq!(overlay_option_hit(area, &counts, 21, 11), None);
    assert_eq!(overlay_option_hit(area, &counts, 25, 15), None);
}

#[test]
fn click_header_menu_confirms_clicked_option() {
    let mut state = script_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    open_menu(&mut state, HeaderMenuKind::Interpreter);
    // Third option (python) spans rows 15-16.
    let interaction = state.click_header_menu(area, 30, 15);
    let pending = interaction.pending_edit().expect("pending language");
    assert_eq!(
        pending.field,
        EditedField::Interpreter(taurine_core::engine::shell::ScriptInterpreter::Python)
    );
    assert!(state.modal().is_none());
}

#[test]
fn click_header_menu_outside_closes_without_pending() {
    let mut state = script_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    open_menu(&mut state, HeaderMenuKind::Interpreter);
    let interaction = state.click_header_menu(area, 0, 0);
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn move_header_menu_cursor_walks_options() {
    let mut state = script_state();
    open_menu(&mut state, HeaderMenuKind::Interpreter);
    state.move_header_menu_cursor(true);
    let Some(LibraryModal::HeaderMenu(menu)) = state.modal() else {
        panic!("header menu open");
    };
    assert_eq!(menu.selected_option(), Some("powershell"));
    state.move_header_menu_cursor(false);
    let Some(LibraryModal::HeaderMenu(menu)) = state.modal() else {
        panic!("header menu open");
    };
    assert_eq!(menu.selected_option(), Some("bash"));
}

#[test]
fn header_menu_type_parks_on_current_value() {
    let mut state = sample_state();
    let menu = open_menu(&mut state, HeaderMenuKind::InvocationType);
    assert_eq!(menu.kind(), HeaderMenuKind::InvocationType);
    assert_eq!(menu.options(), &["word", "hotkey", "regex", "voice"]);
    // First sorted row is the hotkey script.
    assert_eq!(menu.selected(), 1);
}

#[test]
fn header_menu_confirm_unchanged_is_silent() {
    let mut state = sample_state();
    open_menu(&mut state, HeaderMenuKind::InvocationType);
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn header_menu_arrows_wrap_and_enter_retypes() {
    let mut state = sample_state();
    open_menu(&mut state, HeaderMenuKind::InvocationType);
    state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    let Some(LibraryModal::HeaderMenu(menu)) = state.modal() else {
        panic!("header menu open");
    };
    assert_eq!(menu.selected_option(), Some("word"));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let pending = interaction.pending_edit().expect("pending retype");
    assert_eq!(pending.trigger_id, "id-alt+r");
    assert_eq!(pending.trigger, "alt+r");
    assert_eq!(
        pending.field,
        EditedField::InvocationType(taurine_core::db::crud::InvocationType::Word)
    );
    assert!(state.modal().is_none());
}

#[test]
fn header_menu_cursor_wraps_around_both_ends() {
    let mut menu = LibraryHeaderMenuState::new(HeaderMenuKind::Behavior, "inline");
    menu.move_cursor(-1);
    assert_eq!(menu.selected_option(), Some("silent"));
    menu.move_cursor(1);
    assert_eq!(menu.selected_option(), Some("inline"));
}

#[test]
fn header_menu_esc_cancels_without_pending() {
    let mut state = sample_state();
    open_menu(&mut state, HeaderMenuKind::InvocationType);
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn header_menu_typing_never_leaks_into_search() {
    let mut state = sample_state();
    open_menu(&mut state, HeaderMenuKind::InvocationType);
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(state.modal().is_some());
    assert!(!state.is_search_active());
}

#[test]
fn header_menu_interpreter_confirms_language() {
    let mut state = script_state();
    let menu = open_menu(&mut state, HeaderMenuKind::Interpreter);
    assert_eq!(menu.options().len(), 5);
    assert_eq!(menu.selected_option(), Some("bash"));
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let pending = interaction.pending_edit().expect("pending language");
    assert_eq!(
        pending.field,
        EditedField::Interpreter(taurine_core::engine::shell::ScriptInterpreter::Python)
    );
    assert_eq!(pending.restore_index, 0);
    assert!(state.modal().is_none());
}

#[test]
fn header_menu_behavior_parks_on_inline() {
    let mut state = script_state();
    let menu = open_menu(&mut state, HeaderMenuKind::Behavior);
    assert_eq!(menu.options(), &["inline", "silent"]);
    assert_eq!(menu.selected(), 0);
}

#[test]
fn header_menu_missing_metadata_parks_on_first_option() {
    // Sample hotkey script has no interpreter metadata: ─── never
    // matches, so the cursor parks at the head of the list.
    let mut state = sample_state();
    let menu = open_menu(&mut state, HeaderMenuKind::Interpreter);
    assert_eq!(menu.selected(), 0);
}

#[test]
fn open_menu_flips_spawning_button_chevron() {
    use crate::widgets::library::icons::{CHEVRON_DOWN, CHEVRON_UP};
    use ratatui::{Terminal, backend::TestBackend};

    fn chevrons(state: &LibraryPageState) -> (usize, usize) {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                detail::render_detail(
                    frame,
                    frame.area(),
                    &crate::theme::builtin::DARK_THEME,
                    state,
                );
            })
            .expect("test draw");
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        (
            text.matches(CHEVRON_UP).count(),
            text.matches(CHEVRON_DOWN).count(),
        )
    }

    let mut state = script_state();
    assert_eq!(chevrons(&state), (0, 3));
    state.open_header_menu(HeaderMenuKind::Interpreter);
    assert_eq!(chevrons(&state), (1, 2));
}

#[test]
fn header_menu_button_hit_targets_rendered_buttons() {
    let state = script_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let content = detail::center_content(area, state.split_ratio(), state.detail_ratio());
    let buttons_y = content.y.saturating_add(4);
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            buttons_y
        ),
        Some(detail::DetailHit::Button(0))
    );
    let labels = ["word", "bash", "inline"];
    let cells = detail::button_layout(
        content.width,
        &labels
            .iter()
            .map(|label| label.to_string())
            .collect::<Vec<_>>(),
    );
    assert_eq!(cells.len(), 3);
    let second = content.x.saturating_add(cells[1].x);
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            second,
            buttons_y
        ),
        Some(detail::DetailHit::Button(1))
    );
}

#[test]
fn auto_case_hit_targets_value_row() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let content = props::props_content(area, state.split_ratio(), state.detail_ratio());
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            content.y.saturating_add(2)
        ),
        Some(props::PropsHit::AutoCase)
    );
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            content.y.saturating_add(3)
        ),
        None
    );
}

#[test]
fn auto_case_toggle_flips_value() {
    let state = sample_state();
    let interaction = state.toggle_selected_auto_case();
    let pending = interaction.pending_edit().expect("pending autocase");
    assert_eq!(pending.trigger_id, "id-alt+r");
    assert_eq!(pending.field, EditedField::AutoCase(true));
}

#[test]
fn alias_text_lines_show_one_per_line() {
    let rows = LibraryTrigger::expand(multi_alias_list_item());
    let item = &rows[0];
    assert_eq!(
        props::alias_text_lines(item, 30, 40),
        vec!["gst".to_string(), "ctrl+g".to_string()]
    );
    assert_eq!(
        props::alias_text_lines(item, 2, 40),
        vec!["g…".to_string(), "ctrl+g".to_string()]
    );
}

#[test]
fn alias_text_lines_mark_siblings_past_three() {
    let mut list_item = multi_alias_list_item();
    list_item
        .invocations
        .push(alias_fixture("id-multi", "ga", InvocationType::Word, false));
    list_item
        .invocations
        .push(alias_fixture("id-multi", "gb", InvocationType::Word, false));
    let rows = LibraryTrigger::expand(list_item);
    let lines = props::alias_text_lines(&rows[0], 40, 40);
    assert_eq!(
        lines,
        vec!["gst".to_string(), "ctrl+g".to_string(), "ga …".to_string()]
    );
}

#[test]
fn alias_text_lines_empty_stays_border_token() {
    let item = LibraryTrigger::single(list_item(
        "id-solo",
        None,
        TriggerType::Word,
        "solo",
        "output",
        "text",
        "all",
        0,
        None,
    ));
    assert_eq!(
        props::alias_text_lines(&item, 30, 40),
        vec![detail::EMPTY_TOKEN.to_string()]
    );
}

#[test]
fn wrapped_alias_block_shifts_usage_toggle_down() {
    let item = TriggerListItem {
        id: "id-long".to_string(),
        name: String::new(),
        description: None,
        invocations: vec![
            alias_fixture(
                "id-long",
                "first-long-alias-name",
                InvocationType::Word,
                false,
            ),
            alias_fixture("id-long", "second-long-alias", InvocationType::Word, false),
            alias_fixture("id-long", "third", InvocationType::Word, false),
        ],
        display: "first-long-alias-name".to_string(),
        output: "output".to_string(),
        action_type: "text".to_string(),
        target_os: "all".to_string(),
        only_apps: None,
        except_apps: None,
        auto_case: false,
        is_enabled: true,
        usage_count: 0,
        last_used_at: None,
        created_at: 0,
        tags: "[]".to_string(),
        script_content: None,
        interpreter: None,
        behavior: None,
    };
    let mut state = LibraryPageState::default();
    state.replace_items(LibraryTrigger::expand(item));
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let content = props::props_content(area, state.split_ratio(), state.detail_ratio());
    let selected = state.selected_index().expect("selection");
    let item = state.item_at_filtered(selected).expect("item").clone();
    let base = props::usage_toggle_offset(&item);
    let extra = props::alias_extra_lines(&item, content.width);
    assert!(extra > 0);
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            content.y.saturating_add(base).saturating_add(extra)
        ),
        Some(props::PropsHit::UsageToggle)
    );
}

#[test]
fn platform_hit_targets_value_row() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let content = props::props_content(area, state.split_ratio(), state.detail_ratio());
    let selected = state.selected_index().expect("selection");
    let item = state.item_at_filtered(selected).expect("item");
    // honey: recompute the expected row exactly like the renderer.
    let rows = props::info_rows(item);
    let position = rows
        .iter()
        .position(|(label, _)| *label == "Platform")
        .expect("platform row") as u16;
    let offset = position.saturating_mul(2).saturating_add(2);
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            content.y.saturating_add(offset)
        ),
        Some(props::PropsHit::Platform)
    );
}

#[test]
fn platform_menu_parks_on_current_os_with_icons() {
    let mut state = sample_state();
    let menu = open_menu(&mut state, HeaderMenuKind::Platform);
    assert_eq!(menu.kind(), HeaderMenuKind::Platform);
    assert_eq!(
        menu.options(),
        &["All", "Windows", "macOS", "Linux", "Android", "iOS"]
    );
    // First sorted row targets windows.
    assert_eq!(menu.selected_option(), Some("Windows"));
    let icons = menu.icons();
    assert_eq!(icons.len(), 6);
    assert!(icons.iter().all(|icon| !icon.is_empty()));
}

#[test]
fn platform_menu_confirm_persists_db_value() {
    let mut state = sample_state();
    open_menu(&mut state, HeaderMenuKind::Platform);
    state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let pending = interaction.pending_edit().expect("pending platform");
    assert_eq!(pending.trigger_id, "id-alt+r");
    assert_eq!(pending.field, EditedField::TargetOs("all".to_string()));
    assert!(state.modal().is_none());
}

fn tags_menu(trigger_tags: &[&str]) -> LibraryTagsModalState {
    LibraryTagsModalState::new(
        "id-tags".to_string(),
        0,
        trigger_tags.iter().map(|tag| tag.to_string()).collect(),
    )
}

#[test]
fn tags_menu_confirm_saves_typed_tag() {
    let mut menu = tags_menu(&["work"]);
    menu.start_input();
    for ch in "dev".chars() {
        menu.push_input(ch);
    }
    let InputConfirm::Save(pending) = menu.confirm_input() else {
        panic!("typed tag saves");
    };
    assert_eq!(
        pending.field,
        EditedField::Tags(vec!["work".to_string(), "dev".to_string()])
    );
}

#[test]
fn tags_menu_confirm_validates_input() {
    let mut empty = tags_menu(&[]);
    empty.start_input();
    assert_eq!(empty.confirm_input(), InputConfirm::Cancel);
    assert!(!empty.input_active());

    let mut dup = tags_menu(&["work"]);
    dup.start_input();
    for ch in "WORK".chars() {
        dup.push_input(ch);
    }
    assert!(matches!(dup.confirm_input(), InputConfirm::Invalid(_)));
    assert!(dup.input_active());
    assert!(dup.error().is_some());

    let mut long = tags_menu(&[]);
    long.start_input();
    for _ in 0..51 {
        long.push_input('a');
    }
    assert!(matches!(long.confirm_input(), InputConfirm::Invalid(_)));
    assert!(long.input_active());
}

#[test]
fn tags_menu_char_opens_input_and_types() {
    let mut state = sample_state();
    state.modal = Some(LibraryModal::Tags(tags_menu(&["work"])));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert!(menu.input_active());
    assert_eq!(menu.input().text(), "d");
}

#[test]
fn tags_menu_empty_state_reports_none() {
    assert!(tags_menu(&[]).is_empty());
    assert!(!tags_menu(&["work"]).is_empty());
    assert_eq!(TAGS_EMPTY_LINE, "No tags available");
}

#[test]
fn tags_menu_chip_colors_match_props_row() {
    use crate::theme::builtin::DARK_THEME;
    use crate::widgets::library::props::tag_color;

    // honey: overlay chips reuse the Properties pane color by tag
    // position, text only with no background.
    for index in 0..7 {
        let style = tag_style(&DARK_THEME, index);
        assert_eq!(style.fg, Some(tag_color(&DARK_THEME, index)));
        assert_eq!(style.bg, None);
    }
    assert_eq!(
        tag_style(&DARK_THEME, 0),
        tag_style(&DARK_THEME, 5),
        "palette cycles every five tags like the props row"
    );
}

#[test]
fn tags_menu_chip_layout_wraps() {
    let menu = tags_menu(&["work", "home"]);
    let cells = menu.chip_cells(50);
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].x, 0);
    assert_eq!(cells[0].width, 5);
    assert_eq!(menu.plus_row_y(50), 0);
    let narrow = menu.chip_cells(8);
    assert_eq!(narrow[0].y, 0);
    assert_eq!(narrow[1].y, 1);
    assert_eq!(menu.plus_row_y(8), 1);
}

#[test]
fn tags_menu_chip_hit_matches_cell_not_row() {
    let menu = tags_menu(&["work", "home"]);
    // Width 50: `work` at x0 w5, `home` at x7 w5.
    let hit = menu.chip_at(50, 8, 0, 8).expect("home cell");
    assert_eq!(hit.index, 1);
    assert!(menu.chip_at(50, 8, 0, 5).is_none());
    assert!(menu.chip_at(50, 8, 1, 0).is_none());
    // The `+` row reads no chip.
    let (plus_x, plus_y) = menu.plus_cell(50);
    assert!(menu.chip_at(50, 8, plus_y, plus_x).is_none());
}

#[test]
fn tags_menu_hover_clamps_to_chips() {
    let mut menu = tags_menu(&["work"]);
    menu.set_hover(Some(0));
    assert_eq!(menu.hover(), Some(0));
    menu.set_hover(Some(9));
    assert_eq!(menu.hover(), None);
}

#[test]
fn tags_menu_focus_walks_chips_and_add_row() {
    let mut menu = tags_menu(&["work", "home"]);
    menu.move_focus(50, 1, 0);
    assert_eq!(menu.focus(), 1);
    menu.move_focus(50, 1, 0);
    assert_eq!(menu.focus(), 2);
    assert!(menu.focus_is_add_row());
    menu.move_focus(50, 1, 0);
    assert_eq!(menu.focus(), 0);
    menu.move_focus(50, 0, -1);
    assert!(menu.focus_is_add_row());
    menu.move_focus(50, 0, 1);
    assert_eq!(menu.focus(), 0);
}

#[test]
fn tags_menu_esc_closes_enter_opens_input() {
    let mut state = sample_state();
    state.modal = Some(LibraryModal::Tags(tags_menu(&["work"])));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_none());

    // Enter on a chip is a no-op; the menu lives on.
    state.modal = Some(LibraryModal::Tags(tags_menu(&["work"])));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_some());

    // Enter on the `+` row opens the inline input; Esc closes it.
    let mut menu = tags_menu(&["work"]);
    menu.set_focus(1);
    state.modal = Some(LibraryModal::Tags(menu));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert!(menu.input_active());
    state.modal = Some(LibraryModal::Tags(menu.clone()));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert!(!menu.input_active());
}

#[test]
fn tags_menu_enter_saves_typed_input() {
    let mut state = sample_state();
    let mut menu = tags_menu(&["work"]);
    menu.set_focus(1);
    menu.start_input();
    for ch in "dev".chars() {
        menu.push_input(ch);
    }
    state.modal = Some(LibraryModal::Tags(menu));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let pending = interaction.pending_edit().expect("pending save");
    assert_eq!(
        pending.field,
        EditedField::Tags(vec!["work".to_string(), "dev".to_string()])
    );
    assert!(state.modal().is_some());
}

#[test]
fn tags_menu_delete_removes_focused_chip() {
    let mut state = sample_state();
    state.modal = Some(LibraryModal::Tags(tags_menu(&["work", "home"])));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
    let pending = interaction.pending_edit().expect("pending remove");
    assert_eq!(pending.field, EditedField::Tags(vec!["home".to_string()]));
    assert!(state.modal().is_some());
}

#[test]
fn tags_row_hit_opens_position() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let content = props::props_content(area, state.split_ratio(), state.detail_ratio());
    let selected = state.selected_index().expect("selection");
    let item = state.item_at_filtered(selected).expect("item");
    let tags_at = content
        .y
        .saturating_add(props::usage_toggle_offset(item))
        .saturating_sub(2);
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            tags_at
        ),
        Some(props::PropsHit::Tags)
    );
}

#[test]
fn tags_menu_remove_drops_checked_row() {
    let menu = tags_menu(&["work", "home"]);
    let pending = menu.remove_at(1).expect("pending remove");
    assert_eq!(pending.field, EditedField::Tags(vec!["work".to_string()]));
    assert!(tags_menu(&[]).remove_at(0).is_none());
}

#[test]
fn click_tags_menu_first_cell_removes_at_once() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Popup 56x14 centered with 3-cell body padding and one blank
    // row: first chip row 11, its first cell 25; `+` trails the
    // chip on row 11.
    // Body click parks focus without persisting; the close cell
    // removes with a single click.
    state.modal = Some(LibraryModal::Tags(tags_menu(&["work"])));
    let interaction = state.click_tags_menu(area, 26, 11);
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert_eq!(menu.focus(), 0);
    state.modal = Some(LibraryModal::Tags(menu.clone()));
    let interaction = state.click_tags_menu(area, 25, 11);
    let pending = interaction.pending_edit().expect("pending remove");
    assert_eq!(pending.field, EditedField::Tags(Vec::new()));
    assert!(state.modal().is_some());

    // Clicking the trailing `+` opens the inline input.
    let mut menu = tags_menu(&["work"]);
    menu.set_cloud_width(50);
    let (plus_x, plus_y) = menu.plus_cell(50);
    state.modal = Some(LibraryModal::Tags(menu));
    let popup = crate::widgets::util::overlay_popup(area);
    let body = crate::widgets::util::overlay_body(popup);
    let interaction = state.click_tags_menu(
        area,
        body.x.saturating_add(plus_x),
        body.y.saturating_add(2).saturating_add(plus_y),
    );
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert!(menu.input_active());
}

#[test]
fn click_tags_menu_empty_plus_opens_input() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Empty cloud: `No tags available  + ` with the button padded
    // one cell each side; chip-row clicks do nothing.
    state.modal = Some(LibraryModal::Tags(tags_menu(&[])));
    let interaction = state.click_tags_menu(area, 25, 11);
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert!(!menu.input_active());
    let popup = crate::widgets::util::overlay_popup(area);
    let body = crate::widgets::util::overlay_body(popup);
    let plus_x = menu.plus_hit_x(body.width);
    state.modal = Some(LibraryModal::Tags(menu.clone()));
    let interaction = state.click_tags_menu(
        area,
        body.x.saturating_add(plus_x),
        body.y.saturating_add(2),
    );
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert!(menu.input_active());
}

#[test]
fn hover_tags_menu_tracks_chip_then_clears() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    state.modal = Some(LibraryModal::Tags(tags_menu(&["work"])));
    state.hover_tags_menu(area, 26, 11);
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert_eq!(menu.hover(), Some(0));
    state.modal = Some(LibraryModal::Tags(menu.clone()));
    state.hover_tags_menu(area, 10, 10);
    let Some(LibraryModal::Tags(menu)) = state.modal() else {
        panic!("tags menu open");
    };
    assert_eq!(menu.hover(), None);
}

fn fg_app(
    exe: &str,
    title: &str,
    class: &str,
) -> taurine_core::system::foreground_apps::ForegroundApp {
    taurine_core::system::foreground_apps::ForegroundApp {
        exe: exe.to_string(),
        path: format!("C:\\bin\\{exe}"),
        class: class.to_string(),
        title: title.to_string(),
    }
}

fn filter_menu(side: AppFilterSide) -> LibraryAppFilterState {
    LibraryAppFilterState::new(
        "id-apps".to_string(),
        0,
        side,
        vec!["exe:code".to_string()],
        vec!["exe:notepad".to_string()],
        vec![
            fg_app("notepad.exe", "notes", "Notepad"),
            fg_app("calc.exe", "Calculator", "CalcClass"),
        ],
    )
}

#[test]
fn gray_key_collides_across_forms() {
    assert_eq!(gray_key("exe:code"), gray_key("CODE"));
    assert_eq!(gray_key("exe:code"), gray_key("code.exe"));
    assert_eq!(gray_key("exe:code"), gray_key("Exe:CODE.EXE"));
    assert_ne!(gray_key("exe:code"), gray_key("exe:notepad"));
    assert_eq!(gray_key("class:Chrome_WidgetWin_1"), "chrome_widgetwin_1");
}

#[test]
fn filter_menu_grays_opposite_list() {
    let menu = filter_menu(AppFilterSide::Allow);
    assert!(menu.is_gray(0));
    assert!(!menu.is_gray(1));
    assert_eq!(menu.candidate(0).as_deref(), Some("exe:notepad.exe"));
    // Block side mirrors: code is the gray one there.
    let blocked = LibraryAppFilterState::new(
        "id-apps".to_string(),
        0,
        AppFilterSide::Block,
        vec!["exe:notepad".to_string()],
        vec!["exe:code".to_string()],
        vec![fg_app("code.exe", "VS Code", "Chrome_WidgetWin_1")],
    );
    assert!(blocked.is_gray(0));
}

#[test]
fn filter_menu_cursor_skips_gray() {
    let mut menu = filter_menu(AppFilterSide::Allow);
    // Rows: checked(0), gray notepad(1), calc(2).
    menu.move_cursor(1);
    assert_eq!(menu.cursor(), 2);
    menu.move_cursor(1);
    assert_eq!(menu.cursor(), 0);
    menu.move_cursor(-1);
    assert_eq!(menu.cursor(), 2);
}

#[test]
fn filter_menu_toggle_adds_and_removes() {
    let menu = filter_menu(AppFilterSide::Allow);
    // Enter on the checked row removes it.
    let pending = menu.toggle_focused().expect("pending remove");
    assert_eq!(pending.field, EditedField::OnlyApps(Vec::new()));
    // Foreground calc adds an `exe:` candidate.
    let mut menu = filter_menu(AppFilterSide::Allow);
    menu.set_cursor(FilterRow::Foreground(1));
    let pending = menu.toggle_focused().expect("pending add");
    assert_eq!(
        pending.field,
        EditedField::OnlyApps(vec!["exe:code".to_string(), "exe:calc.exe".to_string()])
    );
    // Gray rows never toggle.
    let menu = filter_menu(AppFilterSide::Allow);
    assert!(menu.toggle_focused().is_some());
    // Block side persists the other field.
    let blocked = LibraryAppFilterState::new(
        "id-apps".to_string(),
        0,
        AppFilterSide::Block,
        Vec::new(),
        Vec::new(),
        vec![fg_app("calc.exe", "Calculator", "CalcClass")],
    );
    let pending = blocked.toggle_focused().expect("pending add");
    assert_eq!(
        pending.field,
        EditedField::ExceptApps(vec!["exe:calc.exe".to_string()])
    );
}

#[test]
fn filter_menu_keys_toggle_and_type() {
    let mut state = sample_state();
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    // Down skips the gray row onto calc; Enter adds it live.
    state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let pending = interaction.pending_edit().expect("pending add");
    assert_eq!(
        pending.field,
        EditedField::OnlyApps(vec!["exe:code".to_string(), "exe:calc.exe".to_string()])
    );
    assert!(state.modal().is_some());

    // Typing filters the list.
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::AppFilter(menu)) = state.modal() else {
        panic!("filter menu open");
    };
    assert_eq!(menu.search().text(), "c");
    assert_eq!(menu.matching_indices(), vec![1]);
}

#[test]
fn filter_menu_search_backspace_restores() {
    let mut menu = filter_menu(AppFilterSide::Allow);
    for ch in "calc".chars() {
        menu.search_mut().insert(ch);
    }
    menu.refilter();
    assert_eq!(menu.matching_indices(), vec![1]);
    for _ in 0..4 {
        menu.search_mut().backspace();
    }
    menu.refilter();
    assert_eq!(menu.matching_indices(), vec![0, 1]);
}

#[test]
fn filter_menu_caps_foreground_at_eight() {
    let apps: Vec<_> = (0..11)
        .map(|n| fg_app(&format!("app{n}.exe"), &format!("App {n}"), "SomeClass"))
        .collect();
    let menu = LibraryAppFilterState::new(
        "id-apps".to_string(),
        0,
        AppFilterSide::Allow,
        Vec::new(),
        Vec::new(),
        apps,
    );
    assert_eq!(menu.foreground().len(), 8);
    assert_eq!(menu.matching_indices().len(), 8);
}

#[test]
fn click_app_filter_search_places_caret() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Body at x21 y7, 58x16: rows from row 9, search box pinned
    // to row 22. With `calc` typed the list shrinks: one checked
    // row, one two-line match, the Add row.
    let mut menu = filter_menu(AppFilterSide::Allow);
    for ch in "calc".chars() {
        menu.search_mut().insert(ch);
    }
    state.modal = Some(LibraryModal::AppFilter(menu));
    let interaction = state.click_app_filter_menu(area, 20, 22);
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::AppFilter(menu)) = state.modal() else {
        panic!("filter menu open");
    };
    assert_eq!(menu.search().cursor(), 2);
    assert_eq!(menu.matching_indices(), vec![1]);
}

#[test]
fn hover_app_filter_menu_follows_rows_skips_gray() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Rows from row 9: checked(0), gray notepad(10-11), calc(12-13).
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    state.hover_app_filter_menu(area, 25, 12);
    let Some(LibraryModal::AppFilter(menu)) = state.modal() else {
        panic!("filter menu open");
    };
    assert_eq!(menu.cursor(), 2);
    // Gray rows refuse the hover.
    state.modal = Some(LibraryModal::AppFilter(menu.clone()));
    state.hover_app_filter_menu(area, 25, 10);
    let Some(LibraryModal::AppFilter(menu)) = state.modal() else {
        panic!("filter menu open");
    };
    assert_eq!(menu.cursor(), 2);

    // Wheel walks the cursor like the arrows do.
    state.modal = Some(LibraryModal::AppFilter(menu.clone()));
    state.move_app_filter_cursor(false);
    let Some(LibraryModal::AppFilter(menu)) = state.modal() else {
        panic!("filter menu open");
    };
    assert_eq!(menu.cursor(), 0);
}

#[test]
fn click_app_filter_blank_line_is_dead() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Row 8 is the blank line under the title: no write, no close.
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    let interaction = state.click_app_filter_menu(area, 25, 8);
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_some());
}

#[test]
fn click_app_filter_right_pane_is_dead() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Right pane starts past the divider; clicks there do nothing.
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    let interaction = state.click_app_filter_menu(area, 60, 10);
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_some());
}

#[test]
fn click_app_filter_menu_toggles_and_grays() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Popup 64x18 centered, body at x21 y9: rows start at row 9.
    // Checked(0) row 9, gray notepad rows 10-11, calc rows 12-13.
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    let interaction = state.click_app_filter_menu(area, 26, 9);
    let pending = interaction.pending_edit().expect("pending remove");
    assert_eq!(pending.field, EditedField::OnlyApps(Vec::new()));
    assert!(state.modal().is_some());

    // Gray rows refuse with an error and no write.
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    let interaction = state.click_app_filter_menu(area, 25, 10);
    assert!(interaction.pending_edit().is_none());
    let Some(LibraryModal::AppFilter(menu)) = state.modal() else {
        panic!("filter menu open");
    };
    assert!(menu.error().is_some());

    // Foreground calc adds.
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    let interaction = state.click_app_filter_menu(area, 25, 12);
    let pending = interaction.pending_edit().expect("pending add");
    assert_eq!(
        pending.field,
        EditedField::OnlyApps(vec!["exe:code".to_string(), "exe:calc.exe".to_string()])
    );
}

#[test]
fn click_app_filter_menu_outside_closes() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    state.modal = Some(LibraryModal::AppFilter(filter_menu(AppFilterSide::Allow)));
    let interaction = state.click_app_filter_menu(area, 0, 0);
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn allow_block_rows_hit_open_positions() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let content = props::props_content(area, state.split_ratio(), state.detail_ratio());
    // Allow on sits at label position 1, Block on at 2: offset *2+2.
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            content.y + 4
        ),
        Some(props::PropsHit::Allow)
    );
    assert_eq!(
        props::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            content.x,
            content.y + 6
        ),
        Some(props::PropsHit::Block)
    );
}

#[test]
fn join_app_filters_escapes_commas() {
    assert_eq!(
        join_app_filters(&["exe:code".to_string(), "title:Hello, World".to_string()]),
        "exe:code,title:Hello\\, World"
    );
    assert_eq!(join_app_filters(&[]), String::new());
}

#[test]
fn click_tags_menu_outside_closes() {
    let mut state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    state.modal = Some(LibraryModal::Tags(tags_menu(&["work"])));
    let interaction = state.click_tags_menu(area, 0, 0);
    assert!(interaction.pending_edit().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn right_pane_clicks_hit_nothing() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);

    assert_eq!(list::hit_test(area, &state, 60, 5), None);
    assert_eq!(list::hit_test(area, &state, 60, 28), None);
}

#[test]
fn default_split_matches_legacy_halves() {
    let state = LibraryPageState::default();
    assert_eq!(state.split_ratio(), 2.0 / 7.0);
    assert_eq!(state.detail_ratio(), 0.4);

    // 2:3:2 proportions: sides near-equal, content biggest.
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    let split = split_panes(area, state.split_ratio(), state.detail_ratio());
    assert!((27..=29).contains(&split.list.width));
    assert!((27..=29).contains(&split.props.width));
    assert!(split.center.width > split.list.width);
    assert!(split.center.width > split.props.width);
    let columns = divider_columns(area, state.split_ratio(), state.detail_ratio());
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0], split.list.width);
    assert_eq!(
        columns[1],
        split.center.x.saturating_add(split.center.width)
    );
}

#[test]
fn custom_split_moves_divider_and_clamps() {
    let mut state = LibraryPageState::default();
    state.set_split_ratio(0.0);
    assert_eq!(state.split_ratio(), MIN_SPLIT_RATIO);
    state.set_split_ratio(2.0);
    assert_eq!(state.split_ratio(), MAX_SPLIT_RATIO);
    state.set_detail_ratio(0.0);
    assert_eq!(state.detail_ratio(), MIN_DETAIL_RATIO);
    state.set_detail_ratio(2.0);
    assert_eq!(state.detail_ratio(), MAX_DETAIL_RATIO);
}

#[test]
fn narrow_page_collapses_to_list_only() {
    let state = LibraryPageState::default();

    // Degenerate width: list only, no dividers.
    let area = ratatui::layout::Rect::new(0, 0, 4, 30);
    let split = split_panes(area, state.split_ratio(), state.detail_ratio());
    assert_eq!(split.list.width, 4);
    assert_eq!(split.center.width, 0);
    assert_eq!(split.props.width, 0);
    assert!(divider_columns(area, state.split_ratio(), state.detail_ratio()).is_empty());
    assert_eq!(
        divider_hit(area, state.split_ratio(), state.detail_ratio(), 2, 5),
        None
    );

    // No minimums: narrow pages keep all three panes, squeezed.
    let area = ratatui::layout::Rect::new(0, 0, 42, 30);
    let split = split_panes(area, state.split_ratio(), state.detail_ratio());
    assert_eq!(split.list.width, 11);
    assert_eq!(split.center.width, 18);
    assert_eq!(split.props.width, 11);
    assert_eq!(
        divider_columns(area, state.split_ratio(), state.detail_ratio()),
        vec![11, 30]
    );
}

#[test]
fn divider_double_click_resets_pane_width() {
    let mut state = LibraryPageState::default();
    state.set_split_ratio(0.6);
    state.set_detail_ratio(0.6);

    // Slow second click: no reset.
    assert!(!state.divider_double_click_at(DividerSide::List, 1000));
    assert!(!state.divider_double_click_at(DividerSide::List, 1000 + DIVIDER_DOUBLE_CLICK_MS + 1));
    assert_eq!(state.split_ratio(), 0.6);

    // Quick double-click on the list divider resets only it.
    assert!(!state.divider_double_click_at(DividerSide::List, 2000));
    assert!(state.divider_double_click_at(DividerSide::List, 2100));
    assert_eq!(state.split_ratio(), DEFAULT_SPLIT_RATIO);
    assert_eq!(state.detail_ratio(), 0.6);

    // Other side is independent.
    assert!(!state.divider_double_click_at(DividerSide::Props, 3000));
    assert!(state.divider_double_click_at(DividerSide::Props, 3050));
    assert_eq!(state.detail_ratio(), DEFAULT_DETAIL_RATIO);
}

#[test]
fn divider_double_click_toggles_default_and_collapsed() {
    let mut state = LibraryPageState::default();
    assert_eq!(state.split_ratio(), DEFAULT_SPLIT_RATIO);

    // Default width collapses to the edge.
    assert!(!state.divider_double_click_at(DividerSide::List, 1000));
    assert!(state.divider_double_click_at(DividerSide::List, 1100));
    assert_eq!(state.split_ratio(), MIN_SPLIT_RATIO);

    // Collapsed restores the default width.
    assert!(!state.divider_double_click_at(DividerSide::List, 2000));
    assert!(state.divider_double_click_at(DividerSide::List, 2100));
    assert_eq!(state.split_ratio(), DEFAULT_SPLIT_RATIO);

    // Same toggle on the props divider.
    assert!(!state.divider_double_click_at(DividerSide::Props, 3000));
    assert!(state.divider_double_click_at(DividerSide::Props, 3050));
    assert_eq!(state.detail_ratio(), MIN_DETAIL_RATIO);
    assert!(!state.divider_double_click_at(DividerSide::Props, 4000));
    assert!(state.divider_double_click_at(DividerSide::Props, 4050));
    assert_eq!(state.detail_ratio(), DEFAULT_DETAIL_RATIO);
}

#[test]
fn drag_range_parks_panes_at_either_edge() {
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    // honey: float truncation keeps this within one cell of the edge.
    let split = split_panes(area, split_ratio_for_column(area, 0), DEFAULT_DETAIL_RATIO);
    assert!(split.list.width <= 1);
    // Divider stays on the edge, still grabbable to drag back.
    assert_eq!(
        divider_columns(area, split_ratio_for_column(area, 0), DEFAULT_DETAIL_RATIO),
        vec![0, 48]
    );
    assert_eq!(
        divider_hit(
            area,
            split_ratio_for_column(area, 0),
            DEFAULT_DETAIL_RATIO,
            0,
            5
        ),
        Some(DividerSide::List)
    );
    let split = split_panes(area, split_ratio_for_column(area, 79), DEFAULT_DETAIL_RATIO);
    // Exact edge ratio parks fully open with the lone pane absorbing
    // its dead gutter: divider on the last column, nothing after it.
    assert_eq!(split.list.width, 79);
    assert_eq!(split.center.width, 0);
    assert_eq!(
        divider_columns(area, split_ratio_for_column(area, 79), DEFAULT_DETAIL_RATIO),
        vec![79, 80]
    );
}

#[test]
fn split_overflow_eats_props_then_center() {
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Span 98: list wants 78, capped at 52; 26 overflow takes the
    // 18-wide props first, then 8 of center.
    let split = split_panes(area, 0.8, DEFAULT_DETAIL_RATIO);
    assert_eq!(split.list.width, 78);
    assert_eq!(split.props.width, 0);
    assert_eq!(split.center.width, 20);

    // Deeper overflow leaves a 5-cell center sliver.
    let split = split_panes(area, 0.95, DEFAULT_DETAIL_RATIO);
    assert_eq!(split.list.width, 93);
    assert_eq!(split.props.width, 0);
    assert_eq!(split.center.width, 5);
}

#[test]
fn split_overflow_on_props_side_eats_list_then_center() {
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // List at default 28; props wants 63 of the 70 rest, capped at 40;
    // 23 overflow takes the far list pane, center untouched.
    let split = split_panes(area, DEFAULT_SPLIT_RATIO, 0.9);
    assert_eq!(split.list.width, 5);
    assert_eq!(split.center.width, 30);
    assert_eq!(split.props.width, 63);
}

#[test]
fn split_extreme_ratios_park_without_resurrection() {
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // Props side: 0.99 and 1.0 differ by single cells; the list stays
    // gone instead of snapping back.
    let before = split_panes(area, DEFAULT_SPLIT_RATIO, 0.99);
    let parked = split_panes(area, DEFAULT_SPLIT_RATIO, 1.0);
    assert_eq!(before.list.width, 0);
    assert_eq!(parked.list.width, 0);
    assert!((before.center.width as i16 - parked.center.width as i16).abs() <= 2);
    assert!((parked.props.width as i16 - before.props.width as i16).abs() <= 2);

    // Deep overflow parks props nearly full screen.
    let parked = split_panes(area, DEFAULT_SPLIT_RATIO, 2.0);
    assert_eq!(parked.list.width, 0);
    assert_eq!(parked.center.width, 0);
    assert_eq!(parked.props.width, 99);

    // List side parks fully open with no survivors.
    let parked = split_panes(area, 1.0, DEFAULT_DETAIL_RATIO);
    assert_eq!(parked.list.width, 99);
    assert_eq!(parked.center.width, 0);
    assert_eq!(parked.props.width, 0);
}

#[test]
fn visible_dividers_hide_parked_nothing_dividers() {
    let area = ratatui::layout::Rect::new(0, 0, 100, 30);
    // List parked full screen: only its own divider paints, on the
    // last column with nothing after it.
    assert_eq!(
        visible_divider_columns(area, 1.0, DEFAULT_DETAIL_RATIO),
        vec![(DividerSide::List, 99)]
    );
    // Props parked full screen: only its own divider paints, on the
    // first column.
    assert_eq!(
        visible_divider_columns(area, DEFAULT_SPLIT_RATIO, 2.0),
        vec![(DividerSide::Props, 0)]
    );
    // Ordinary split paints both, tagged in order.
    let split = split_panes(area, DEFAULT_SPLIT_RATIO, DEFAULT_DETAIL_RATIO);
    assert_eq!(
        visible_divider_columns(area, DEFAULT_SPLIT_RATIO, DEFAULT_DETAIL_RATIO),
        vec![
            (DividerSide::List, split.dividers[0]),
            (DividerSide::Props, split.dividers[1]),
        ]
    );
}

#[test]
fn divider_hit_only_on_gutter_column() {
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    let state = LibraryPageState::default();

    assert_eq!(
        divider_hit(area, state.split_ratio(), state.detail_ratio(), 22, 5),
        Some(DividerSide::List)
    );
    assert_eq!(
        divider_hit(area, state.split_ratio(), state.detail_ratio(), 57, 5),
        Some(DividerSide::Props)
    );
    assert_eq!(
        divider_hit(area, state.split_ratio(), state.detail_ratio(), 23, 5),
        None
    );
    assert_eq!(
        divider_hit(area, state.split_ratio(), state.detail_ratio(), 22, 30),
        None
    );

    let narrow = ratatui::layout::Rect::new(0, 0, 4, 30);
    assert_eq!(
        divider_hit(narrow, state.split_ratio(), state.detail_ratio(), 2, 5),
        None
    );
}

#[test]
fn split_ratio_for_column_round_trips_divider() {
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    let ratio = split_ratio_for_column(area, 20);
    let split = split_panes(area, ratio, DEFAULT_DETAIL_RATIO);
    assert!((19..=21).contains(&split.list.width));
    assert!(divider_columns(area, ratio, DEFAULT_DETAIL_RATIO).contains(&split.list.width));
}

#[test]
fn detail_ratio_for_column_round_trips_props_divider() {
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    let ratio = detail_ratio_for_column(area, DEFAULT_SPLIT_RATIO, 55);
    let split = split_panes(area, DEFAULT_SPLIT_RATIO, ratio);
    let divider = split.center.x.saturating_add(split.center.width);
    assert!((54..=58).contains(&divider));
}

fn six_item_state() -> LibraryPageState {
    let mut state = LibraryPageState::default();
    state.replace_items(
        ["t0", "t1", "t2", "t3", "t4", "t5"]
            .into_iter()
            .enumerate()
            .map(|(index, trigger)| {
                LibraryTrigger::single(list_item(
                    &format!("id-{trigger}"),
                    None,
                    TriggerType::Word,
                    trigger,
                    "out",
                    "text",
                    "all",
                    index as i64,
                    None,
                ))
            })
            .collect(),
    );
    state
}

#[test]
fn clicked_window_holds_while_default_policy_would_jump() {
    let mut state = six_item_state();
    for _ in 0..4 {
        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    assert_eq!(state.selected_index(), Some(4));
    assert_eq!(state.visible_window(2), (3, 5));

    state.click_item(3, 3);
    assert_eq!(state.selected_index(), Some(3));
    assert_eq!(state.visible_window(2), (3, 5));
}

#[test]
fn switching_targets_commits_and_keeps_new_session_across_refresh() {
    let mut state = sample_state();
    state.start_description_edit();
    for ch in " hi".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    // Clicking the name commits the description, typed text intact.
    let interaction = state.start_name_edit_at(0);
    let pending = interaction.pending_edit().expect("pending edit");
    assert!(matches!(
        &pending.field,
        crate::widgets::library::actions::EditedField::Description(Some(text))
            if text.ends_with(" hi")
    ));
    assert!(matches!(
        state.edit().map(|edit| edit.target()),
        Some(crate::widgets::library::state::EditTarget::Name)
    ));

    // Simulate lib apply + refresh: the new session survives.
    let fresh: Vec<LibraryTrigger> = (0..state.filtered_len())
        .filter_map(|index| state.item_at_filtered(index).cloned())
        .collect();
    state.replace_items(fresh);
    assert!(matches!(
        state.edit().map(|edit| edit.target()),
        Some(crate::widgets::library::state::EditTarget::Name)
    ));
}

#[test]
fn refresh_drops_session_when_trigger_is_gone() {
    let mut state = sample_state();
    state.start_description_edit();
    let id = state
        .item_at_filtered(state.selected_index().unwrap())
        .expect("row")
        .id()
        .to_string();
    let fresh: Vec<LibraryTrigger> = (0..state.filtered_len())
        .filter_map(|index| state.item_at_filtered(index).cloned())
        .filter(|item| item.id() != id)
        .collect();
    state.replace_items(fresh);
    assert!(state.edit().is_none());
}

#[test]
fn refresh_after_save_keeps_window_still() {
    let mut state = six_item_state();
    state.click_item(3, 3);
    assert_eq!(state.visible_window(2), (3, 5));

    // Simulate the post-save refresh: identical rows, rebuilt.
    let fresh: Vec<LibraryTrigger> = (0..state.filtered_len())
        .filter_map(|index| state.item_at_filtered(index).cloned())
        .collect();
    state.replace_items(fresh);
    assert_eq!(state.selected_index(), Some(3));
    assert_eq!(state.visible_window(2), (3, 5));
}

#[test]
fn select_row_keeps_click_anchor() {
    let mut state = six_item_state();
    state.click_item(3, 3);
    let target = state.item_at_filtered(4).expect("fifth row").clone();
    assert!(state.select_row(target.id(), target.trigger()));
    assert_eq!(state.selected_index(), Some(4));
    assert_eq!(state.visible_window(2), (3, 5));
}

#[test]
fn stale_anchor_falls_back_to_default_window() {
    let mut state = six_item_state();
    state.click_item(0, 3);
    assert_eq!(state.selected_index(), Some(0));
    assert_eq!(state.visible_window(2), (0, 2));
}

#[test]
fn name_edit_typing_enter_esc_flow() {
    let mut state = sample_state();
    assert!(state.name_edit().is_none());

    state.start_name_edit();
    // Draft starts with the displayed text; clicking never erases.
    assert_eq!(state.name_edit().expect("editing").field().text(), "alt+r");

    state.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
    assert!(
        state
            .name_edit()
            .expect("editing")
            .field()
            .text()
            .ends_with('!')
    );

    state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(state.name_edit().expect("editing").field().text(), "alt+r");

    // Esc cancels without persisting.
    state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(state.name_edit().is_none());
}

#[test]
fn name_edit_commit_persists_changed_name() {
    let mut state = sample_state();
    state.start_name_edit();
    for ch in " Jr".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let interaction = state.commit_edit();
    let pending = interaction.pending_edit().expect("pending edit");
    assert!(matches!(
        &pending.field,
        crate::widgets::library::actions::EditedField::Name(name) if name == "alt+r Jr"
    ));
    assert!(state.name_edit().is_none());
}

#[test]
fn name_edit_enter_is_dormant() {
    let mut state = sample_state();
    state.start_name_edit();
    for ch in " Jr".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    // honey: Enter stays in the field; autosave persists.
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    assert!(state.name_edit().is_some());
}

#[test]
fn name_edit_commit_without_changes_is_noop() {
    let mut item = list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    );
    item.name = "Morning Greeting".to_string();
    let mut state = LibraryPageState::default();
    state.replace_items(vec![LibraryTrigger::single(item)]);
    state.start_name_edit();
    let interaction = state.commit_edit();
    assert!(interaction.pending_edit().is_none());
    assert!(state.name_edit().is_none());
}

#[test]
fn name_edit_blank_is_silent_noop() {
    let mut state = sample_state();
    state.start_name_edit();
    // Clear the whole draft.
    for _ in 0..64 {
        state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    assert_eq!(state.name_edit().expect("editing").field().text(), "");
    // No warning, no error, no persist: session stays open, value untouched.
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    assert!(state.name_edit().is_some());
    assert!(state.status_message().is_none());
    let interaction = state.commit_edit();
    assert!(interaction.pending_edit().is_none());
    assert!(state.name_edit().is_none());
}

#[test]
fn autosave_persists_idle_edit_and_keeps_session() {
    let mut state = sample_state();
    state.start_name_edit();
    for ch in " Jr".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let tick = crate::widgets::library::state::now_millis()
        + crate::widgets::library::state::AUTOSAVE_DELAY_MS
        + 10;
    let interaction = state.autosave_tick_at(tick);
    let pending = interaction.pending_edit().expect("pending edit");
    assert!(matches!(
        &pending.field,
        crate::widgets::library::actions::EditedField::Name(name) if name == "alt+r Jr"
    ));
    // Session stays open for continued typing.
    assert!(state.edit().is_some());
}

#[test]
fn autosave_waits_for_quiet_period() {
    let mut state = sample_state();
    state.start_name_edit();
    state.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
    let now = crate::widgets::library::state::now_millis();
    let interaction = state.autosave_tick_at(now);
    assert!(interaction.pending_edit().is_none());
    assert!(state.edit().is_some());
}

#[test]
fn autosave_silent_on_invalid_text() {
    let mut state = sample_state();
    state.start_name_edit();
    for _ in 0..64 {
        state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    let tick = crate::widgets::library::state::now_millis()
        + crate::widgets::library::state::AUTOSAVE_DELAY_MS
        + 10;
    let interaction = state.autosave_tick_at(tick);
    assert!(interaction.pending_edit().is_none());
    assert!(state.edit().is_some());
    assert!(state.status_message().is_none());
}

#[test]
fn autosave_skips_unchanged_text() {
    let mut item = list_item(
        "id-gm",
        None,
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    );
    item.name = "Morning Greeting".to_string();
    let mut state = LibraryPageState::default();
    state.replace_items(vec![LibraryTrigger::single(item)]);
    state.start_name_edit();
    // Type and undo: stamped, but identical to storage.
    state.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    let tick = crate::widgets::library::state::now_millis()
        + crate::widgets::library::state::AUTOSAVE_DELAY_MS
        + 10;
    let interaction = state.autosave_tick_at(tick);
    assert!(interaction.pending_edit().is_none());
}

#[test]
fn name_edit_blank_then_navigate_moves_on() {
    let mut state = sample_state();
    let first = state.selected_index().unwrap();
    state.start_name_edit();
    for _ in 0..64 {
        state.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_none());
    assert_eq!(state.selected_index(), Some(first + 1));
    assert!(state.name_edit().is_none());
}

#[test]
fn name_edit_navigation_commits_and_moves() {
    let mut state = sample_state();
    let first = state.selected_index().unwrap();
    state.start_name_edit();
    for ch in " Jr".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let interaction = state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(interaction.pending_edit().is_some());
    assert_eq!(state.selected_index(), Some(first + 1));
    assert!(state.name_edit().is_none());
}

#[test]
fn name_edit_click_other_row_commits() {
    let mut state = sample_state();
    state.start_name_edit();
    for ch in " Jr".chars() {
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    let interaction = state.click_item(1, 0);
    assert!(interaction.pending_edit().is_some());
    assert_eq!(state.selected_index(), Some(1));
}

#[test]
fn name_edit_click_same_row_keeps_editing() {
    let mut state = sample_state();
    let current = state.selected_index().unwrap();
    state.start_name_edit();
    let interaction = state.click_item(current, 0);
    assert!(interaction.pending_edit().is_none());
    assert_eq!(state.selected_index(), Some(current));
    assert!(state.name_edit().is_some());
}

#[test]
fn restore_name_edit_reselects_and_keeps_draft() {
    let mut state = sample_state();
    let id = state
        .item_at_filtered(1)
        .expect("second row")
        .id()
        .to_string();
    assert!(state.restore_edit(&id, EditTarget::Name, "half typed".to_string()));
    assert_eq!(state.selected_index(), Some(1));
    assert_eq!(
        state.name_edit().expect("editing").field().text(),
        "half typed"
    );
    assert!(!state.restore_edit("ghost", EditTarget::Name, "x".to_string()));
}

#[test]
fn select_row_prefers_exact_alias_over_first_match() {
    let mut state = LibraryPageState::default();
    state.replace_items(LibraryTrigger::expand(multi_alias_list_item()));
    // Same trigger id owns three rows; the middle alias must win.
    assert!(state.select_row("id-multi", "gst"));
    assert_eq!(
        state
            .item_at_filtered(state.selected_index().unwrap())
            .unwrap()
            .trigger(),
        "gst"
    );
    // Unknown invocation falls back to the id match.
    assert!(state.select_row("id-multi", "gone"));
    assert_eq!(
        state
            .item_at_filtered(state.selected_index().unwrap())
            .unwrap()
            .id(),
        "id-multi"
    );
    assert!(!state.select_row("ghost", "gst"));
}

#[test]
fn select_by_id_clamps_to_known_rows() {
    let mut state = sample_state();
    let id = state
        .item_at_filtered(2)
        .expect("third row")
        .id()
        .to_string();
    assert!(state.select_by_id(&id));
    assert_eq!(state.selected_index(), Some(2));
    assert!(!state.select_by_id("ghost"));
}

#[test]
fn name_edit_hit_only_on_name_cells() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);
    // Center content at x=25 width=31: toggle owns the last 5 cells,
    // the cell before it is a gap, everything left of it edits with
    // the click column carried through.
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            30,
            1
        ),
        Some(detail::DetailHit::NameEditAt(5))
    );
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            56,
            1
        ),
        None
    );
    assert_eq!(
        detail::hit_test(
            area,
            state.split_ratio(),
            state.detail_ratio(),
            &state,
            52,
            1
        ),
        Some(detail::DetailHit::EnableToggle)
    );
}
