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
fn word_text_maps_to_snippet() {
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
    assert_eq!(item.kind_label(), "snippet");
}

#[test]
fn word_script_maps_to_script() {
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
    assert_eq!(item.kind_label(), "script");
}

#[test]
fn hotkey_text_maps_to_hotkey_snippet() {
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
    assert_eq!(item.kind_label(), "hotkey snippet");
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
    state.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    assert_eq!(state.selected_index(), Some(0));

    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));

    assert_eq!(state.selected_index(), Some(2));
}

#[test]
fn selection_moves_to_first_match_when_filter_removes_selected_item() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
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
fn pressing_n_is_reserved_while_editor_is_removed() {
    let mut state = sample_state();

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    assert!(interaction.pending_delete().is_none());
    assert!(state.modal().is_none());
}

#[test]
fn pressing_x_opens_export_modal() {
    let mut state = sample_state();

    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    assert!(matches!(state.modal(), Some(LibraryModal::Export(_))));
}

#[test]
fn pressing_i_opens_import_modal() {
    let mut state = sample_state();

    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    assert!(matches!(state.modal(), Some(LibraryModal::Import(_))));
}

#[test]
fn import_modal_defaults_match_current_behavior() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    let Some(LibraryModal::Export(modal)) = state.modal() else {
        panic!("expected export modal");
    };
    assert!(modal.path().ends_with(".tau"));
    assert_eq!(modal.password_display_value(), "");
}

#[test]
fn export_modal_tab_moves_through_all_fields() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(matches!(state.modal(), Some(LibraryModal::Export(_))));
}

#[test]
fn pressing_d_with_selected_trigger_opens_delete_confirmation_modal() {
    let mut state = sample_state();

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert!(matches!(
        state.modal(),
        Some(LibraryModal::ConfirmDelete(_))
    ));
}

#[test]
fn pressing_escape_closes_open_modal() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(state.modal().is_none());
}

#[test]
fn delete_confirmation_owns_input_and_keeps_search_inactive() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

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
    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    assert!(state.modal().is_none());
}

#[test]
fn delete_confirmation_enter_creates_pending_delete() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let pending = interaction.pending_delete().expect("pending delete");
    assert_eq!(pending.trigger_id, "id-alt+r");
    assert_eq!(pending.restore_index(), 0);
}

#[test]
fn select_after_delete_chooses_nearest_remaining_item() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
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
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    let selected_before = state.selected_index();

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
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
fn multi_alias_entry_expands_to_one_row_per_alias() {
    let rows = LibraryTrigger::expand(multi_alias_list_item());

    let triggers: Vec<&str> = rows.iter().map(|row| row.trigger()).collect();
    assert_eq!(triggers, vec!["gs", "gst", "ctrl+g"]);
    assert!(rows.iter().all(|row| row.id() == "id-multi"));
    assert_eq!(rows[0].kind_label(), "snippet");
    assert_eq!(rows[2].kind_label(), "hotkey snippet");
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
    assert_eq!(alias_line("word", "hi", false), "word: hi");
    assert_eq!(
        alias_line("voice", "say hi", true),
        "voice: say hi (confirm)"
    );
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
    for ch in ['1', '2', 'q'] {
        let mut state = sample_state();
        state.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        assert!(!state.is_search_active());
        assert_eq!(state.search_query(), "");
    }
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
        Some(list::LibraryHit::Search)
    );
    assert_eq!(list::hit_test(area, &state, 79, 5), None);
}

#[test]
fn select_last_targets_final_row() {
    let mut state = sample_state();
    state.select_last();
    assert_eq!(state.selected_index(), Some(2));
}

#[test]
fn select_last_on_empty_state_stays_zero() {
    let mut state = LibraryPageState::default();
    state.select_last();
    assert_eq!(state.selected_index(), None);
}

#[test]
fn right_pane_clicks_hit_nothing() {
    let state = sample_state();
    let area = ratatui::layout::Rect::new(0, 0, 80, 30);

    assert_eq!(list::hit_test(area, &state, 60, 5), None);
    assert_eq!(list::hit_test(area, &state, 60, 28), None);
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
        state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    }
    assert_eq!(state.selected_index(), Some(4));
    assert_eq!(state.visible_window(2), (3, 5));

    state.click_item(3, 3);
    assert_eq!(state.selected_index(), Some(3));
    assert_eq!(state.visible_window(2), (3, 5));
}

#[test]
fn stale_anchor_falls_back_to_default_window() {
    let mut state = six_item_state();
    state.click_item(0, 3);
    assert_eq!(state.selected_index(), Some(0));
    assert_eq!(state.visible_window(2), (0, 2));
}
