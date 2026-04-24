use std::collections::HashSet;

use taurine_core::engine::{EngineState, ExpansionResult};
use taurine_core::keys::{KeyPress, LogicalKey, Modifier, Modifiers};

#[derive(Debug, Clone, PartialEq)]
pub enum HotkeyEvaluation {
    NoMatch,
    Swallow,
    Matched(ExpansionResult),
}

#[derive(Default)]
pub struct HotkeyEvaluator {
    swallowed_keys: HashSet<LogicalKey>,
}

impl HotkeyEvaluator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_key_event(
        &mut self,
        state: &EngineState,
        is_press: bool,
        modifiers: Modifiers,
        key: LogicalKey,
    ) -> HotkeyEvaluation {
        if is_press {
            self.on_key_press(state, modifiers, key)
        } else {
            self.on_key_release(key)
        }
    }

    pub fn clear(&mut self) {
        self.swallowed_keys.clear();
    }

    fn on_key_press(
        &mut self,
        state: &EngineState,
        modifiers: Modifiers,
        key: LogicalKey,
    ) -> HotkeyEvaluation {
        if key.is_modifier_key().is_some() {
            return HotkeyEvaluation::NoMatch;
        }

        if self.swallowed_keys.contains(&key) {
            return HotkeyEvaluation::Swallow;
        }

        let hotkey = KeyPress { modifiers, key };
        let Some((trigger, expansion)) = state.fetch_hotkey_expansion(hotkey) else {
            return HotkeyEvaluation::NoMatch;
        };

        self.swallowed_keys.insert(key);
        HotkeyEvaluation::Matched(ExpansionResult {
            delete_count: 0,
            steps: expansion.steps,
            trigger,
            undo_trigger: None,
            is_calculation: expansion.is_calculation,
            track_usage: true,
            follow_up: None,
        })
    }

    pub fn on_key_release(&mut self, key: LogicalKey) -> HotkeyEvaluation {
        if key.is_modifier_key().is_some() {
            return HotkeyEvaluation::NoMatch;
        }

        if self.swallowed_keys.remove(&key) {
            HotkeyEvaluation::Swallow
        } else {
            HotkeyEvaluation::NoMatch
        }
    }
}

#[cfg(any(test, not(target_os = "linux")))]
pub fn modifiers_from_flags(ctrl: bool, shift: bool, alt: bool, meta: bool) -> Modifiers {
    let mut modifiers = Modifiers::new();
    if ctrl {
        let _ = modifiers.insert(Modifier::Ctrl);
    }
    if shift {
        let _ = modifiers.insert(Modifier::Shift);
    }
    if alt {
        let _ = modifiers.insert(Modifier::Alt);
    }
    if meta {
        let _ = modifiers.insert(Modifier::Meta);
    }
    modifiers
}

#[cfg(not(target_os = "linux"))]
pub fn logical_key_from_rdev(key: rdev::Key) -> Option<LogicalKey> {
    use rdev::Key;

    match key {
        Key::Alt | Key::AltGr => Some(LogicalKey::Modifier(Modifier::Alt)),
        Key::Backspace => Some(LogicalKey::Backspace),
        Key::CapsLock => Some(LogicalKey::CapsLock),
        Key::ControlLeft | Key::ControlRight => Some(LogicalKey::Modifier(Modifier::Ctrl)),
        Key::Delete | Key::KpDelete => Some(LogicalKey::Delete),
        Key::DownArrow => Some(LogicalKey::Down),
        Key::End => Some(LogicalKey::End),
        Key::Escape => Some(LogicalKey::Escape),
        Key::F1 => Some(LogicalKey::Function(1)),
        Key::F2 => Some(LogicalKey::Function(2)),
        Key::F3 => Some(LogicalKey::Function(3)),
        Key::F4 => Some(LogicalKey::Function(4)),
        Key::F5 => Some(LogicalKey::Function(5)),
        Key::F6 => Some(LogicalKey::Function(6)),
        Key::F7 => Some(LogicalKey::Function(7)),
        Key::F8 => Some(LogicalKey::Function(8)),
        Key::F9 => Some(LogicalKey::Function(9)),
        Key::F10 => Some(LogicalKey::Function(10)),
        Key::F11 => Some(LogicalKey::Function(11)),
        Key::F12 => Some(LogicalKey::Function(12)),
        Key::Home => Some(LogicalKey::Home),
        Key::Insert => Some(LogicalKey::Insert),
        Key::Kp0 => Some(LogicalKey::NumpadDigit(0)),
        Key::Kp1 => Some(LogicalKey::NumpadDigit(1)),
        Key::Kp2 => Some(LogicalKey::NumpadDigit(2)),
        Key::Kp3 => Some(LogicalKey::NumpadDigit(3)),
        Key::Kp4 => Some(LogicalKey::NumpadDigit(4)),
        Key::Kp5 => Some(LogicalKey::NumpadDigit(5)),
        Key::Kp6 => Some(LogicalKey::NumpadDigit(6)),
        Key::Kp7 => Some(LogicalKey::NumpadDigit(7)),
        Key::Kp8 => Some(LogicalKey::NumpadDigit(8)),
        Key::Kp9 => Some(LogicalKey::NumpadDigit(9)),
        Key::KpReturn | Key::Return => Some(LogicalKey::Enter),
        Key::LeftArrow => Some(LogicalKey::Left),
        Key::MetaLeft | Key::MetaRight => Some(LogicalKey::Modifier(Modifier::Meta)),
        Key::Minus | Key::KpMinus => Some(LogicalKey::Minus),
        Key::Num0 => Some(LogicalKey::Digit(0)),
        Key::Num1 => Some(LogicalKey::Digit(1)),
        Key::Num2 => Some(LogicalKey::Digit(2)),
        Key::Num3 => Some(LogicalKey::Digit(3)),
        Key::Num4 => Some(LogicalKey::Digit(4)),
        Key::Num5 => Some(LogicalKey::Digit(5)),
        Key::Num6 => Some(LogicalKey::Digit(6)),
        Key::Num7 => Some(LogicalKey::Digit(7)),
        Key::Num8 => Some(LogicalKey::Digit(8)),
        Key::Num9 => Some(LogicalKey::Digit(9)),
        Key::NumLock => Some(LogicalKey::NumLock),
        Key::PageDown => Some(LogicalKey::PageDown),
        Key::PageUp => Some(LogicalKey::PageUp),
        Key::Pause => Some(LogicalKey::Pause),
        Key::PrintScreen => Some(LogicalKey::PrintScreen),
        Key::RightArrow => Some(LogicalKey::Right),
        Key::ScrollLock => Some(LogicalKey::ScrollLock),
        Key::ShiftLeft | Key::ShiftRight => Some(LogicalKey::Modifier(Modifier::Shift)),
        Key::Space => Some(LogicalKey::Space),
        Key::Tab => Some(LogicalKey::Tab),
        Key::UpArrow => Some(LogicalKey::Up),
        Key::BackQuote => Some(LogicalKey::Backquote),
        Key::Equal => Some(LogicalKey::Equal),
        Key::LeftBracket => Some(LogicalKey::LeftBracket),
        Key::RightBracket => Some(LogicalKey::RightBracket),
        Key::SemiColon => Some(LogicalKey::Semicolon),
        Key::Quote => Some(LogicalKey::Quote),
        Key::BackSlash => Some(LogicalKey::Backslash),
        Key::Comma => Some(LogicalKey::Comma),
        Key::Dot => Some(LogicalKey::Period),
        Key::Slash => Some(LogicalKey::Slash),
        Key::KeyA => Some(LogicalKey::Letter('a')),
        Key::KeyB => Some(LogicalKey::Letter('b')),
        Key::KeyC => Some(LogicalKey::Letter('c')),
        Key::KeyD => Some(LogicalKey::Letter('d')),
        Key::KeyE => Some(LogicalKey::Letter('e')),
        Key::KeyF => Some(LogicalKey::Letter('f')),
        Key::KeyG => Some(LogicalKey::Letter('g')),
        Key::KeyH => Some(LogicalKey::Letter('h')),
        Key::KeyI => Some(LogicalKey::Letter('i')),
        Key::KeyJ => Some(LogicalKey::Letter('j')),
        Key::KeyK => Some(LogicalKey::Letter('k')),
        Key::KeyL => Some(LogicalKey::Letter('l')),
        Key::KeyM => Some(LogicalKey::Letter('m')),
        Key::KeyN => Some(LogicalKey::Letter('n')),
        Key::KeyO => Some(LogicalKey::Letter('o')),
        Key::KeyP => Some(LogicalKey::Letter('p')),
        Key::KeyQ => Some(LogicalKey::Letter('q')),
        Key::KeyR => Some(LogicalKey::Letter('r')),
        Key::KeyS => Some(LogicalKey::Letter('s')),
        Key::KeyT => Some(LogicalKey::Letter('t')),
        Key::KeyU => Some(LogicalKey::Letter('u')),
        Key::KeyV => Some(LogicalKey::Letter('v')),
        Key::KeyW => Some(LogicalKey::Letter('w')),
        Key::KeyX => Some(LogicalKey::Letter('x')),
        Key::KeyY => Some(LogicalKey::Letter('y')),
        Key::KeyZ => Some(LogicalKey::Letter('z')),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
pub fn logical_key_from_evdev(key: evdev::KeyCode) -> Option<LogicalKey> {
    use evdev::KeyCode;

    match key {
        KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => Some(LogicalKey::Modifier(Modifier::Alt)),
        KeyCode::KEY_BACKSPACE => Some(LogicalKey::Backspace),
        KeyCode::KEY_CAPSLOCK => Some(LogicalKey::CapsLock),
        KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => {
            Some(LogicalKey::Modifier(Modifier::Ctrl))
        }
        KeyCode::KEY_DELETE => Some(LogicalKey::Delete),
        KeyCode::KEY_DOWN => Some(LogicalKey::Down),
        KeyCode::KEY_END => Some(LogicalKey::End),
        KeyCode::KEY_ESC => Some(LogicalKey::Escape),
        KeyCode::KEY_F1 => Some(LogicalKey::Function(1)),
        KeyCode::KEY_F2 => Some(LogicalKey::Function(2)),
        KeyCode::KEY_F3 => Some(LogicalKey::Function(3)),
        KeyCode::KEY_F4 => Some(LogicalKey::Function(4)),
        KeyCode::KEY_F5 => Some(LogicalKey::Function(5)),
        KeyCode::KEY_F6 => Some(LogicalKey::Function(6)),
        KeyCode::KEY_F7 => Some(LogicalKey::Function(7)),
        KeyCode::KEY_F8 => Some(LogicalKey::Function(8)),
        KeyCode::KEY_F9 => Some(LogicalKey::Function(9)),
        KeyCode::KEY_F10 => Some(LogicalKey::Function(10)),
        KeyCode::KEY_F11 => Some(LogicalKey::Function(11)),
        KeyCode::KEY_F12 => Some(LogicalKey::Function(12)),
        KeyCode::KEY_HOME => Some(LogicalKey::Home),
        KeyCode::KEY_INSERT => Some(LogicalKey::Insert),
        KeyCode::KEY_KP0 => Some(LogicalKey::NumpadDigit(0)),
        KeyCode::KEY_KP1 => Some(LogicalKey::NumpadDigit(1)),
        KeyCode::KEY_KP2 => Some(LogicalKey::NumpadDigit(2)),
        KeyCode::KEY_KP3 => Some(LogicalKey::NumpadDigit(3)),
        KeyCode::KEY_KP4 => Some(LogicalKey::NumpadDigit(4)),
        KeyCode::KEY_KP5 => Some(LogicalKey::NumpadDigit(5)),
        KeyCode::KEY_KP6 => Some(LogicalKey::NumpadDigit(6)),
        KeyCode::KEY_KP7 => Some(LogicalKey::NumpadDigit(7)),
        KeyCode::KEY_KP8 => Some(LogicalKey::NumpadDigit(8)),
        KeyCode::KEY_KP9 => Some(LogicalKey::NumpadDigit(9)),
        KeyCode::KEY_KPENTER | KeyCode::KEY_ENTER => Some(LogicalKey::Enter),
        KeyCode::KEY_LEFT => Some(LogicalKey::Left),
        KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => {
            Some(LogicalKey::Modifier(Modifier::Meta))
        }
        KeyCode::KEY_MINUS | KeyCode::KEY_KPMINUS => Some(LogicalKey::Minus),
        KeyCode::KEY_0 => Some(LogicalKey::Digit(0)),
        KeyCode::KEY_1 => Some(LogicalKey::Digit(1)),
        KeyCode::KEY_2 => Some(LogicalKey::Digit(2)),
        KeyCode::KEY_3 => Some(LogicalKey::Digit(3)),
        KeyCode::KEY_4 => Some(LogicalKey::Digit(4)),
        KeyCode::KEY_5 => Some(LogicalKey::Digit(5)),
        KeyCode::KEY_6 => Some(LogicalKey::Digit(6)),
        KeyCode::KEY_7 => Some(LogicalKey::Digit(7)),
        KeyCode::KEY_8 => Some(LogicalKey::Digit(8)),
        KeyCode::KEY_9 => Some(LogicalKey::Digit(9)),
        KeyCode::KEY_NUMLOCK => Some(LogicalKey::NumLock),
        KeyCode::KEY_PAGEDOWN => Some(LogicalKey::PageDown),
        KeyCode::KEY_PAGEUP => Some(LogicalKey::PageUp),
        KeyCode::KEY_PAUSE => Some(LogicalKey::Pause),
        KeyCode::KEY_PRINT => Some(LogicalKey::PrintScreen),
        KeyCode::KEY_RIGHT => Some(LogicalKey::Right),
        KeyCode::KEY_SCROLLLOCK => Some(LogicalKey::ScrollLock),
        KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => {
            Some(LogicalKey::Modifier(Modifier::Shift))
        }
        KeyCode::KEY_SPACE => Some(LogicalKey::Space),
        KeyCode::KEY_TAB => Some(LogicalKey::Tab),
        KeyCode::KEY_UP => Some(LogicalKey::Up),
        KeyCode::KEY_GRAVE => Some(LogicalKey::Backquote),
        KeyCode::KEY_EQUAL => Some(LogicalKey::Equal),
        KeyCode::KEY_LEFTBRACE => Some(LogicalKey::LeftBracket),
        KeyCode::KEY_RIGHTBRACE => Some(LogicalKey::RightBracket),
        KeyCode::KEY_SEMICOLON => Some(LogicalKey::Semicolon),
        KeyCode::KEY_APOSTROPHE => Some(LogicalKey::Quote),
        KeyCode::KEY_BACKSLASH => Some(LogicalKey::Backslash),
        KeyCode::KEY_COMMA => Some(LogicalKey::Comma),
        KeyCode::KEY_DOT => Some(LogicalKey::Period),
        KeyCode::KEY_SLASH => Some(LogicalKey::Slash),
        KeyCode::KEY_A => Some(LogicalKey::Letter('a')),
        KeyCode::KEY_B => Some(LogicalKey::Letter('b')),
        KeyCode::KEY_C => Some(LogicalKey::Letter('c')),
        KeyCode::KEY_D => Some(LogicalKey::Letter('d')),
        KeyCode::KEY_E => Some(LogicalKey::Letter('e')),
        KeyCode::KEY_F => Some(LogicalKey::Letter('f')),
        KeyCode::KEY_G => Some(LogicalKey::Letter('g')),
        KeyCode::KEY_H => Some(LogicalKey::Letter('h')),
        KeyCode::KEY_I => Some(LogicalKey::Letter('i')),
        KeyCode::KEY_J => Some(LogicalKey::Letter('j')),
        KeyCode::KEY_K => Some(LogicalKey::Letter('k')),
        KeyCode::KEY_L => Some(LogicalKey::Letter('l')),
        KeyCode::KEY_M => Some(LogicalKey::Letter('m')),
        KeyCode::KEY_N => Some(LogicalKey::Letter('n')),
        KeyCode::KEY_O => Some(LogicalKey::Letter('o')),
        KeyCode::KEY_P => Some(LogicalKey::Letter('p')),
        KeyCode::KEY_Q => Some(LogicalKey::Letter('q')),
        KeyCode::KEY_R => Some(LogicalKey::Letter('r')),
        KeyCode::KEY_S => Some(LogicalKey::Letter('s')),
        KeyCode::KEY_T => Some(LogicalKey::Letter('t')),
        KeyCode::KEY_U => Some(LogicalKey::Letter('u')),
        KeyCode::KEY_V => Some(LogicalKey::Letter('v')),
        KeyCode::KEY_W => Some(LogicalKey::Letter('w')),
        KeyCode::KEY_X => Some(LogicalKey::Letter('x')),
        KeyCode::KEY_Y => Some(LogicalKey::Letter('y')),
        KeyCode::KEY_Z => Some(LogicalKey::Letter('z')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::db::crud::AutomationAction;
    use taurine_core::engine::{EngineEvent, Evaluator};
    use taurine_core::keys::Modifier;

    fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
        let mut bitset = Modifiers::new();
        for modifier in modifiers {
            let _ = bitset.insert(*modifier);
        }
        bitset
    }

    fn load_hotkey(state: &EngineState, trigger: &str, output: &str) {
        state.load_hotkey_actions(vec![(trigger.to_string(), AutomationAction::text(output))]);
    }

    #[test]
    fn hotkey_match_returns_expansion_result_and_swallow_outcome() {
        let state = EngineState::new('>');
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
        let state = std::sync::Arc::new(EngineState::new('>'));
        state.load_actions(vec![(
            "gm".to_string(),
            AutomationAction::text("Good morning!"),
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
            assert_eq!(text.process_event(EngineEvent::Char(ch)), None);
        }
        let expansion = text
            .process_event(EngineEvent::Char(' '))
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
        let state = std::sync::Arc::new(EngineState::new('>'));
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
        assert_eq!(text.process_event(EngineEvent::Char(' ')), None);
        assert!(state.take_active_undo_state().is_none());
    }

    #[test]
    fn hotkey_fires_once_per_press_until_release() {
        let state = EngineState::new('>');
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
    fn modifier_only_keypresses_do_not_match_hotkeys() {
        let state = EngineState::new('>');
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
        let state = EngineState::new('>');
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
    fn modifiers_from_flags_preserves_expected_order_independent_bitset() {
        let modifiers = modifiers_from_flags(true, true, false, true);
        let ordered: Vec<Modifier> = modifiers.ordered().collect();
        assert_eq!(
            ordered,
            vec![Modifier::Ctrl, Modifier::Shift, Modifier::Meta]
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
    }
}
