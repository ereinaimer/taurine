use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use taurine_core::db::crud::{TriggerListItem, TriggerRow, TriggerType};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};
use taurine_core::exchange::ImportStatsMode;

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
        name: trigger.to_string(),
        description: description.map(str::to_string),
        trigger_type,
        trigger: trigger.to_string(),
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

fn trigger_row(
    trigger_type: TriggerType,
    trigger: &str,
    output: &str,
    action_type: &str,
    target_os: &str,
    usage_count: i64,
    script_content: Option<&str>,
) -> TriggerRow {
    TriggerRow {
        id: format!("trigger-{trigger}"),
        name: format!("Trigger {trigger}"),
        description: Some("Open Reddit".to_string()),
        trigger_type,
        trigger: trigger.to_string(),
        output: output.to_string(),
        action_type: action_type.to_string(),
        target_os: target_os.to_string(),
        only_apps: None,
        except_apps: None,
        tags: "[]".to_string(),
        usage_count,
        last_used_at: Some(1),
        created_at: 1,
        updated_at: 1,
        version: 1,
        is_deleted: false,
        is_synced: true,
        is_enabled: true,
        auto_case: false,
        interpreter: Some(ScriptInterpreter::PowerShell),
        behavior: Some(ScriptBehavior::Silent),
        script_binary: script_content
            .map(|content| taurine_core::engine::shell::compress(content).unwrap()),
    }
}

fn sample_state() -> LibraryPageState {
    let mut state = LibraryPageState::default();
    state.replace_items(vec![
        LibraryTrigger::from(list_item(
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
        LibraryTrigger::from(list_item(
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
        LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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
    let item = LibraryTrigger::from(list_item(
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

    assert_eq!(item.preview(), DEFAULT_SCRIPT_FALLBACK);
}

#[test]
fn search_matches_trigger() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));

    assert_eq!(state.filtered_len(), 1);
    assert_eq!(state.item_at_filtered(0).unwrap().trigger(), "gm");
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
    state.replace_items(vec![LibraryTrigger::from(TriggerListItem {
        id: "id-alt+r".to_string(),
        name: "Reddit opener".to_string(),
        description: Some("Open Reddit".to_string()),
        trigger_type: TriggerType::Hotkey,
        trigger: "alt+r".to_string(),
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
fn metadata_uses_double_slash_separator() {
    let item = LibraryTrigger::from(list_item(
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

    assert_eq!(item.metadata_label(), "all // 9 uses");
}

#[test]
fn normalized_modal_text_preserves_meaningful_outer_whitespace() {
    assert_eq!(
        normalized_modal_text(Some("  padded body  ")).as_deref(),
        Some("  padded body  ")
    );
    assert_eq!(
        normalized_modal_text(Some("first\r\nsecond")).as_deref(),
        Some("first\nsecond")
    );
}

#[test]
fn kind_selector_uses_kind_title() {
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    let mut state = sample_state();
    state.open_editor_modal(detail);

    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let Some(LibraryModal::Editor(modal)) = state.modal() else {
        panic!("expected editor modal");
    };
    assert_eq!(
        modal.selector().map(LibrarySelectState::title),
        Some("Select Kind")
    );
}

#[test]
fn target_os_selector_uses_target_os_title() {
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    let mut state = sample_state();
    state.open_editor_modal(detail);

    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let Some(LibraryModal::Editor(modal)) = state.modal() else {
        panic!("expected editor modal");
    };
    assert_eq!(
        modal.selector().map(LibrarySelectState::title),
        Some("Select Target OS")
    );
}

#[test]
fn pressing_enter_requests_selected_trigger_modal() {
    let mut state = sample_state();
    let expected_id = state
        .selected_index()
        .and_then(|index| state.item_at_filtered(index))
        .map(|item| item.id().to_string());

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        interaction.into_open_request(),
        expected_id.map(LibraryOpenRequest::Selected)
    );
}

#[test]
fn pressing_n_opens_editor_modal_in_create_mode() {
    let mut state = sample_state();

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    assert_eq!(
        interaction.into_open_request(),
        Some(LibraryOpenRequest::Create)
    );
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
    assert!(!modal.include_settings());
    assert_eq!(modal.stats_mode(), ImportStatsMode::Ignore);
    assert_eq!(modal.conflict_mode(), LibraryImportConflictMode::Skip);
    assert_eq!(state.footer_text(), LIBRARY_IMPORT_MODAL_FOOTER);
}

#[test]
fn import_modal_requires_non_empty_path() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
        panic!("expected import modal");
    };
    // Tab through all fields to ActionButton
    for _ in 0..6 {
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
fn import_modal_stats_selector_uses_existing_modes() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    let Some(LibraryModal::Import(modal)) = state.modal.as_mut() else {
        panic!("expected import modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

    let selector = modal.selector().expect("stats selector");
    assert_eq!(selector.title(), "Select Stats Mode");
    assert_eq!(selector.options, vec!["ignore", "merge", "overwrite"]);
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
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
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
    let outcome = LibraryImportOutcome::new(12, true, true);
    let mut state = sample_state();

    state.open_import_result_modal(&outcome);

    let Some(LibraryModal::ImportResult(modal)) = state.modal() else {
        panic!("expected import result modal");
    };
    assert_eq!(modal.lines()[0], "Imported 12 trigger(s).");
    assert!(
        modal
            .lines()
            .iter()
            .any(|line| line == "Settings imported.")
    );
    assert!(modal.lines().iter().any(|line| line == "Stats updated."));
    assert_eq!(state.footer_text(), LIBRARY_IMPORT_RESULT_FOOTER);
}

#[test]
fn import_result_modal_closes_on_enter() {
    let outcome = LibraryImportOutcome::new(3, false, false);
    let mut state = sample_state();
    state.open_import_result_modal(&outcome);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn import_result_modal_closes_on_escape() {
    let outcome = LibraryImportOutcome::new(3, false, false);
    let mut state = sample_state();
    state.open_import_result_modal(&outcome);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn import_result_modal_owns_input_and_keeps_search_inactive() {
    let outcome = LibraryImportOutcome::new(3, false, false);
    let mut state = sample_state();
    state.open_import_result_modal(&outcome);

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(matches!(state.modal(), Some(LibraryModal::ImportResult(_))));
}

#[test]
fn export_result_modal_body_for_triggers_without_encryption_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, false, false, false);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(modal.body(), "Triggers are exported to: backup.tau");
}

#[test]
fn export_result_modal_body_for_triggers_with_encryption_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, true, false, false);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers are exported to: backup.tau as an encrypted export."
    );
}

#[test]
fn export_result_modal_body_for_triggers_and_settings_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, false, true, false);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers and Settings were exported to: backup.tau"
    );
}

#[test]
fn export_result_modal_body_for_triggers_and_settings_with_encryption_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, true, true, false);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers and Settings were exported to: backup.tau with encryption."
    );
}

#[test]
fn export_result_modal_body_for_triggers_and_stats_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, false, false, true);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers and Stats were exported to: backup.tau"
    );
}

#[test]
fn export_result_modal_body_for_triggers_and_stats_with_encryption_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, true, false, true);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers and Stats were exported to: backup.tau with encryption."
    );
}

#[test]
fn export_result_modal_body_for_all_export_data_without_encryption_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, false, true, true);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers, Settings and Stats were exported to: backup.tau"
    );
}

#[test]
fn export_result_modal_body_for_all_export_data_with_encryption_matches_exactly() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, true, true, true);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert_eq!(
        modal.body(),
        "Triggers, Settings and Stats were exported to: backup.tau with encryption."
    );
}

#[test]
fn export_result_modal_does_not_use_separate_encryption_line() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");

    state.open_export_result_modal(&path, true, true, true);

    let Some(LibraryModal::ExportResult(modal)) = state.modal() else {
        panic!("expected export result modal");
    };
    assert!(!modal.body().contains("This export was encrypted."));
    assert_eq!(state.footer_text(), LIBRARY_EXPORT_RESULT_FOOTER);
}

#[test]
fn export_result_modal_closes_on_enter() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");
    state.open_export_result_modal(&path, false, false, false);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn export_result_modal_closes_on_escape() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");
    state.open_export_result_modal(&path, false, false, false);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn export_result_modal_owns_input_and_keeps_search_inactive() {
    let mut state = sample_state();
    let path = PathBuf::from("backup.tau");
    state.open_export_result_modal(&path, false, false, false);

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
    assert!(modal.encrypt());
    assert_eq!(modal.password_display_value(), "");
    assert!(!modal.include_settings());
    assert!(!modal.include_stats());
    assert_eq!(state.footer_text(), LIBRARY_EXPORT_MODAL_FOOTER);
}

#[test]
fn export_modal_tab_skips_password_when_encryption_is_disabled() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryExportModalField::Encrypt);

    modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    assert!(!modal.encrypt());

    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryExportModalField::IncludeSettings);
}

#[test]
fn export_modal_requires_password_when_encryption_is_enabled() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    // Tab through all fields to ActionButton
    for _ in 0..6 {
        modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    }
    modal.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(interaction.pending_export().is_none());
    assert_eq!(modal.error(), Some("Encryption password is required."));
}

#[test]
fn export_modal_password_field_stores_typed_characters() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    assert_eq!(modal.password_display_value(), "***");
}

#[test]
fn enter_on_confirm_creates_pending_export_when_plaintext() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));

    let Some(LibraryModal::Export(modal)) = state.modal.as_mut() else {
        panic!("expected export modal");
    };
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    // Tab through remaining fields to ActionButton
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let pending = interaction.pending_export().expect("pending export");
    assert!(!pending.encrypt);
    assert_eq!(pending.password, None);
    assert!(!pending.include_settings);
    assert!(!pending.include_stats);
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
fn pressing_d_from_editor_edit_mode_keeps_editor_open() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    state.open_editor_modal(detail);

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
}

#[test]
fn typing_d_in_create_modal_trigger_field_inserts_text() {
    let mut state = sample_state();
    state.open_create_modal();

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    let Some(LibraryModal::Editor(modal)) = state.modal() else {
        panic!("expected editor modal");
    };
    assert_eq!(modal.trigger(), "d");
    assert!(modal.error().is_none());
}

#[test]
fn typing_d_in_create_modal_content_field_inserts_text() {
    let mut state = sample_state();
    state.open_create_modal();
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    let Some(LibraryModal::Editor(modal)) = state.modal() else {
        panic!("expected editor modal");
    };
    assert_eq!(modal.content(), "d");
    assert!(modal.error().is_none());
}

#[test]
fn typing_d_in_edit_modal_trigger_field_inserts_text() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    state.open_editor_modal(detail);

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    let Some(LibraryModal::Editor(modal)) = state.modal() else {
        panic!("expected editor modal");
    };
    assert_eq!(modal.trigger(), "gmd");
    assert!(modal.error().is_none());
}

#[test]
fn typing_d_in_edit_modal_content_field_inserts_text() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    state.open_editor_modal(detail);
    state.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    let Some(LibraryModal::Editor(modal)) = state.modal() else {
        panic!("expected editor modal");
    };
    assert_eq!(modal.content(), "Good Morningd");
    assert!(modal.error().is_none());
}

#[test]
fn pressing_d_from_create_mode_does_not_open_delete_confirmation() {
    let mut state = sample_state();
    state.open_create_modal();

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
}

#[test]
fn pressing_d_from_edit_mode_with_text_focus_does_not_open_delete_confirmation() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    state.open_editor_modal(detail);

    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
}

#[test]
fn pressing_escape_closes_open_modal() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Hotkey,
        "alt+r",
        "[Script: powershell]",
        "script",
        "win",
        6,
        Some("Start-Process https://reddit.com"),
    ))
    .unwrap();
    state.open_editor_modal(detail);

    let interaction = state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(interaction.should_close_modal());
}

#[test]
fn script_modal_uses_actual_script_content_instead_of_description() {
    let mut row = trigger_row(
        TriggerType::Hotkey,
        "alt+r",
        "[Script: powershell]",
        "script",
        "win",
        6,
        Some("Start-Process https://reddit.com"),
    );
    row.description = Some("Open Reddit".to_string());

    let detail = LibraryTriggerDetail::from_row(row).unwrap();

    assert_eq!(detail.content_label(), "Script");
    assert_eq!(detail.content(), "Start-Process https://reddit.com");
}

#[test]
fn snippet_modal_uses_actual_output_content() {
    let row = trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    );

    let detail = LibraryTriggerDetail::from_row(row).unwrap();

    assert_eq!(detail.content_label(), "Output");
    assert_eq!(detail.content(), "Good Morning");
}

#[test]
fn modal_footer_replaces_library_actions_while_open() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();

    state.open_editor_modal(detail);

    assert_eq!(state.footer_text(), LIBRARY_EDIT_MODAL_FOOTER);
}

#[test]
fn modal_owns_input_and_keeps_search_inactive() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    state.open_editor_modal(detail);

    state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

    assert!(!state.is_search_active());
    assert!(state.is_modal_open());
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
fn delete_confirmation_cancel_restores_editor_modal() {
    let mut state = sample_state();
    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "gm",
        "Good Morning",
        "text",
        "all",
        9,
        None,
    ))
    .unwrap();
    state.open_editor_modal(detail);
    state.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

    state.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));

    assert!(matches!(state.modal(), Some(LibraryModal::Editor(_))));
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
        LibraryTrigger::from(list_item(
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
        LibraryTrigger::from(list_item(
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
fn tab_and_shift_tab_cycle_modal_focus() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap(),
    );

    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Content);

    modal.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(modal.focus(), LibraryModalField::Trigger);
}

#[test]
fn content_focus_supports_cursor_navigation() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Word,
            "gm",
            "line one\nline two\nline three",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    modal.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

    assert_eq!(modal.content_line_indicator(1).as_deref(), Some("2/3"));
}

#[test]
fn editor_modal_initializes_editable_fields_from_selected_trigger() {
    let modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );

    assert_eq!(modal.trigger(), "alt+r");
    assert_eq!(modal.content(), "Start-Process https://reddit.com");
    assert_eq!(modal.kind_label(), "hotkey script");
    assert_eq!(modal.target_os(), "windows");
}

#[test]
fn editing_trigger_updates_modal_draft_state() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Word,
            "gm",
            "Good Morning",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap(),
    );

    modal.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::SHIFT));

    assert_eq!(modal.trigger(), "gm!");
}

#[test]
fn editing_content_updates_modal_draft_state() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Word,
            "gm",
            "Good",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT));

    assert_eq!(modal.content(), "Good M");
}

#[test]
fn kind_selector_updates_kind_on_enter() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Word,
            "gm",
            "Good",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(modal.selector().is_some());
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.kind_label(), "script");
    assert_eq!(modal.content_label(), "Script");
}

#[test]
fn create_modal_initially_hides_language_and_mode_for_snippet() {
    let modal = LibraryEditorModalState::new_create();

    assert_eq!(modal.visible_fields(), &SNIPPET_MODAL_FIELDS);
    assert!(!modal.is_script_kind());
}

#[test]
fn changing_kind_to_script_shows_language_and_mode() {
    let mut modal = LibraryEditorModalState::new_create();
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.kind_label(), "script");
    assert_eq!(modal.visible_fields(), &SCRIPT_MODAL_FIELDS);
    assert_eq!(
        modal.interpreter(),
        default_script_interpreter_for_target_os("all")
    );
    assert_eq!(modal.mode_label(), "inline");
}

#[test]
fn changing_kind_to_hotkey_script_shows_language_and_mode() {
    let mut modal = LibraryEditorModalState::new_create();
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.kind_label(), "hotkey script");
    assert_eq!(modal.visible_fields(), &SCRIPT_MODAL_FIELDS);
    assert_eq!(modal.mode_label(), "inline");
}

#[test]
fn changing_kind_back_to_snippet_hides_language_and_mode() {
    let mut modal = LibraryEditorModalState::new_create();
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.kind_label(), "snippet");
    assert_eq!(modal.visible_fields(), &SNIPPET_MODAL_FIELDS);
    assert_eq!(modal.focus(), LibraryModalField::Kind);
}

#[test]
fn new_script_mode_defaults_to_inline() {
    let mut modal = LibraryEditorModalState::new_create();
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.behavior(), ScriptBehavior::Inline);
    assert_eq!(modal.mode_label(), "inline");
}

#[test]
fn language_selector_uses_exact_supported_options() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let selector = modal.selector().expect("language selector");
    assert_eq!(selector.title(), "Select Language");
    assert_eq!(
        selector.options,
        vec!["bash", "powershell", "python", "node", "node-esm", "cmd"]
    );
}

#[test]
fn mode_selector_uses_exact_supported_options() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let selector = modal.selector().expect("mode selector");
    assert_eq!(selector.title(), "Select Mode");
    assert_eq!(selector.options, vec!["inline", "silent"]);
}

#[test]
fn selecting_language_updates_draft_language() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.language_label(), "python");
}

#[test]
fn selecting_mode_updates_draft_mode() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.behavior(), ScriptBehavior::Inline);
    assert_eq!(modal.mode_label(), "inline");
}

#[test]
fn tab_visits_language_and_mode_only_for_script_kinds() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );

    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Content);
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Kind);
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::TargetOs);
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Language);
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Mode);
}

#[test]
fn tab_skips_language_and_mode_for_snippet_kinds() {
    let mut modal = LibraryEditorModalState::new_create();

    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Content);
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Kind);
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::TargetOs);
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(modal.focus(), LibraryModalField::Trigger);
}

#[test]
fn typing_j_and_k_in_content_field() {
    let mut modal = LibraryEditorModalState::new_create();
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)); // Focus content

    modal.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));

    assert_eq!(modal.content(), "jk");
}

#[test]
fn target_os_selector_updates_target_os_on_enter() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Word,
            "gm",
            "Good",
            "text",
            "all",
            9,
            None,
        ))
        .unwrap(),
    );
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(modal.selector().is_some());
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(modal.target_os(), "windows");
}

#[test]
fn ctrl_s_creates_pending_save_for_existing_trigger() {
    let mut modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );

    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let pending = interaction.pending_save().unwrap();

    assert_eq!(pending.kind, LibraryKind::HotkeyScript);
    assert_eq!(pending.content, "Start-Process https://reddit.com");
    assert_eq!(pending.interpreter, Some(ScriptInterpreter::PowerShell));
    assert_eq!(pending.behavior, Some(ScriptBehavior::Silent));
    assert!(matches!(
        pending.mode(),
        PendingLibrarySaveMode::Update { id, .. } if id == "trigger-alt+r"
    ));
}

#[test]
fn create_modal_initializes_empty_defaults() {
    let modal = LibraryEditorModalState::new_create();

    assert_eq!(modal.mode(), LibraryEditorMode::Create);
    assert_eq!(modal.trigger(), "");
    assert_eq!(modal.content(), "");
    assert_eq!(modal.kind_label(), "snippet");
    assert_eq!(modal.target_os(), "all");
    assert_eq!(
        modal.interpreter(),
        default_script_interpreter_for_target_os("all")
    );
    assert_eq!(modal.behavior(), ScriptBehavior::Inline);
}

#[test]
fn ctrl_s_creates_pending_save_for_new_trigger() {
    let mut modal = LibraryEditorModalState::new_create();
    modal.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT));
    modal.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let pending = interaction.pending_save().unwrap();

    assert!(matches!(pending.mode(), PendingLibrarySaveMode::Create));
    assert_eq!(pending.kind, LibraryKind::Snippet);
    assert_eq!(pending.target_os, "all");
    assert_eq!(pending.trigger, "gm");
    assert_eq!(pending.content, "Hi");
    assert_eq!(pending.interpreter, None);
    assert_eq!(pending.behavior, None);
}

#[test]
fn ctrl_s_for_new_script_captures_language_and_mode() {
    let mut modal = LibraryEditorModalState::new_create();
    modal.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let language_steps = match modal.interpreter() {
        ScriptInterpreter::Bash => 2,
        ScriptInterpreter::PowerShell => 1,
        _ => 0,
    };
    for _ in 0..language_steps {
        modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    }
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    modal.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let interaction = modal.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let pending = interaction.pending_save().unwrap();

    assert_eq!(pending.kind, LibraryKind::Script);
    assert_eq!(pending.interpreter, Some(ScriptInterpreter::Python));
    assert_eq!(pending.behavior, Some(ScriptBehavior::Silent));
}

#[test]
fn editing_existing_script_preserves_language_and_mode() {
    let modal = LibraryEditorModalState::new_edit(
        LibraryTriggerDetail::from_row(trigger_row(
            TriggerType::Hotkey,
            "alt+r",
            "[Script: powershell]",
            "script",
            "win",
            6,
            Some("Start-Process https://reddit.com"),
        ))
        .unwrap(),
    );

    assert_eq!(modal.language_label(), "powershell");
    assert_eq!(modal.mode_label(), "silent");
}

#[test]
fn modal_keeps_library_selection_stable_after_close() {
    let mut state = sample_state();
    state.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    let selected_before = state.selected_index();

    let detail = LibraryTriggerDetail::from_row(trigger_row(
        TriggerType::Word,
        "deploy",
        "[Script: bash]",
        "script",
        "linux",
        4,
        Some("npm run build && npm publish"),
    ))
    .unwrap();
    state.open_editor_modal(detail);
    state.clear_modal();

    assert_eq!(state.selected_index(), selected_before);
    assert_eq!(state.search_query(), "");
}
