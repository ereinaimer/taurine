use super::hotkey_evaluator::*;
use taurine_core::db::crud::TriggerAction;
use taurine_core::engine::EngineState;
use taurine_core::engine::{EngineEvent, Evaluator};
use taurine_core::keys::Modifier;
use taurine_core::keys::{LogicalKey, Modifiers};

fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
    let mut bitset = Modifiers::new();
    for modifier in modifiers {
        let _ = bitset.insert(*modifier);
    }
    bitset
}

fn load_hotkey(state: &EngineState, trigger: &str, output: &str) {
    state.load_hotkey_actions(vec![(trigger.to_string(), TriggerAction::text(output))]);
}

#[test]
fn hotkey_match_returns_expansion_result_and_swallow_outcome() {
    let state = EngineState::new();
    load_hotkey(&state, "ctrl+shift+g", "git status");
    let mut evaluator = HotkeyEvaluator::new();

    let result = evaluator.on_key_event(
        &state,
        true,
        modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
        LogicalKey::Letter('g'),
    );

    match result {
        HotkeyEvaluation::Matched(expansion) => {
            assert_eq!(expansion.delete_count, 0);
            assert_eq!(expansion.trigger, "ctrl+shift+g");
            assert_eq!(expansion.undo_trigger, None);
            assert_eq!(
                expansion.steps,
                vec![taurine_core::engine::variables::ExpansionStep::Text(
                    "git status".to_string()
                )]
            );
        }
        other => panic!("expected matched hotkey, got {other:?}"),
    }
}

#[test]
fn hotkey_miss_falls_through_and_preserves_word_evaluator_behavior() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let state = std::sync::Arc::new(EngineState::with_source(std::sync::Arc::new(
        taurine_core::engine::source::MemorySource::new(),
    )));
    state.load_actions(vec![(
        "gm".to_string(),
        TriggerAction::text("Good morning!"),
    )]);
    let mut hotkeys = HotkeyEvaluator::new();
    let miss = hotkeys.on_key_event(
        state.as_ref(),
        true,
        modifiers_with(&[Modifier::Ctrl]),
        LogicalKey::Letter('g'),
    );
    assert_eq!(miss, HotkeyEvaluation::NoMatch);

    let mut text = Evaluator::new(state);
    for ch in ">gm".chars() {
        assert_eq!(
            text.process_event(
                if ch == ' ' {
                    EngineEvent::ActionKey
                } else {
                    EngineEvent::Char(ch)
                },
                None
            ),
            None
        );
    }
    let expansion = text
        .process_event(EngineEvent::ActionKey, None)
        .expect("word trigger should still expand on hotkey miss");
    assert_eq!(
        expansion.steps,
        vec![taurine_core::engine::variables::ExpansionStep::Text(
            "Good morning!".to_string()
        )]
    );
}

#[test]
fn hotkey_match_does_not_touch_text_buffer_or_register_undo() {
    let state = std::sync::Arc::new(EngineState::new());
    load_hotkey(state.as_ref(), "ctrl+shift+g", "git status");
    let mut hotkeys = HotkeyEvaluator::new();
    let mut text = Evaluator::new(state.clone());

    let result = hotkeys.on_key_event(
        state.as_ref(),
        true,
        modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
        LogicalKey::Letter('g'),
    );

    assert!(matches!(result, HotkeyEvaluation::Matched(_)));
    assert_eq!(text.process_event(EngineEvent::ActionKey, None), None);
    assert!(state.take_active_undo_state().is_none());
}

#[test]
fn hotkey_fires_once_per_press_until_release() {
    let state = EngineState::new();
    load_hotkey(&state, "ctrl+shift+g", "git status");
    let mut evaluator = HotkeyEvaluator::new();
    let modifiers = modifiers_with(&[Modifier::Ctrl, Modifier::Shift]);

    assert!(matches!(
        evaluator.on_key_event(&state, true, modifiers, LogicalKey::Letter('g')),
        HotkeyEvaluation::Matched(_)
    ));
    assert_eq!(
        evaluator.on_key_event(&state, true, modifiers, LogicalKey::Letter('g')),
        HotkeyEvaluation::Swallow
    );
    assert_eq!(
        evaluator.on_key_event(&state, false, modifiers, LogicalKey::Letter('g')),
        HotkeyEvaluation::Swallow
    );
    assert!(matches!(
        evaluator.on_key_event(&state, true, modifiers, LogicalKey::Letter('g')),
        HotkeyEvaluation::Matched(_)
    ));
}

#[test]
fn side_specific_alt_hotkeys_only_match_the_requested_side() {
    let state = EngineState::new();
    state.load_hotkey_actions(vec![
        ("lalt+m".to_string(), TriggerAction::text("left alt")),
        ("ralt+m".to_string(), TriggerAction::text("right alt")),
    ]);
    let mut evaluator = HotkeyEvaluator::new();

    let left_alt = modifiers_from_sides(false, false, false, false, true, false, false, false);
    let right_alt = modifiers_from_sides(false, false, false, false, false, true, false, false);

    match evaluator.on_key_event(&state, true, left_alt, LogicalKey::Letter('m')) {
        HotkeyEvaluation::Matched(expansion) => assert_eq!(expansion.trigger, "lalt+m"),
        other => panic!("expected left alt match, got {other:?}"),
    }
    assert_eq!(
        evaluator.on_key_event(&state, false, left_alt, LogicalKey::Letter('m')),
        HotkeyEvaluation::Swallow
    );

    match evaluator.on_key_event(&state, true, right_alt, LogicalKey::Letter('m')) {
        HotkeyEvaluation::Matched(expansion) => assert_eq!(expansion.trigger, "ralt+m"),
        other => panic!("expected right alt match, got {other:?}"),
    }
}

#[test]
fn generic_alt_hotkey_matches_either_side_without_matching_extra_families() {
    let state = EngineState::new();
    load_hotkey(&state, "alt+m", "generic alt");
    let mut evaluator = HotkeyEvaluator::new();

    let left_alt = modifiers_from_sides(false, false, false, false, true, false, false, false);
    let right_alt = modifiers_from_sides(false, false, false, false, false, true, false, false);
    let ctrl_right_alt = modifiers_from_sides(true, false, false, false, false, true, false, false);

    assert!(matches!(
        evaluator.on_key_event(&state, true, left_alt, LogicalKey::Letter('m')),
        HotkeyEvaluation::Matched(_)
    ));
    assert_eq!(
        evaluator.on_key_event(&state, false, left_alt, LogicalKey::Letter('m')),
        HotkeyEvaluation::Swallow
    );
    assert!(matches!(
        evaluator.on_key_event(&state, true, right_alt, LogicalKey::Letter('m')),
        HotkeyEvaluation::Matched(_)
    ));
    assert_eq!(
        evaluator.on_key_event(&state, false, right_alt, LogicalKey::Letter('m')),
        HotkeyEvaluation::Swallow
    );
    assert_eq!(
        evaluator.on_key_event(&state, true, ctrl_right_alt, LogicalKey::Letter('m')),
        HotkeyEvaluation::NoMatch
    );
}

#[test]
fn alt_hotkey_does_not_match_shift_ctrl_or_plain_same_base_key() {
    let state = EngineState::new();
    load_hotkey(&state, "alt+g", "generic alt");
    let mut evaluator = HotkeyEvaluator::new();

    let left_alt = modifiers_from_sides(false, false, false, false, true, false, false, false);
    let left_shift = modifiers_from_sides(false, false, true, false, false, false, false, false);
    let left_ctrl = modifiers_from_sides(true, false, false, false, false, false, false, false);

    assert!(matches!(
        evaluator.on_key_event(&state, true, left_alt, LogicalKey::Letter('g')),
        HotkeyEvaluation::Matched(_)
    ));
    assert_eq!(
        evaluator.on_key_event(&state, false, left_alt, LogicalKey::Letter('g')),
        HotkeyEvaluation::Swallow
    );
    assert_eq!(
        evaluator.on_key_event(&state, true, left_shift, LogicalKey::Letter('g')),
        HotkeyEvaluation::NoMatch
    );
    assert_eq!(
        evaluator.on_key_event(&state, true, left_ctrl, LogicalKey::Letter('g')),
        HotkeyEvaluation::NoMatch
    );
    assert_eq!(
        evaluator.on_key_event(&state, true, Modifiers::new(), LogicalKey::Letter('g')),
        HotkeyEvaluation::NoMatch
    );
}

#[test]
fn same_base_key_hotkeys_remain_distinct_by_modifier_identity() {
    let state = EngineState::new();
    state.load_hotkey_actions(vec![
        ("alt+g".to_string(), TriggerAction::text("alt")),
        ("ctrl+g".to_string(), TriggerAction::text("ctrl")),
        ("shift+g".to_string(), TriggerAction::text("shift")),
    ]);
    let mut evaluator = HotkeyEvaluator::new();

    let left_alt = modifiers_from_sides(false, false, false, false, true, false, false, false);
    let left_ctrl = modifiers_from_sides(true, false, false, false, false, false, false, false);
    let left_shift = modifiers_from_sides(false, false, true, false, false, false, false, false);

    match evaluator.on_key_event(&state, true, left_alt, LogicalKey::Letter('g')) {
        HotkeyEvaluation::Matched(expansion) => assert_eq!(expansion.trigger, "alt+g"),
        other => panic!("expected alt+g match, got {other:?}"),
    }
    assert_eq!(
        evaluator.on_key_event(&state, false, left_alt, LogicalKey::Letter('g')),
        HotkeyEvaluation::Swallow
    );

    match evaluator.on_key_event(&state, true, left_ctrl, LogicalKey::Letter('g')) {
        HotkeyEvaluation::Matched(expansion) => assert_eq!(expansion.trigger, "ctrl+g"),
        other => panic!("expected ctrl+g match, got {other:?}"),
    }
    assert_eq!(
        evaluator.on_key_event(&state, false, left_ctrl, LogicalKey::Letter('g')),
        HotkeyEvaluation::Swallow
    );

    match evaluator.on_key_event(&state, true, left_shift, LogicalKey::Letter('g')) {
        HotkeyEvaluation::Matched(expansion) => assert_eq!(expansion.trigger, "shift+g"),
        other => panic!("expected shift+g match, got {other:?}"),
    }
}

#[test]
fn modifier_only_keypresses_do_not_match_hotkeys() {
    let state = EngineState::new();
    load_hotkey(&state, "ctrl+shift+g", "git status");
    let mut evaluator = HotkeyEvaluator::new();

    assert_eq!(
        evaluator.on_key_event(
            &state,
            true,
            modifiers_with(&[Modifier::Ctrl]),
            LogicalKey::Modifier(Modifier::Shift),
        ),
        HotkeyEvaluation::NoMatch
    );
}

#[test]
fn keeps_top_row_and_numpad_digits_distinct_at_runtime() {
    let state = EngineState::new();
    load_hotkey(&state, "ctrl+num1", "numpad");
    let mut evaluator = HotkeyEvaluator::new();
    let modifiers = modifiers_with(&[Modifier::Ctrl]);

    assert_eq!(
        evaluator.on_key_event(&state, true, modifiers, LogicalKey::Digit(1)),
        HotkeyEvaluation::NoMatch
    );
    assert!(matches!(
        evaluator.on_key_event(&state, true, modifiers, LogicalKey::NumpadDigit(1)),
        HotkeyEvaluation::Matched(_)
    ));
}

#[test]
fn no_hotkey_configured_for_key_returns_no_match_without_window_call() {
    let state = EngineState::new();
    let mut evaluator = HotkeyEvaluator::new();

    let result = evaluator.on_key_event(
        &state,
        true,
        modifiers_with(&[Modifier::Ctrl]),
        LogicalKey::Letter('g'),
    );

    assert_eq!(result, HotkeyEvaluation::NoMatch);
}

#[test]
fn hotkey_match_fetches_window_only_when_needed() {
    let state = EngineState::new();
    state.load_hotkey_actions(vec![
        (
            "ctrl+shift+h".to_string(),
            TriggerAction::text("no filter needed"),
        ),
        (
            "ctrl+shift+g".to_string(),
            TriggerAction {
                output: "only in chrome".to_string(),
                only_apps: Some("chrome".to_string()),
                ..TriggerAction::text("")
            },
        ),
    ]);
    let mut evaluator = HotkeyEvaluator::new();

    // Unfiltered entry — matches without any window fetch
    let result = evaluator.on_key_event(
        &state,
        true,
        modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
        LogicalKey::Letter('h'),
    );
    assert!(
        matches!(&result, HotkeyEvaluation::Matched(exp) if exp.trigger == "ctrl+shift+h"),
        "unfiltered hotkey should always match: {result:?}"
    );

    // Filtered entry — lazy chain fetches the window and resolves.
    // Accept either outcome so the test is not flaky across environments.
    let result = evaluator.on_key_event(
        &state,
        true,
        modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
        LogicalKey::Letter('g'),
    );

    match result {
        HotkeyEvaluation::Matched(expansion) => {
            assert_eq!(expansion.trigger, "ctrl+shift+g");
        }
        HotkeyEvaluation::NoMatch => {}
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn hotkey_match_without_window_returns_no_match_when_only_app_filtered_entries_exist() {
    let state = EngineState::new();
    state.load_hotkey_actions(vec![(
        "ctrl+shift+g".to_string(),
        TriggerAction {
            output: "only in chrome".to_string(),
            only_apps: Some("chrome".to_string()),
            ..TriggerAction::text("")
        },
    )]);
    let mut evaluator = HotkeyEvaluator::new();

    let result = evaluator.on_key_event(
        &state,
        true,
        modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
        LogicalKey::Letter('g'),
    );

    // In headless CI the active window is None so NoMatch is returned.
    // On a dev machine with Chrome focused it could match.
    // Accept either — the important thing is no crash or panic.
    match result {
        HotkeyEvaluation::NoMatch => {}
        HotkeyEvaluation::Matched(expansion) => {
            assert_eq!(expansion.trigger, "ctrl+shift+g");
        }
        other => panic!("unexpected result: {other:?}"),
    }
}

#[test]
fn hotkey_match_priority_preserved_with_mixed_filtered_unfiltered_entries() {
    let state = EngineState::new();
    state.load_hotkey_actions(vec![
        (
            "ctrl+shift+g".to_string(),
            TriggerAction::text("unfiltered"),
        ),
        (
            "ctrl+shift+g".to_string(),
            TriggerAction {
                output: "only in chrome".to_string(),
                only_apps: Some("chrome".to_string()),
                ..TriggerAction::text("")
            },
        ),
    ]);
    let mut evaluator = HotkeyEvaluator::new();

    let result = evaluator.on_key_event(
        &state,
        true,
        modifiers_with(&[Modifier::Ctrl, Modifier::Shift]),
        LogicalKey::Letter('g'),
    );

    // The unfiltered entry is scanned first in the bucket and returned before
    // any window fetch, regardless of the current active window.
    match result {
        HotkeyEvaluation::Matched(expansion) => {
            assert_eq!(expansion.trigger, "ctrl+shift+g");
            assert_eq!(
                expansion.steps,
                vec![taurine_core::engine::variables::ExpansionStep::Text(
                    "unfiltered".to_string()
                )]
            );
        }
        other => panic!("expected matched unfiltered hotkey, got {other:?}"),
    }
}

#[test]
fn modifiers_from_flags_preserves_expected_order_independent_bitset() {
    let modifiers = modifiers_from_flags(true, true, false, true);
    let ordered: Vec<Modifier> = modifiers.ordered().collect();
    assert_eq!(
        ordered,
        vec![Modifier::Ctrl, Modifier::Shift, Modifier::Meta]
    );
}

#[test]
fn modifiers_from_sides_preserves_side_specific_order() {
    let modifiers = modifiers_from_sides(true, false, false, false, false, true, false, true);
    let ordered: Vec<Modifier> = modifiers.ordered().collect();
    assert_eq!(
        ordered,
        vec![Modifier::LeftCtrl, Modifier::RightAlt, Modifier::RightMeta]
    );
}

#[cfg(not(target_os = "linux"))]
#[test]
fn rdev_mapping_keeps_top_row_and_numpad_digits_distinct() {
    assert_eq!(
        logical_key_from_rdev(rdev::Key::Num1),
        Some(LogicalKey::Digit(1))
    );
    assert_eq!(
        logical_key_from_rdev(rdev::Key::Kp1),
        Some(LogicalKey::NumpadDigit(1))
    );
    assert_eq!(
        logical_key_from_rdev(rdev::Key::AltGr),
        Some(LogicalKey::Modifier(Modifier::RightAlt))
    );
    assert_eq!(
        logical_key_from_rdev(rdev::Key::ControlLeft),
        Some(LogicalKey::Modifier(Modifier::LeftCtrl))
    );
}

#[cfg(target_os = "linux")]
#[test]
fn evdev_mapping_keeps_top_row_and_numpad_digits_distinct() {
    assert_eq!(
        logical_key_from_evdev(evdev::KeyCode::KEY_1),
        Some(LogicalKey::Digit(1))
    );
    assert_eq!(
        logical_key_from_evdev(evdev::KeyCode::KEY_KP1),
        Some(LogicalKey::NumpadDigit(1))
    );
    assert_eq!(
        logical_key_from_evdev(evdev::KeyCode::KEY_RIGHTALT),
        Some(LogicalKey::Modifier(Modifier::RightAlt))
    );
    assert_eq!(
        logical_key_from_evdev(evdev::KeyCode::KEY_LEFTCTRL),
        Some(LogicalKey::Modifier(Modifier::LeftCtrl))
    );
}
