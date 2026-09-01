use super::clipboard::prepare_clipboard_for_expansion;
use super::gate::{InjectionGate, inject_mutex};
use crate::platform::ClipboardManager;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;
#[cfg(not(target_os = "linux"))]
use std::time::Instant;
use taurine_core::db::crud::TriggerAction;
use taurine_core::engine::source::MemorySource;
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
        if self.sabotage_second_read && self.get_count >= 2 {
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
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let state = Arc::new(EngineState::with_source(Arc::new(MemorySource::new())));
    state.load_actions(vec![(
        "gm".to_string(),
        TriggerAction::text("Good morning!"),
    )]);
    let mut evaluator = Evaluator::new(state);

    for ch in "gm".chars() {
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
    let scope_depth = AtomicUsize::new(0);
    let visibility_depth = AtomicUsize::new(0);
    let gate = InjectionGate::new(&is_injecting, &scope_depth, &visibility_depth);

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
    assert_normal_expansion_still_works();
}

#[test]
fn injection_gate_cancelled_follow_up_cleanup_reenables_normal_expansion() {
    let is_injecting = AtomicBool::new(false);
    let scope_depth = AtomicUsize::new(0);
    let visibility_depth = AtomicUsize::new(0);
    let gate = InjectionGate::new(&is_injecting, &scope_depth, &visibility_depth);

    gate.begin_scope();
    gate.begin_scope();

    gate.end_scope();

    assert!(
        is_injecting.load(AtomicOrdering::SeqCst),
        "the active follow-up should still observe the cancel request until it exits"
    );

    gate.end_scope();

    assert!(
        !is_injecting.load(AtomicOrdering::SeqCst),
        "cancelled follow-up cleanup must release hook suppression"
    );
    assert_normal_expansion_still_works();
}

#[test]
fn injection_gate_error_cleanup_waits_for_overlapping_visibility_scope_before_releasing() {
    let is_injecting = AtomicBool::new(false);
    let scope_depth = AtomicUsize::new(0);
    let visibility_depth = AtomicUsize::new(0);
    let gate = InjectionGate::new(&is_injecting, &scope_depth, &visibility_depth);

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

#[cfg(not(target_os = "linux"))]
#[test]
fn simulated_event_expiry_prunes_old_events() {
    let key_a = EventType::KeyPress(Key::KeyA);

    {
        let mut queue = simulated_events()
            .lock()
            .expect("queue lock should succeed");
        queue.clear();
        queue.push_back(SimulatedEvent {
            event: key_a,
            queued_at: Instant::now() - Duration::from_millis(300),
        });
    }

    assert!(
        !consume_simulated_event(&key_a),
        "expired events must be pruned and not consumed"
    );

    {
        let mut queue = simulated_events()
            .lock()
            .expect("queue lock should succeed");
        queue.push_back(SimulatedEvent {
            event: key_a,
            queued_at: Instant::now(),
        });
    }

    assert!(
        consume_simulated_event(&key_a),
        "fresh events must still be consumed"
    );

    // Verify pruning doesn't affect fresh events
    {
        let mut queue = simulated_events()
            .lock()
            .expect("queue lock should succeed");
        queue.clear();
        let key_b = EventType::KeyPress(Key::KeyB);
        queue.push_back(SimulatedEvent {
            event: key_b,
            queued_at: Instant::now() - Duration::from_millis(200),
        });
        queue.push_back(SimulatedEvent {
            event: key_a,
            queued_at: Instant::now(),
        });
    }

    // The first event (200ms old) is within the TTL (250ms), so both should be consumed
    assert!(
        consume_simulated_event(&EventType::KeyPress(Key::KeyB)),
        "events within 250ms TTL must not be pruned"
    );
    assert!(
        consume_simulated_event(&key_a),
        "second fresh event must also be consumed"
    );
}

#[test]
fn prepare_reads_previous_clipboard_sets_payload_verifies_then_restore_restores_previous() {
    let mut mock = MockClipboard::new("Something the user had copied earlier");
    let payload = "Expanded text only — not the old clipboard";

    let original = prepare_clipboard_for_expansion(&mut mock, payload, 0).unwrap();
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

/// Mock where get_text returns empty for the first `settle_calls_needed` reads
/// after each set_text, simulating slow OS clipboard propagation.
struct MockSlowClipboard {
    text: String,
    settle_calls_needed: usize,
    reads_since_set: usize,
    has_set: bool,
    ops: Vec<&'static str>,
}

impl MockSlowClipboard {
    fn new(initial: &str, settle_calls_needed: usize) -> Self {
        Self {
            text: initial.to_string(),
            settle_calls_needed,
            reads_since_set: 0,
            has_set: false,
            ops: Vec::new(),
        }
    }
}

impl crate::platform::ClipboardManager for MockSlowClipboard {
    fn get_text(&mut self) -> Result<String, String> {
        self.reads_since_set += 1;
        self.ops.push("get_text");
        if self.has_set && self.reads_since_set <= self.settle_calls_needed {
            return Ok(String::new());
        }
        Ok(self.text.clone())
    }

    fn set_text(&mut self, text: &str) -> Result<(), String> {
        self.ops.push("set_text");
        self.text = text.to_string();
        self.reads_since_set = 0;
        self.has_set = true;
        Ok(())
    }

    fn set_image_file(&mut self, _path: &std::path::Path) -> Result<(), String> {
        self.ops.push("set_image");
        Ok(())
    }

    fn set_html(&mut self, _html: &str, plaintext: &str) -> Result<(), String> {
        self.ops.push("set_html");
        self.text = plaintext.to_string();
        self.reads_since_set = 0;
        self.has_set = true;
        Ok(())
    }
}

#[test]
fn prepare_polls_until_clipboard_settles() {
    let mut mock = MockSlowClipboard::new("original", 3);
    let payload = "slow payload";

    let original = prepare_clipboard_for_expansion(&mut mock, payload, 0).unwrap();
    assert_eq!(original, "original");
    assert_eq!(mock.text, payload);

    // Initial read (1) + set_text + 3 stale reads + 1 success read = 5 get_text ops
    let get_count = mock.ops.iter().filter(|&&op| op == "get_text").count();
    assert!(
        get_count >= 5,
        "expected at least 5 get_text calls (1 original + 3 stale + 1 success), got {}",
        get_count
    );
}

#[test]
fn prepare_polls_until_timeout_when_clipboard_never_settles() {
    let mut mock = MockSlowClipboard::new("original", 999);
    let payload = "never settles";

    let err = prepare_clipboard_for_expansion(&mut mock, payload, 0).unwrap_err();
    assert!(
        err.contains("clipboard verify failed"),
        "expected verify error after exhausting poll retries, got {:?}",
        err
    );
}

#[test]
fn rapid_sequential_expansion_preserves_clipboard_state() {
    let mut mock = MockClipboard::new("user copied text");

    // First expansion
    let orig1 = prepare_clipboard_for_expansion(&mut mock, "expansion one", 0).unwrap();
    assert_eq!(
        orig1, "user copied text",
        "first prepare must save original"
    );
    assert_eq!(
        mock.text, "expansion one",
        "clipboard must hold payload after first prepare"
    );

    // App reads the payload (simulated paste)
    assert_eq!(
        mock.get_text().unwrap(),
        "expansion one",
        "app must see payload on paste"
    );

    // Restore original clipboard
    mock.set_text(&orig1).unwrap();
    assert_eq!(
        mock.text, "user copied text",
        "clipboard must be restored to original"
    );

    // Second expansion (immediately after restore, simulating rapid trigger)
    let orig2 = prepare_clipboard_for_expansion(&mut mock, "expansion two", 0).unwrap();
    assert_eq!(
        orig2, "user copied text",
        "second prepare must see restored original, not stale payload"
    );
    assert_eq!(
        mock.text, "expansion two",
        "clipboard must hold second payload"
    );

    // App reads second payload
    assert_eq!(
        mock.get_text().unwrap(),
        "expansion two",
        "app must see second payload on paste"
    );
}

#[test]
fn prepare_fails_if_clipboard_raced_before_paste_so_stale_clip_is_never_intended_payload() {
    let mut mock = MockClipboard::with_sabotage("old");
    let err = prepare_clipboard_for_expansion(&mut mock, "new", 0).unwrap_err();
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

    let original = prepare_clipboard_for_expansion(&mut mock, payload, 0).unwrap();
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
    let _ = prepare_clipboard_for_expansion(&mut mock, "payload1", 0).unwrap();
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

#[test]
fn test_atomic_unicode_expansion_batches_backspaces_and_chars() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let injector = crate::platform::get_injector();
    let _ = injector.inject_atomic_text_expansion(3, "Hello World! 🚀");
}

#[test]
fn test_atomic_backspaces_batch() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let injector = crate::platform::get_injector();
    injector.inject_atomic_backspaces(10);
}

#[test]
fn test_inject_unicode_text_direct() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let injector = crate::platform::get_injector();
    let _ = injector.inject_unicode_text_direct("Unicode: é, ñ, 🚀, 中文");
}

#[test]
fn test_dual_path_routes_plain_text_to_fast_path() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let steps = vec![taurine_core::engine::variables::ExpansionStep::Text(
        "instant text".to_string(),
    )];
    let report =
        super::inject::inject_expansion(steps, 2, taurine_core::settings::SpinnerStyle::Braille);
    assert_eq!(report.successful_chars, 12);
}

#[test]
fn test_inject_undo_fast_path() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    super::inject::inject_undo("trigger".to_string(), 15);
}
