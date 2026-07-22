use super::completion::{
    CompletionKeyAction, CompletionKeyKind, completion_is_active, completion_key_action,
    completion_key_kind_from_tab_like, should_swallow_trigger_assist_key_release,
    trigger_assist_is_active, trigger_assist_key_action,
};
use super::dispatch::{dispatch_completion_rewrite_with, dispatch_expansion_with};
use std::sync::{Arc, Mutex};

#[test]
fn dispatch_expansion_runs_injection_before_follow_up_consumption() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let inject_events = events.clone();
    let follow_up_events = events.clone();
    let state = Arc::new(taurine_core::engine::EngineState::new('>'));

    let _rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");

    let expansion = taurine_core::engine::ExpansionResult {
        delete_count: 4,
        steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
            "thinking".to_string(),
        )],
        trigger: "ai".to_string(),
        undo_trigger: Some(">ai".to_string()),
        is_calculation: false,
        stat_kind: taurine_core::db::crud::TriggerStatKind::InlineAi,
        track_usage: false,
        follow_up: Some(taurine_core::engine::ExpansionFollowUp::InlineAi {
            prompt: "prompt".to_string(),
            system_prompt_override: Some("expert editor".to_string()),
        }),
    };

    dispatch_expansion_with(
        expansion,
        taurine_core::settings::SpinnerStyle::default(),
        state,
        move |_, _, _| {
            inject_events
                .lock()
                .expect("inject events poisoned")
                .push("inject");
            crate::injector::InjectionReport::default()
        },
        move |follow_up, _| {
            follow_up_events
                .lock()
                .expect("follow-up events poisoned")
                .push("follow_up");
            assert_eq!(
                follow_up,
                Some(taurine_core::engine::ExpansionFollowUp::InlineAi {
                    prompt: "prompt".to_string(),
                    system_prompt_override: Some("expert editor".to_string()),
                })
            );
        },
    );

    assert_eq!(
        &*events.lock().expect("events poisoned"),
        &["inject", "follow_up"]
    );
}

#[test]
fn dispatch_expansion_records_undo_state_for_plain_text_output() {
    let state = Arc::new(taurine_core::engine::EngineState::new('>'));
    let _rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");

    let expansion = taurine_core::engine::ExpansionResult {
        delete_count: 4,
        steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
            "Good Morning".to_string(),
        )],
        trigger: "gm".to_string(),
        undo_trigger: Some(">gm".to_string()),
        is_calculation: false,
        stat_kind: taurine_core::db::crud::TriggerStatKind::Snippet,
        track_usage: false,
        follow_up: None,
    };

    dispatch_expansion_with(
        expansion,
        taurine_core::settings::SpinnerStyle::default(),
        state.clone(),
        move |_, _, _| crate::injector::InjectionReport {
            successful_chars: "Good Morning".chars().count(),
            completed: true,
        },
        move |_, _| {},
    );

    let undo = state
        .take_active_undo_state()
        .expect("undo state should be recorded");
    assert!(undo.trigger_string.starts_with('>'));
    assert_eq!(undo.trigger_string, ">gm");
    assert_eq!(undo.output_length, "Good Morning".chars().count());
}

#[test]
fn dispatch_expansion_skips_undo_registration_for_hotkey_results() {
    let state = Arc::new(taurine_core::engine::EngineState::new('>'));
    let _rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");

    let expansion = taurine_core::engine::ExpansionResult {
        delete_count: 0,
        steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
            "git status".to_string(),
        )],
        trigger: "ctrl+shift+g".to_string(),
        undo_trigger: None,
        is_calculation: false,
        stat_kind: taurine_core::db::crud::TriggerStatKind::Hotkey,
        track_usage: false,
        follow_up: None,
    };

    dispatch_expansion_with(
        expansion,
        taurine_core::settings::SpinnerStyle::default(),
        state.clone(),
        move |_, _, _| crate::injector::InjectionReport {
            successful_chars: "git status".chars().count(),
            completed: true,
        },
        move |_, _| {},
    );

    assert!(state.take_active_undo_state().is_none());
}

#[test]
fn completion_rewrite_dispatch_uses_single_bulk_text_step() {
    let captured = Arc::new(Mutex::new(None));
    let captured_clone = captured.clone();

    dispatch_completion_rewrite_with(
        taurine_core::engine::CompletionRewrite {
            delete_count: 5,
            replacement: "gco".to_string(),
        },
        taurine_core::settings::SpinnerStyle::default(),
        move |steps, delete_count, _| {
            *captured_clone.lock().expect("capture poisoned") = Some((steps, delete_count));
            crate::injector::InjectionReport::default()
        },
    );

    let (steps, delete_count) = captured
        .lock()
        .expect("capture poisoned")
        .clone()
        .expect("rewrite should be captured");
    assert_eq!(delete_count, 5);
    assert_eq!(
        steps,
        vec![taurine_core::engine::variables::ExpansionStep::Text(
            "gco".to_string()
        )]
    );
}

#[test]
fn completion_key_action_wraps_plain_and_shift_tab_into_cycle_actions() {
    assert_eq!(
        completion_key_action(CompletionKeyKind::Tab, false, false, false, false),
        CompletionKeyAction::CycleForward
    );
    assert_eq!(
        completion_key_action(CompletionKeyKind::Tab, true, false, false, false),
        CompletionKeyAction::CycleBackward
    );
}

#[test]
fn completion_key_action_treats_modified_tabs_as_pass_through_cancels() {
    assert_eq!(
        completion_key_action(CompletionKeyKind::Tab, false, false, true, false),
        CompletionKeyAction::CancelAndPassThrough
    );
    assert_eq!(
        completion_key_action(CompletionKeyKind::Tab, false, true, false, false),
        CompletionKeyAction::CancelAndPassThrough
    );
    assert_eq!(
        completion_key_action(CompletionKeyKind::Tab, true, true, false, false),
        CompletionKeyAction::CancelAndPassThrough
    );
    assert_eq!(
        completion_key_action(CompletionKeyKind::Tab, false, false, false, true),
        CompletionKeyAction::CancelAndPassThrough
    );
}

#[test]
fn completion_key_action_swallows_escape_and_vertical_navigation() {
    assert_eq!(
        completion_key_action(CompletionKeyKind::Escape, false, false, false, false),
        CompletionKeyAction::CancelAndSwallow
    );
    assert_eq!(
        completion_key_action(CompletionKeyKind::Up, false, false, false, false),
        CompletionKeyAction::HistoryOlder
    );
    assert_eq!(
        completion_key_action(CompletionKeyKind::Down, false, false, false, false),
        CompletionKeyAction::HistoryNewer
    );
}

#[test]
fn completion_key_kind_from_tab_like_maps_expected_keys() {
    assert_eq!(
        completion_key_kind_from_tab_like(true, false, false, false),
        CompletionKeyKind::Tab
    );
    assert_eq!(
        completion_key_kind_from_tab_like(false, true, false, false),
        CompletionKeyKind::Escape
    );
    assert_eq!(
        completion_key_kind_from_tab_like(false, false, true, false),
        CompletionKeyKind::Up
    );
    assert_eq!(
        completion_key_kind_from_tab_like(false, false, false, true),
        CompletionKeyKind::Down
    );
    assert_eq!(
        completion_key_kind_from_tab_like(false, false, false, false),
        CompletionKeyKind::Other
    );
}

#[test]
fn trigger_assist_key_action_passes_tab_through_when_tab_completion_is_disabled() {
    use std::sync::atomic::Ordering;

    let state = taurine_core::engine::EngineState::new('>');
    state
        .inline_tab_completion_enabled
        .store(false, Ordering::Relaxed);

    assert_eq!(
        trigger_assist_key_action(&state, CompletionKeyKind::Tab, false, false, false, false),
        CompletionKeyAction::PassThrough
    );
}

#[test]
fn trigger_assist_key_action_passes_history_through_when_history_is_disabled() {
    use std::sync::atomic::Ordering;

    let state = taurine_core::engine::EngineState::new('>');
    state.inline_history_enabled.store(false, Ordering::Relaxed);

    assert_eq!(
        trigger_assist_key_action(&state, CompletionKeyKind::Up, false, false, false, false),
        CompletionKeyAction::PassThrough
    );
    assert_eq!(
        trigger_assist_key_action(&state, CompletionKeyKind::Down, false, false, false, false),
        CompletionKeyAction::PassThrough
    );
}

#[test]
fn trigger_assist_key_release_swallowing_respects_feature_settings() {
    use std::sync::atomic::Ordering;

    let state = taurine_core::engine::EngineState::new('>');
    assert!(should_swallow_trigger_assist_key_release(
        &state,
        CompletionKeyKind::Tab
    ));
    assert!(should_swallow_trigger_assist_key_release(
        &state,
        CompletionKeyKind::Up
    ));

    state
        .inline_tab_completion_enabled
        .store(false, Ordering::Relaxed);
    state.inline_history_enabled.store(false, Ordering::Relaxed);

    assert!(!should_swallow_trigger_assist_key_release(
        &state,
        CompletionKeyKind::Tab
    ));
    assert!(!should_swallow_trigger_assist_key_release(
        &state,
        CompletionKeyKind::Down
    ));
}

#[test]
fn completion_is_inactive_after_trigger_character_is_deleted() {
    let state = Arc::new(taurine_core::engine::EngineState::new('>'));
    let mut evaluator = taurine_core::engine::Evaluator::new(state);
    for ch in ">g".chars() {
        assert_eq!(
            evaluator.process_event(
                if ch == ' ' {
                    taurine_core::engine::EngineEvent::ActionKey
                } else {
                    taurine_core::engine::EngineEvent::Char(ch)
                },
                None
            ),
            None
        );
    }
    assert_eq!(
        evaluator.process_event(taurine_core::engine::EngineEvent::Backspace, None),
        None
    );
    assert_eq!(
        evaluator.process_event(taurine_core::engine::EngineEvent::Backspace, None),
        None
    );

    let evaluator = Arc::new(Mutex::new(evaluator));
    assert!(
        !completion_is_active(&evaluator),
        "hook gating must not treat deleted-trigger state as active completion"
    );
}

#[test]
fn dispatch_expansion_promotes_word_trigger_history_on_success() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let state = Arc::new(taurine_core::engine::EngineState::new('>'));
    state.load_actions(vec![
        (
            "email".to_string(),
            taurine_core::db::crud::TriggerAction::text("team update"),
        ),
        (
            "gs".to_string(),
            taurine_core::db::crud::TriggerAction::text("git status"),
        ),
    ]);
    state.load_word_trigger_history(vec!["email".to_string(), "gs".to_string()]);
    let _rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");

    let expansion = taurine_core::engine::ExpansionResult {
        delete_count: 4,
        steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
            "git status".to_string(),
        )],
        trigger: "gs".to_string(),
        undo_trigger: Some(">gs".to_string()),
        is_calculation: false,
        stat_kind: taurine_core::db::crud::TriggerStatKind::InlineAi,
        track_usage: true,
        follow_up: None,
    };

    dispatch_expansion_with(
        expansion,
        taurine_core::settings::SpinnerStyle::default(),
        state.clone(),
        move |_, _, _| crate::injector::InjectionReport {
            successful_chars: "git status".chars().count(),
            completed: true,
        },
        move |_, _| {},
    );

    assert_eq!(
        state.matching_word_trigger_history(""),
        vec!["gs".to_string(), "email".to_string()]
    );
}

#[test]
fn trigger_assist_is_inactive_while_inline_ai_capture_mode_is_active() {
    let state = Arc::new(taurine_core::engine::EngineState::new('>'));
    let mut evaluator = taurine_core::engine::Evaluator::new(state.clone());

    let _ = evaluator.process_event(taurine_core::engine::EngineEvent::Char('>'), None);
    let expansion = evaluator
        .process_event(taurine_core::engine::EngineEvent::Char('>'), None)
        .expect("inline ai capture should start on >>");
    assert_eq!(expansion.trigger, ">>");

    let evaluator = Arc::new(Mutex::new(evaluator));
    assert!(
        !trigger_assist_is_active(&evaluator, state.as_ref()),
        "history and completion keys must not be hijacked once AI capture is active"
    );
}

pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_dispatch_expansion_skips_ai_stats() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    taurine_core::logs::init_tracing_for_tests();
    let test_dir = std::env::temp_dir().join(format!(
        "taurine_ai_stats_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let test_db = test_dir.join("test.db");
    // SAFETY: Setting environment variables for test DB isolation.
    unsafe {
        std::env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap());
        std::env::set_var("TAURINE_DB_PATH", test_db.to_str().unwrap());
    }
    let _ = std::fs::remove_dir_all(&test_dir);
    std::fs::create_dir_all(&test_dir).unwrap();

    let _rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime should build");
    let state = Arc::new(taurine_core::engine::EngineState::new('>'));

    // Initialize/clear db stats for today to have a clean slate
    let conn = taurine_core::db::init::setup().unwrap();
    let today = taurine_core::stats::get_current_date_string();
    let _ = conn.execute("DELETE FROM stats WHERE date = ?1", [&today]);

    // 1. Dispatch Snippet expansion -> should write to stats
    let expansion_snippet = taurine_core::engine::ExpansionResult {
        delete_count: 2,
        steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
            "hello".to_string(),
        )],
        trigger: "h".to_string(),
        undo_trigger: None,
        is_calculation: false,
        stat_kind: taurine_core::db::crud::TriggerStatKind::Snippet,
        track_usage: true,
        follow_up: None,
    };
    dispatch_expansion_with(
        expansion_snippet,
        taurine_core::settings::SpinnerStyle::default(),
        state.clone(),
        move |_, _, _| crate::injector::InjectionReport {
            successful_chars: 5,
            completed: true,
        },
        move |_, _| {},
    );

    // Verify snippet execution was counted (allow background thread to write)
    let mut row = None;
    for _ in 0..30 {
        if let Ok(Some(r)) = taurine_core::db::crud::get_stat(&conn, &today) {
            row = Some(r);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let row = row.expect("Stats row was not written in time");
    assert_eq!(row.executions, 1);
    assert_eq!(row.ai_executions, 0);

    // 2. Dispatch InlineAi expansion -> should NOT write additional stats during dispatch
    let expansion_ai = taurine_core::engine::ExpansionResult {
        delete_count: 2,
        steps: vec![taurine_core::engine::variables::ExpansionStep::Text(
            "thinking".to_string(),
        )],
        trigger: "ai".to_string(),
        undo_trigger: None,
        is_calculation: false,
        stat_kind: taurine_core::db::crud::TriggerStatKind::InlineAi,
        track_usage: true,
        follow_up: None,
    };
    dispatch_expansion_with(
        expansion_ai,
        taurine_core::settings::SpinnerStyle::default(),
        state,
        move |_, _, _| crate::injector::InjectionReport {
            successful_chars: 1,
            completed: true,
        },
        move |_, _| {},
    );

    // Verify executions/ai_executions remain unchanged after AI dispatch (allow sleep anyway to be sure)
    std::thread::sleep(std::time::Duration::from_millis(100));
    let row = taurine_core::db::crud::get_stat(&conn, &today)
        .unwrap()
        .unwrap();
    assert_eq!(row.executions, 1);
    assert_eq!(row.ai_executions, 0);

    // Cleanup env
    let _ = std::fs::remove_dir_all(&test_dir);
    unsafe {
        std::env::remove_var("TAURINE_DATA_DIR");
        std::env::remove_var("TAURINE_DB_PATH");
    }
}
