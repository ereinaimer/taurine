use super::clipboard::prepare_clipboard_for_expansion;
use super::gate::{
    INJECTION_ABORT, INJECTION_SCOPE_DEPTH, INJECTION_VISIBILITY_DEPTH, IS_INJECTING,
    InjectionGate, inject_mutex,
};
use super::inject::{InjectionReport, inject_expansion, inject_text_segment};
use crate::platform::ClipboardManager;
use crate::platform::MouseButton;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use taurine_core::db::crud::AutomationAction;
use taurine_core::engine::variables::ExpansionStep;
use taurine_core::engine::{EngineEvent, EngineState, Evaluator};

#[cfg(not(target_os = "linux"))]
use super::simulate::{SimulatedEvent, consume_simulated_event, simulated_events};
#[cfg(not(target_os = "linux"))]
use rdev::{EventType, Key};

/// Mock clipboard: records operations and supports simulating a race where post-write read
/// does not match the payload (verify failure).
struct MockClipboard {
    text: String,
    /// Number of `get_text` calls so far (used to sabotage the verify read).
    get_count: usize,
    /// If true, the second `get_text` returns stale content (another writer "won" the race).
    sabotage_second_read: bool,
    ops: Vec<&'static str>,
}

impl MockClipboard {
    fn new(initial: &str) -> Self {
        Self {
            text: initial.to_string(),
            get_count: 0,
            sabotage_second_read: false,
            ops: Vec::new(),
        }
    }

    fn with_sabotage(initial: &str) -> Self {
        Self {
            text: initial.to_string(),
            get_count: 0,
            sabotage_second_read: true,
            ops: Vec::new(),
        }
    }
}

impl crate::platform::ClipboardManager for MockClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        self.get_count += 1;
        self.ops.push("get_text");
        if self.sabotage_second_read && self.get_count == 2 {
            // Verify read (after set_text): another writer won the race.
            return Ok("STALE_FROM_ANOTHER_PROCESS".to_string());
        }
        Ok(self.text.clone())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.ops.push("set_text");
        self.text = text.to_string();
        Ok(())
    }

    fn set_image_file(&mut self, _path: &std::path::Path) -> Result<(), String> {
        self.ops.push("set_image");
        Ok(())
    }

    fn set_html(&mut self, _html: &str, plaintext: &str) -> Result<(), String> {
        self.ops.push("set_html");
        self.text = plaintext.to_string();
        Ok(())
    }
}

fn assert_normal_expansion_still_works() {
    let state = Arc::new(EngineState::new('>'));
    state.load_actions(vec![(
        "gm".to_string(),
        AutomationAction::text("Good morning!"),
    )]);
    let mut evaluator = Evaluator::new(state);

    for ch in ">gm".chars() {
        assert_eq!(
            evaluator.process_event(
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

    let expansion = evaluator
        .process_event(EngineEvent::ActionKey, None)
        .expect("word trigger should still expand after AI cleanup");
    assert_eq!(
        expansion.steps,
        vec![ExpansionStep::Text("Good morning!".to_string())]
    );
}

#[test]
fn injection_gate_successful_follow_up_cleanup_reenables_normal_expansion() {
    let is_injecting = AtomicBool::new(false);
    let abort = AtomicBool::new(false);
    let scope_depth = AtomicUsize::new(0);
    let visibility_depth = AtomicUsize::new(0);
    let gate = InjectionGate::new(&is_injecting, &abort, &scope_depth, &visibility_depth);

    gate.begin_scope();
    gate.begin_scope();
    gate.end_scope();

    assert!(
        is_injecting.load(AtomicOrdering::SeqCst),
        "the follow-up scope must stay active after the dispatch scope exits"
    );

    gate.end_scope();

    assert!(
        !is_injecting.load(AtomicOrdering::SeqCst),
        "successful follow-up cleanup must release hook suppression"
    );
    assert!(
        !abort.load(AtomicOrdering::SeqCst),
        "successful cleanup must leave no stale abort signal behind"
    );
    assert_normal_expansion_still_works();
}

#[test]
fn injection_gate_cancelled_follow_up_cleanup_clears_abort_and_reenables_normal_expansion() {
    let is_injecting = AtomicBool::new(false);
    let abort = AtomicBool::new(false);
    let scope_depth = AtomicUsize::new(0);
    let visibility_depth = AtomicUsize::new(0);
    let gate = InjectionGate::new(&is_injecting, &abort, &scope_depth, &visibility_depth);

    gate.begin_scope();
    gate.begin_scope();
    abort.store(true, AtomicOrdering::SeqCst);

    gate.end_scope();
    assert!(
        abort.load(AtomicOrdering::SeqCst),
        "the active follow-up should still observe the cancel request until it exits"
    );

    gate.end_scope();

    assert!(
        !is_injecting.load(AtomicOrdering::SeqCst),
        "cancelled follow-up cleanup must release hook suppression"
    );
    assert!(
        !abort.load(AtomicOrdering::SeqCst),
        "cancelled cleanup must clear the abort signal for later triggers"
    );
    assert_normal_expansion_still_works();
}

#[test]
fn injection_gate_error_cleanup_waits_for_overlapping_visibility_scope_before_releasing() {
    let is_injecting = AtomicBool::new(false);
    let abort = AtomicBool::new(false);
    let scope_depth = AtomicUsize::new(0);
    let visibility_depth = AtomicUsize::new(0);
    let gate = InjectionGate::new(&is_injecting, &abort, &scope_depth, &visibility_depth);

    gate.begin_scope();
    gate.begin_scope();
    gate.begin_visibility();

    gate.end_scope();
    gate.end_scope();

    assert!(
        is_injecting.load(AtomicOrdering::SeqCst),
        "an overlapping spinner or UI-injection frame must keep suppression active until it finishes"
    );

    gate.end_visibility();

    assert!(
        !is_injecting.load(AtomicOrdering::SeqCst),
        "error cleanup must fully release suppression after the last overlapping scope ends"
    );
    assert!(
        !abort.load(AtomicOrdering::SeqCst),
        "error cleanup must not leak abort state into future trigger handling"
    );
    assert_normal_expansion_still_works();
}

#[cfg(not(target_os = "linux"))]
#[test]
fn simulated_event_filter_consumes_only_the_expected_event() {
    let key_a_press = EventType::KeyPress(Key::KeyA);
    let key_b_press = EventType::KeyPress(Key::KeyB);

    {
        let mut queue = simulated_events()
            .lock()
            .expect("queue lock should succeed");
        queue.clear();
        queue.push_back(SimulatedEvent {
            event: key_a_press,
            queued_at: Instant::now(),
        });
    }

    assert!(
        !consume_simulated_event(&key_b_press),
        "different physical events must not be swallowed as synthetic"
    );
    assert!(
        consume_simulated_event(&key_a_press),
        "the exact synthetic event should be swallowed"
    );
    assert!(
        !consume_simulated_event(&key_a_press),
        "a consumed synthetic event should not match twice"
    );
}

#[test]
fn prepare_reads_previous_clipboard_sets_payload_verifies_then_restore_restores_previous() {
    let mut mock = MockClipboard::new("Something the user had copied earlier");
    let payload = "Expanded text only — not the old clipboard";

    let original = prepare_clipboard_for_expansion(&mut mock, payload).unwrap();
    assert_eq!(original, "Something the user had copied earlier");
    assert_eq!(
        mock.text, payload,
        "clipboard must hold payload until after paste+restore"
    );

    // Simulated app would read this value on Ctrl+V — not the pre-expansion clipboard.
    assert_eq!(mock.get_text().unwrap(), payload);

    mock.set_text(&original).unwrap();
    assert_eq!(
        mock.text, "Something the user had copied earlier",
        "after restore, user must see their original clip, not the expansion"
    );
}

#[test]
fn prepare_fails_if_clipboard_raced_before_paste_so_stale_clip_is_never_intended_payload() {
    let mut mock = MockClipboard::with_sabotage("old");
    let err = prepare_clipboard_for_expansion(&mut mock, "new").unwrap_err();
    assert!(
        err.contains("clipboard verify failed"),
        "expected verify error, got {:?}",
        err
    );
}

#[test]
fn prepare_uses_html_and_verifies_with_plaintext() {
    let mut mock = MockClipboard::new("old clipboard");
    let payload = "<b>Hello</b><br>World";

    let original = prepare_clipboard_for_expansion(&mut mock, payload).unwrap();
    assert_eq!(original, "old clipboard");

    // Check that set_html was called by verifying the MockClipboard text contains the stripped fallback
    assert_eq!(mock.text, "Hello\nWorld");
    assert!(mock.ops.contains(&"set_html"));
}

#[test]
fn inject_mutex_serializes_overlapping_injections_no_interleaved_critical_sections() {
    let depth = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let depth = depth.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                let _guard = inject_mutex().lock().expect("mutex");
                assert_eq!(depth.fetch_add(1, AtomicOrdering::SeqCst), 0);
                thread::sleep(Duration::from_millis(40));
                depth.fetch_sub(1, AtomicOrdering::SeqCst);
            })
        })
        .collect();

    barrier.wait();
    for h in handles {
        h.join().unwrap();
    }
}

#[test]
fn mock_clipboard_operation_order_matches_protocol() {
    let mut mock = MockClipboard::new("clip0");
    let _ = prepare_clipboard_for_expansion(&mut mock, "payload1").unwrap();
    assert_eq!(
        mock.ops,
        vec!["get_text", "set_text", "get_text"],
        "must read original, write payload, read back to verify before any paste"
    );
}

#[cfg(target_os = "linux")]
fn resolve_alias(alias: &str) -> bool {
    crate::platform::linux::injector::alias_to_evdev_key(alias).is_some()
}
#[cfg(not(target_os = "linux"))]
fn resolve_alias(alias: &str) -> bool {
    crate::platform::rdev_injector::alias_to_rdev_key(alias).is_some()
}

#[cfg(target_os = "linux")]
fn resolve_modifier(alias: &str) -> bool {
    crate::platform::linux::injector::modifier_alias_to_evdev_key(alias).is_some()
}
#[cfg(not(target_os = "linux"))]
fn resolve_modifier(alias: &str) -> bool {
    crate::platform::rdev_injector::modifier_alias_to_rdev_key(alias).is_some()
}

#[test]
fn alias_resolves_alphabet_keys() {
    for letter in 'a'..='z' {
        let alias = letter.to_string();
        assert!(resolve_alias(&alias), "missing alias for key '{}'", alias);
    }
}

#[test]
fn alias_resolves_number_keys() {
    for digit in '0'..='9' {
        let alias = digit.to_string();
        assert!(resolve_alias(&alias), "missing alias for key '{}'", alias);
    }
}

#[test]
fn alias_resolves_function_keys() {
    for n in 1..=12 {
        let alias = format!("f{}", n);
        assert!(resolve_alias(&alias), "missing alias for key '{}'", alias);
    }
}

#[test]
fn alias_resolves_special_characters() {
    let specials = [
        "backtick",
        "grave",
        "tilde",
        "minus",
        "dash",
        "equal",
        "equals",
        "backslash",
        "semicolon",
        "quote",
        "apostrophe",
        "comma",
        "dot",
        "period",
        "slash",
        "lbracket",
        "rbracket",
        "capslock",
        "numlock",
        "scrolllock",
        "printscreen",
        "prtsc",
        "pause",
        "break",
        "insert",
        "ins",
    ];
    for alias in &specials {
        assert!(
            resolve_alias(alias),
            "missing alias for special key '{}'",
            alias
        );
    }
}

#[test]
fn modifier_alias_resolves_all_variants() {
    let modifiers = [
        "ctrl",
        "control",
        "lctrl",
        "leftctrl",
        "leftcontrol",
        "rctrl",
        "rightctrl",
        "rightcontrol",
        "alt",
        "lalt",
        "leftalt",
        "leftoption",
        "ralt",
        "rightalt",
        "rightoption",
        "altgr",
        "shift",
        "lshift",
        "leftshift",
        "rshift",
        "rightshift",
        "win",
        "mod",
        "super",
        "meta",
        "lmeta",
        "leftmeta",
        "leftwin",
        "leftsuper",
        "leftcmd",
        "leftcommand",
        "rmeta",
        "rightmeta",
        "rightwin",
        "rightsuper",
        "rightcmd",
        "rightcommand",
        "cmd",
        "command",
        "opt",
        "option",
    ];
    for alias in &modifiers {
        assert!(
            resolve_modifier(alias),
            "missing modifier alias '{}'",
            alias
        );
    }
}

#[test]
fn modifier_alias_rejects_unknown() {
    assert!(!resolve_modifier("hyper"));
    assert!(!resolve_modifier("fn"));
}

#[test]
fn alias_resolves_standalone_modifiers() {
    assert!(resolve_alias("ctrl"));
    assert!(resolve_alias("shift"));
    assert!(resolve_alias("alt"));
    assert!(resolve_alias("win"));
}

#[test]
fn alias_rejects_unknown_keys() {
    assert!(!resolve_alias("unknown_key"));
    assert!(!resolve_alias("hyper"));
}
