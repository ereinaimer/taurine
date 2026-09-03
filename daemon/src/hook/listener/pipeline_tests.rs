/// Tests that directly exercise the `process_keyboard_event` pipeline without a live daemon.
///
/// These tests are the critical regression net that the resilience-only stress test
/// (`hook_stress.rs`) was missing. The stress test only verifies that the hook
/// *stays alive* — it never checks whether the expansion engine produces the correct
/// output or swallows the right keys. These tests fill that gap by:
///
/// 1. Constructing a real `Evaluator` with seeded triggers
/// 2. Driving `process_keyboard_event` directly (no OS hook required)
/// 3. Asserting the exact return value (None = swallowed, Some = pass-through)
/// 4. Asserting the evaluator's `ExpansionResult` is correct
///
/// Run with: `cargo test --lib`
#[cfg(test)]
#[cfg(windows)]
mod listener_pipeline_tests {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;

    use rdev::{Event, EventType, Key};
    use taurine_core::engine::variables::ExpansionStep;
    use taurine_core::engine::{EngineEvent, EngineState, Evaluator};

    use crate::hook::listener::process_keyboard_event;

    // ─── Helpers ─────────────────────────────────────────────────────────────

    fn bare_event(et: EventType) -> Event {
        Event {
            event_type: et,
            time: SystemTime::now(),
            name: None,
        }
    }

    fn named_event(et: EventType, name: &str) -> Event {
        Event {
            event_type: et,
            time: SystemTime::now(),
            name: Some(name.to_string()),
        }
    }

    struct Harness {
        evaluator: Arc<Mutex<Evaluator>>,
        state: Arc<EngineState>,
        paused: Arc<AtomicBool>,
        pause_hotkey: Arc<std::sync::RwLock<crate::input::hotkey::HotkeySpec>>,
        spinner_style: Arc<std::sync::RwLock<taurine_core::settings::SpinnerStyle>>,
        pause_tx: tokio::sync::mpsc::Sender<bool>,
        l_alt: AtomicBool,
        r_alt: AtomicBool,
        l_ctrl: AtomicBool,
        r_ctrl: AtomicBool,
        l_shift: AtomicBool,
        r_shift: AtomicBool,
        l_meta: AtomicBool,
        r_meta: AtomicBool,
        hk_eval: Arc<Mutex<crate::input::hotkey_evaluator::HotkeyEvaluator>>,
        counter: Arc<AtomicU32>,
    }

    impl Harness {
        fn new() -> Self {
            let state = Arc::new(EngineState::new());
            let (tx, _rx) = tokio::sync::mpsc::channel(8);
            // Use a well-known hotkey string that will always parse correctly.
            let pause_hotkey_spec = crate::input::hotkey::parse_pause_hotkey_setting("alt+`")
                .expect("default pause hotkey must parse");
            Self {
                evaluator: Arc::new(Mutex::new(Evaluator::new(state.clone()))),
                state,
                paused: Arc::new(AtomicBool::new(false)),
                pause_hotkey: Arc::new(std::sync::RwLock::new(pause_hotkey_spec)),
                spinner_style: Arc::new(std::sync::RwLock::new(Default::default())),
                pause_tx: tx,
                l_alt: AtomicBool::new(false),
                r_alt: AtomicBool::new(false),
                l_ctrl: AtomicBool::new(false),
                r_ctrl: AtomicBool::new(false),
                l_shift: AtomicBool::new(false),
                r_shift: AtomicBool::new(false),
                l_meta: AtomicBool::new(false),
                r_meta: AtomicBool::new(false),
                hk_eval: Arc::new(Mutex::new(
                    crate::input::hotkey_evaluator::HotkeyEvaluator::new(),
                )),
                counter: Arc::new(AtomicU32::new(0)),
            }
        }

        fn with_trigger(self, trigger: &str, output: &str) -> Self {
            self.state.load_actions(vec![(
                trigger.to_string(),
                taurine_core::db::crud::TriggerAction::text(output),
            )]);
            self
        }

        fn with_hotkey_trigger(self, trigger: &str, output: &str) -> Self {
            let action = taurine_core::db::crud::TriggerAction::text(output);
            self.state
                .load_hotkey_actions(vec![(trigger.to_string(), action)]);
            self
        }

        fn send(&self, ev: Event) -> Option<Event> {
            process_keyboard_event(
                ev,
                &self.evaluator,
                &self.state,
                &self.paused,
                &self.pause_hotkey,
                &self.spinner_style,
                &self.pause_tx,
                &self.l_alt,
                &self.r_alt,
                &self.l_ctrl,
                &self.r_ctrl,
                &self.l_shift,
                &self.r_shift,
                &self.l_meta,
                &self.r_meta,
                &self.hk_eval,
                &self.counter,
            )
        }

        fn type_char(&self, c: char) -> Option<Event> {
            self.send(named_event(
                EventType::KeyPress(char_key(c)),
                &c.to_string(),
            ))
        }

        fn type_str(&self, s: &str) {
            for c in s.chars() {
                self.type_char(c);
            }
        }

        fn enter(&self) -> Option<Event> {
            self.send(bare_event(EventType::KeyPress(Key::Return)))
        }

        fn backspace(&self) -> Option<Event> {
            self.send(bare_event(EventType::KeyPress(Key::Backspace)))
        }

        fn escape(&self) -> Option<Event> {
            self.send(bare_event(EventType::KeyPress(Key::Escape)))
        }

        fn buf(&self) -> String {
            self.evaluator
                .lock()
                .unwrap()
                .buffer
                .buffer_string()
                .to_string()
        }

        fn action_key_result(&self) -> Option<taurine_core::engine::ExpansionResult> {
            self.evaluator
                .lock()
                .unwrap()
                .process_event(EngineEvent::ActionKey, None)
        }
    }

    fn char_key(c: char) -> Key {
        match c {
            'a' | 'A' => Key::KeyA,
            'b' | 'B' => Key::KeyB,
            'c' | 'C' => Key::KeyC,
            'd' | 'D' => Key::KeyD,
            'e' | 'E' => Key::KeyE,
            'f' | 'F' => Key::KeyF,
            'g' | 'G' => Key::KeyG,
            'h' | 'H' => Key::KeyH,
            'i' | 'I' => Key::KeyI,
            'j' | 'J' => Key::KeyJ,
            'k' | 'K' => Key::KeyK,
            'l' | 'L' => Key::KeyL,
            'm' | 'M' => Key::KeyM,
            'n' | 'N' => Key::KeyN,
            'o' | 'O' => Key::KeyO,
            'p' | 'P' => Key::KeyP,
            'q' | 'Q' => Key::KeyQ,
            'r' | 'R' => Key::KeyR,
            's' | 'S' => Key::KeyS,
            't' | 'T' => Key::KeyT,
            'u' | 'U' => Key::KeyU,
            'v' | 'V' => Key::KeyV,
            'w' | 'W' => Key::KeyW,
            'x' | 'X' => Key::KeyX,
            'y' | 'Y' => Key::KeyY,
            'z' | 'Z' => Key::KeyZ,
            ' ' => Key::Space,
            _ => Key::Unknown(c as u32),
        }
    }

    struct TestGuard {
        _mutex_guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TestGuard {
        fn acquire() -> Self {
            let mutex_guard = taurine_core::testing::TEST_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());

            let start = std::time::Instant::now();
            while crate::injector::IS_INJECTING.load(std::sync::atomic::Ordering::SeqCst) {
                if start.elapsed() > std::time::Duration::from_millis(500) {
                    crate::injector::IS_INJECTING.store(false, std::sync::atomic::Ordering::SeqCst);
                    break;
                }
                std::thread::yield_now();
            }

            crate::injector::clear_simulated_events_for_test();

            Self {
                _mutex_guard: mutex_guard,
            }
        }
    }

    // ─── Tests ────────────────────────────────────────────────────────────────

    /// Core invariant: when a trigger is typed and Enter is pressed, the hook
    /// must swallow Enter (return None). If Enter is not swallowed, an extra
    /// newline reaches the target application and the expansion is broken.
    #[test]
    fn expansion_enter_is_swallowed_on_trigger_match() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_trigger("gm", "Good morning!");

        for c in "gm".chars() {
            let r = h.type_char(c);
            assert!(
                r.is_some(),
                "char '{c}' must pass through before trigger fires"
            );
        }
        assert_eq!(h.buf(), "gm");

        let enter = h.enter();
        assert!(
            enter.is_none(),
            "Enter MUST be swallowed when trigger 'gm' matches — \
             got Some (pass-through), meaning the expansion pipeline failed to intercept it"
        );
    }

    /// When no trigger matches, Enter passes through to the OS so the user's
    /// normal newline behaviour is preserved.
    #[test]
    fn no_match_enter_passes_through() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_trigger("gm", "Good morning!");
        h.type_str("xyz");
        assert!(
            h.enter().is_some(),
            "Enter must pass through to OS when no trigger matches"
        );
    }

    /// The evaluator accumulates typed characters correctly into its buffer.
    #[test]
    fn characters_accumulate_in_buffer() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();
        h.type_str("hello");
        assert_eq!(h.buf(), "hello");
    }

    /// Backspace removes the last char from the buffer and passes through so
    /// the target application also sees the deletion.
    #[test]
    fn backspace_removes_last_char_and_passes_through() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();
        h.type_str("abc");
        let r = h.backspace();
        assert!(r.is_some(), "Backspace must pass through to the OS");
        assert_eq!(h.buf(), "ab");
    }

    /// Escape interrupts the current sequence, clears the buffer, and still
    /// passes through so applications receive Escape normally.
    #[test]
    fn escape_clears_buffer_and_passes_through() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();
        h.type_str("hello");
        let r = h.escape();
        assert!(r.is_some(), "Escape must pass through to the OS");
        assert_eq!(h.buf(), "");
    }

    /// A solo modifier key (e.g. Shift alone) must pass through without
    /// disturbing the typed sequence in the buffer.
    #[test]
    fn solo_modifier_passes_through_without_disrupting_buffer() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();
        h.type_str("ab");
        let r = h.send(bare_event(EventType::KeyPress(Key::ShiftLeft)));
        assert!(r.is_some(), "Solo Shift must pass through to the OS");
        assert_eq!(
            h.buf(),
            "ab",
            "Buffer must not be disturbed by a solo Shift press"
        );
    }

    /// While Taurine is paused, every key event must pass through to the OS
    /// unchanged, and the evaluator buffer must stay empty.
    #[test]
    fn paused_passes_all_events_through_without_buffering() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_trigger("gm", "Good morning!");
        h.paused.store(true, Ordering::Relaxed);

        h.type_str("gm");
        let r = h.enter();

        assert!(r.is_some(), "Enter must pass through to OS while paused");
        assert_eq!(h.buf(), "", "Buffer must be empty while paused");
    }

    /// The `consume_simulated_event` function correctly identifies enqueued events.
    ///
    /// This verifies the contract that `process_keyboard_event` relies on:
    /// when an event is in the simulated queue, `consume_simulated_event` must
    /// return `true` for that event type, removing it from the queue.
    ///
    /// Integration note: Full end-to-end verification (event filtered in the pipeline)
    /// requires serial execution due to the shared global queue. Run with:
    ///   cargo test ... -- --test-threads=1
    #[test]
    fn simulated_event_consume_contract() {
        use rdev::Key;
        let _guard = TestGuard::acquire();

        // Nothing in queue — must return false
        assert!(
            !crate::injector::consume_simulated_event(&EventType::KeyPress(Key::Return)),
            "Empty queue must not consume any event"
        );

        // Enqueue a Return event
        crate::injector::enqueue_simulated_event_for_test(EventType::KeyPress(Key::Return));

        // Now it must be consumed
        assert!(
            crate::injector::consume_simulated_event(&EventType::KeyPress(Key::Return)),
            "Enqueued Return must be consumable"
        );

        // After consumption, queue is empty again
        assert!(
            !crate::injector::consume_simulated_event(&EventType::KeyPress(Key::Return)),
            "Queue must be empty after consumption"
        );
    }

    /// When `consume_simulated_event` returns true for an incoming event,
    /// `process_keyboard_event` must pass it through (not swallow it).
    ///
    /// This test serialises against other pipeline tests using TEST_LOCK to
    /// prevent a parallel test from consuming the enqueued event first.
    /// Run with `--test-threads=1` if flakiness is observed.
    #[test]
    fn simulated_event_pipeline_passes_through() {
        let _guard = TestGuard::acquire();

        let h = Harness::new().with_trigger("gm", "Good morning!");
        h.type_str("gm");

        // Clear any leftover entries, then enqueue our specific event.
        // We do this AFTER Harness construction and typing to avoid the simulated event
        // expiring (TTL of 250ms) before the pipeline process is called.
        crate::injector::clear_simulated_events_for_test();
        crate::injector::enqueue_simulated_event_for_test(EventType::KeyPress(Key::Return));

        let r = h.send(bare_event(EventType::KeyPress(Key::Return)));
        assert!(
            r.is_some(),
            "A simulated Return must pass through, not be swallowed — \
             otherwise the injector's own output would re-trigger the evaluator"
        );
    }

    /// The evaluator produces the correct `ExpansionResult` — right trigger name,
    /// right delete count, and right output text.
    #[test]
    fn evaluator_expansion_result_is_correct() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_trigger("shrug", r#"¯\_(ツ)_/¯"#);

        let mut lock = h.evaluator.lock().unwrap();
        for c in "shrug".chars() {
            lock.process_event(EngineEvent::Char(c), None);
        }
        let result = lock.process_event(EngineEvent::ActionKey, None);
        drop(lock);

        let exp = result.expect("Expected ExpansionResult for trigger 'shrug'");
        assert_eq!(exp.trigger, "shrug");
        assert_eq!(exp.delete_count, 5, "Must delete exactly 5 trigger chars");

        let text = exp.steps.iter().find_map(|s| {
            if let ExpansionStep::Text(t) = s {
                Some(t.as_str())
            } else {
                None
            }
        });
        assert_eq!(
            text,
            Some(r#"¯\_(ツ)_/¯"#),
            "Output text must exactly match the registered expansion"
        );
    }

    /// A partial trigger must NOT expand.
    #[test]
    fn partial_trigger_does_not_expand() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_trigger("hello", "world");
        h.type_str("hel");
        assert!(
            h.action_key_result().is_none(),
            "Partial trigger 'hel' must not expand 'hello'"
        );
    }

    /// Trigger + extra char before Enter must NOT expand.
    #[test]
    fn trigger_plus_extra_char_does_not_expand() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_trigger("gm", "Good morning!");
        let mut lock = h.evaluator.lock().unwrap();
        for c in "gm!".chars() {
            lock.process_event(EngineEvent::Char(c), None);
        }
        let result = lock.process_event(EngineEvent::ActionKey, None);
        drop(lock);
        assert!(result.is_none(), "'gm!' must not match trigger 'gm'");
    }

    /// Typing a wrong char, backspacing to fix it, then completing the trigger
    /// must still expand. This is the exact regression the async channel model
    /// broke: simulated backspace events during erase_trigger would expire before
    /// being matched, desynchronising the buffer.
    #[test]
    fn trigger_after_mid_word_backspace_correction_still_expands() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_trigger("gm", "Good morning!");

        // Type "gx" (wrong), backspace to erase 'x', then type 'm'
        h.type_char('g');
        h.type_char('x');
        h.backspace(); // buffer → "g"
        assert_eq!(h.buf(), "g");

        h.type_char('m'); // buffer → "gm"
        assert_eq!(h.buf(), "gm");

        assert!(
            h.enter().is_none(),
            "Enter must be swallowed after correcting typo to complete trigger 'gm'"
        );
    }

    /// Two consecutive expansions must be independent with no state leakage.
    ///
    /// In the real daemon the dispatcher calls `evaluator.reset()` after dispatching
    /// the expansion. We replicate that here so the second expansion starts clean.
    #[test]
    fn consecutive_expansions_are_independent() {
        let _guard = TestGuard::acquire();
        // NOTE: load_actions *replaces* all triggers, so both must be loaded together.
        let h = Harness::new();
        h.state.load_actions(vec![
            (
                "gm".to_string(),
                taurine_core::db::crud::TriggerAction::text("Good morning!"),
            ),
            (
                "ty".to_string(),
                taurine_core::db::crud::TriggerAction::text("Thank you!"),
            ),
        ]);

        {
            let mut lock = h.evaluator.lock().unwrap();
            for c in "gm".chars() {
                lock.process_event(EngineEvent::Char(c), None);
            }
            let r1 = lock
                .process_event(EngineEvent::ActionKey, None)
                .expect("First expansion 'gm' must succeed");
            assert_eq!(r1.trigger, "gm");
            // Mirror what the real dispatcher does after a successful expansion.
            lock.reset();
        }

        {
            let mut lock = h.evaluator.lock().unwrap();
            for c in "ty".chars() {
                lock.process_event(EngineEvent::Char(c), None);
            }
            let r2 = lock
                .process_event(EngineEvent::ActionKey, None)
                .expect("Second expansion 'ty' must succeed after 'gm' ran");
            assert_eq!(r2.trigger, "ty");
        }
    }

    /// When a modifier key release is received by the pipeline, it clears the active modifier state.
    #[test]
    fn modifier_release_clears_active_modifier_state() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();
        h.type_char('a');
        assert_eq!(h.buf(), "a");

        // Press Left Alt
        h.send(bare_event(EventType::KeyPress(Key::Alt)));
        assert!(
            h.l_alt.load(Ordering::Relaxed),
            "Left Alt must be active on KeyPress"
        );

        // Release Left Alt
        h.send(bare_event(EventType::KeyRelease(Key::Alt)));
        assert!(
            !h.l_alt.load(Ordering::Relaxed),
            "Left Alt must be inactive after KeyRelease"
        );

        // Type 'i'
        let res = h.type_char('i');
        assert!(
            res.is_some(),
            "Character 'i' should pass through and be processed"
        );
        assert_eq!(h.buf(), "ai", "Buffer must contain 'ai'");
    }

    /// When a mouse button is pressed, stale modifier states are flushed and
    /// any pending buffer evaluation is safely interrupted.
    #[test]
    fn mouse_button_press_clears_stale_modifiers_and_interrupts_buffer() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();
        h.type_str("abc");
        assert_eq!(h.buf(), "abc");

        h.l_alt.store(true, Ordering::Relaxed);
        let r = h.send(bare_event(EventType::ButtonPress(rdev::Button::Left)));
        assert!(r.is_some(), "ButtonPress must pass through to OS");
        assert_eq!(
            h.buf(),
            "",
            "Buffer must be cleared on ButtonPress interrupt"
        );
        assert!(
            !h.l_alt.load(Ordering::Relaxed),
            "Left Alt must be resynchronized to false on ButtonPress"
        );
    }

    #[test]
    fn test_hook_callback_drops_event_cleanly_if_evaluator_lock_held_too_long() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();

        // Artificially hold the lock in another thread / guard
        let eval_clone = h.evaluator.clone();
        let lock_guard = eval_clone.lock().unwrap();

        // Process event should gracefully pass through rather than blocking past OS timeout
        let event = named_event(EventType::KeyPress(rdev::Key::KeyA), "a");
        let result = h.send(event);

        drop(lock_guard);
        assert!(
            result.is_some(),
            "Keystroke must pass through to OS if engine is temporarily locked"
        );
    }

    /// Fast typing or short-dwell key combinations (like `Alt + G` or `Alt + M`) must reliably
    /// match hotkeys without having their modifier states erroneously wiped out by asynchronous
    /// hardware polling during the base keypress event.
    #[test]
    fn rapid_alt_hotkey_matches_reliably_without_physical_modifier_interference() {
        let _guard = TestGuard::acquire();
        let h = Harness::new().with_hotkey_trigger("alt+g", "gemini_script_output");

        // 1. Press Alt (sets left Alt in memory)
        let r_alt = h.send(bare_event(EventType::KeyPress(Key::Alt)));
        assert!(r_alt.is_some(), "Alt key press should pass through to OS");
        assert!(h.l_alt.load(Ordering::Relaxed), "Left Alt must be active");

        // 2. Press G while Alt is active
        let r_g = h.send(named_event(EventType::KeyPress(Key::KeyG), "g"));
        assert!(
            r_g.is_none(),
            "Alt+G hotkey must match and be swallowed, even in headless / fast typing environments"
        );
    }

    /// Mouse hotkey triggers (e.g. `ctrl+mouse1` and `ralt+mouse4`) must match,
    /// be swallowed on press, swallow the subsequent release, and leave subsequent releases unswallowed.
    #[test]
    fn mouse_hotkey_trigger_matches_and_swallows_press_and_release() {
        let _guard = TestGuard::acquire();
        let h = Harness::new();
        h.state.load_hotkey_actions(vec![
            (
                "ctrl+mouse1".to_string(),
                taurine_core::db::crud::TriggerAction::text("clicked_ctrl_mouse1"),
            ),
            (
                "ralt+mouse4".to_string(),
                taurine_core::db::crud::TriggerAction::text("clicked_ralt_mouse4"),
            ),
        ]);

        // 1. Test ctrl+mouse1
        let r_ctrl = h.send(bare_event(EventType::KeyPress(Key::ControlLeft)));
        assert!(r_ctrl.is_some(), "Ctrl key press should pass through");
        assert!(h.l_ctrl.load(Ordering::Relaxed), "Left Ctrl must be active");

        let r_click = h.send(bare_event(EventType::ButtonPress(rdev::Button::Left)));
        assert!(
            r_click.is_none(),
            "ctrl+mouse1 hotkey must match and be swallowed on press"
        );

        let r_release = h.send(bare_event(EventType::ButtonRelease(rdev::Button::Left)));
        assert!(
            r_release.is_none(),
            "mouse1 release after match must be swallowed"
        );

        let r_subsequent_release = h.send(bare_event(EventType::ButtonRelease(rdev::Button::Left)));
        assert!(
            r_subsequent_release.is_some(),
            "subsequent mouse1 release should pass through"
        );

        let _ = h.send(bare_event(EventType::KeyRelease(Key::ControlLeft)));

        // 2. Test ralt+mouse4
        let r_alt = h.send(bare_event(EventType::KeyPress(Key::AltGr)));
        assert!(r_alt.is_some(), "AltGr key press should pass through");
        assert!(
            h.r_alt.load(Ordering::Relaxed),
            "Right Alt (AltGr) must be active"
        );

        let r_m4 = h.send(bare_event(EventType::ButtonPress(rdev::Button::Unknown(4))));
        assert!(
            r_m4.is_none(),
            "ralt+mouse4 hotkey must match and be swallowed on press"
        );

        let r_m4_rel = h.send(bare_event(EventType::ButtonRelease(rdev::Button::Unknown(
            4,
        ))));
        assert!(
            r_m4_rel.is_none(),
            "mouse4 release after match must be swallowed"
        );

        let r_m4_rel2 = h.send(bare_event(EventType::ButtonRelease(rdev::Button::Unknown(
            4,
        ))));
        assert!(
            r_m4_rel2.is_some(),
            "subsequent mouse4 release should pass through"
        );
    }

    /// Unmapped mouse clicks must interrupt any active typing buffer and pass through to the OS.
    #[test]
    fn unmapped_mouse_clicks_interrupt_buffer_and_pass_through() {
        let _guard = TestGuard::acquire();
        let h = Harness::new()
            .with_trigger("gm", "Good morning!")
            .with_hotkey_trigger("ctrl+mouse1", "clicked_ctrl_mouse1");

        // Type partial trigger into buffer
        h.type_str("gm");
        assert_eq!(h.buf(), "gm");

        // Unmapped right click (no modifiers)
        let r_click = h.send(bare_event(EventType::ButtonPress(rdev::Button::Right)));
        assert!(
            r_click.is_some(),
            "Unmapped right click must pass through to OS"
        );
        assert_eq!(
            h.buf(),
            "",
            "Unmapped mouse click must clear and interrupt the buffer"
        );

        let r_rel = h.send(bare_event(EventType::ButtonRelease(rdev::Button::Right)));
        assert!(
            r_rel.is_some(),
            "Unmapped mouse release must pass through to OS"
        );

        // Type again and test unmapped modifier+click (e.g. ctrl+mouse2 when only ctrl+mouse1 is mapped)
        h.type_str("gm");
        assert_eq!(h.buf(), "gm");

        let _ = h.send(bare_event(EventType::KeyPress(Key::ControlLeft)));
        let r_click2 = h.send(bare_event(EventType::ButtonPress(rdev::Button::Right)));
        assert!(
            r_click2.is_some(),
            "Unmapped ctrl+mouse2 must pass through to OS"
        );
        assert_eq!(
            h.buf(),
            "",
            "Unmapped ctrl+mouse2 must clear and interrupt the buffer"
        );

        let r_rel2 = h.send(bare_event(EventType::ButtonRelease(rdev::Button::Right)));
        assert!(
            r_rel2.is_some(),
            "Unmapped mouse release must pass through to OS"
        );
    }
}
