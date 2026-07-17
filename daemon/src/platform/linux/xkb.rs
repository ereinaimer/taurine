use evdev::KeyCode;
use std::collections::HashMap;
use taurine_core::engine::{EngineEvent, EngineMode};
use taurine_core::keys::{Modifier, Modifiers};
use tracing::warn;
use xkbcommon::xkb;

pub struct XkbMapper {
    state: Option<xkb::State>,
    reverse_map: HashMap<char, (KeyCode, bool)>,
    ctrl_pressed: bool,
    shift_pressed: bool,
    alt_pressed: bool,
    meta_pressed: bool,
}

// SAFETY: XkbMapper is moved to and owned by a single thread at a time, so its internal non-thread-safe state is never accessed concurrently.
unsafe impl Send for XkbMapper {}

impl Default for XkbMapper {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            warn!(
                "Failed to initialize XKB mapper: {}. Initializing headless mock XkbMapper.",
                e
            );
            Self::new_mock()
        })
    }
}

fn keycode_to_char(key: KeyCode, shift: bool) -> Option<char> {
    match key {
        KeyCode::KEY_A => Some(if shift { 'A' } else { 'a' }),
        KeyCode::KEY_B => Some(if shift { 'B' } else { 'b' }),
        KeyCode::KEY_C => Some(if shift { 'C' } else { 'c' }),
        KeyCode::KEY_D => Some(if shift { 'D' } else { 'd' }),
        KeyCode::KEY_E => Some(if shift { 'E' } else { 'e' }),
        KeyCode::KEY_F => Some(if shift { 'F' } else { 'f' }),
        KeyCode::KEY_G => Some(if shift { 'G' } else { 'g' }),
        KeyCode::KEY_H => Some(if shift { 'H' } else { 'h' }),
        KeyCode::KEY_I => Some(if shift { 'I' } else { 'i' }),
        KeyCode::KEY_J => Some(if shift { 'J' } else { 'j' }),
        KeyCode::KEY_K => Some(if shift { 'K' } else { 'k' }),
        KeyCode::KEY_L => Some(if shift { 'L' } else { 'l' }),
        KeyCode::KEY_M => Some(if shift { 'M' } else { 'm' }),
        KeyCode::KEY_N => Some(if shift { 'N' } else { 'n' }),
        KeyCode::KEY_O => Some(if shift { 'O' } else { 'o' }),
        KeyCode::KEY_P => Some(if shift { 'P' } else { 'p' }),
        KeyCode::KEY_Q => Some(if shift { 'Q' } else { 'q' }),
        KeyCode::KEY_R => Some(if shift { 'R' } else { 'r' }),
        KeyCode::KEY_S => Some(if shift { 'S' } else { 's' }),
        KeyCode::KEY_T => Some(if shift { 'T' } else { 't' }),
        KeyCode::KEY_U => Some(if shift { 'U' } else { 'u' }),
        KeyCode::KEY_V => Some(if shift { 'V' } else { 'v' }),
        KeyCode::KEY_W => Some(if shift { 'W' } else { 'w' }),
        KeyCode::KEY_X => Some(if shift { 'X' } else { 'x' }),
        KeyCode::KEY_Y => Some(if shift { 'Y' } else { 'y' }),
        KeyCode::KEY_Z => Some(if shift { 'Z' } else { 'z' }),
        KeyCode::KEY_1 => Some(if shift { '!' } else { '1' }),
        KeyCode::KEY_2 => Some(if shift { '@' } else { '2' }),
        KeyCode::KEY_3 => Some(if shift { '#' } else { '3' }),
        KeyCode::KEY_4 => Some(if shift { '$' } else { '4' }),
        KeyCode::KEY_5 => Some(if shift { '%' } else { '5' }),
        KeyCode::KEY_6 => Some(if shift { '^' } else { '6' }),
        KeyCode::KEY_7 => Some(if shift { '&' } else { '7' }),
        KeyCode::KEY_8 => Some(if shift { '*' } else { '8' }),
        KeyCode::KEY_9 => Some(if shift { '(' } else { '9' }),
        KeyCode::KEY_0 => Some(if shift { ')' } else { '0' }),
        KeyCode::KEY_SPACE => Some(' '),
        KeyCode::KEY_MINUS => Some(if shift { '_' } else { '-' }),
        KeyCode::KEY_EQUAL => Some(if shift { '+' } else { '=' }),
        KeyCode::KEY_LEFTBRACE => Some(if shift { '{' } else { '[' }),
        KeyCode::KEY_RIGHTBRACE => Some(if shift { '}' } else { ']' }),
        KeyCode::KEY_SEMICOLON => Some(if shift { ':' } else { ';' }),
        KeyCode::KEY_APOSTROPHE => Some(if shift { '"' } else { '\'' }),
        KeyCode::KEY_GRAVE => Some(if shift { '~' } else { '`' }),
        KeyCode::KEY_BACKSLASH => Some(if shift { '|' } else { '\\' }),
        KeyCode::KEY_COMMA => Some(if shift { '<' } else { ',' }),
        KeyCode::KEY_DOT => Some(if shift { '>' } else { '.' }),
        KeyCode::KEY_SLASH => Some(if shift { '?' } else { '/' }),
        _ => None,
    }
}

impl XkbMapper {
    fn modifier_is_active(&self, modifier: &str) -> bool {
        if let Some(state) = &self.state {
            state.mod_name_is_active(modifier, xkb::STATE_MODS_EFFECTIVE)
        } else {
            false
        }
    }

    pub fn new_mock() -> Self {
        let mut reverse_map = HashMap::new();
        let keys = [
            (KeyCode::KEY_A, 'a', 'A'),
            (KeyCode::KEY_B, 'b', 'B'),
            (KeyCode::KEY_C, 'c', 'C'),
            (KeyCode::KEY_D, 'd', 'D'),
            (KeyCode::KEY_E, 'e', 'E'),
            (KeyCode::KEY_F, 'f', 'F'),
            (KeyCode::KEY_G, 'g', 'G'),
            (KeyCode::KEY_H, 'h', 'H'),
            (KeyCode::KEY_I, 'i', 'I'),
            (KeyCode::KEY_J, 'j', 'J'),
            (KeyCode::KEY_K, 'k', 'K'),
            (KeyCode::KEY_L, 'l', 'L'),
            (KeyCode::KEY_M, 'm', 'M'),
            (KeyCode::KEY_N, 'n', 'N'),
            (KeyCode::KEY_O, 'o', 'O'),
            (KeyCode::KEY_P, 'p', 'P'),
            (KeyCode::KEY_Q, 'q', 'Q'),
            (KeyCode::KEY_R, 'r', 'R'),
            (KeyCode::KEY_S, 's', 'S'),
            (KeyCode::KEY_T, 't', 'T'),
            (KeyCode::KEY_U, 'u', 'U'),
            (KeyCode::KEY_V, 'v', 'V'),
            (KeyCode::KEY_W, 'w', 'W'),
            (KeyCode::KEY_X, 'x', 'X'),
            (KeyCode::KEY_Y, 'y', 'Y'),
            (KeyCode::KEY_Z, 'z', 'Z'),
            (KeyCode::KEY_1, '1', '!'),
            (KeyCode::KEY_2, '2', '@'),
            (KeyCode::KEY_3, '3', '#'),
            (KeyCode::KEY_4, '4', '$'),
            (KeyCode::KEY_5, '5', '%'),
            (KeyCode::KEY_6, '6', '^'),
            (KeyCode::KEY_7, '7', '&'),
            (KeyCode::KEY_8, '8', '*'),
            (KeyCode::KEY_9, '9', '('),
            (KeyCode::KEY_0, '0', ')'),
            (KeyCode::KEY_MINUS, '-', '_'),
            (KeyCode::KEY_EQUAL, '=', '+'),
            (KeyCode::KEY_LEFTBRACE, '[', '{'),
            (KeyCode::KEY_RIGHTBRACE, ']', '}'),
            (KeyCode::KEY_SEMICOLON, ';', ':'),
            (KeyCode::KEY_APOSTROPHE, '\'', '"'),
            (KeyCode::KEY_GRAVE, '`', '~'),
            (KeyCode::KEY_BACKSLASH, '\\', '|'),
            (KeyCode::KEY_COMMA, ',', '<'),
            (KeyCode::KEY_DOT, '.', '>'),
            (KeyCode::KEY_SLASH, '/', '?'),
        ];

        for &(keycode, unshifted, shifted) in &keys {
            reverse_map.insert(unshifted, (keycode, false));
            reverse_map.insert(shifted, (keycode, true));
        }

        reverse_map.insert(' ', (KeyCode::KEY_SPACE, false));
        reverse_map.insert('\t', (KeyCode::KEY_TAB, false));
        reverse_map.insert('\n', (KeyCode::KEY_ENTER, false));
        reverse_map.insert('\r', (KeyCode::KEY_ENTER, false));

        Self {
            state: None,
            reverse_map,
            ctrl_pressed: false,
            shift_pressed: false,
            alt_pressed: false,
            meta_pressed: false,
        }
    }

    pub fn new() -> Result<Self, String> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let mut keymap = xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        );

        if keymap.is_none() {
            warn!(
                "Failed to create XKB keymap from system environment defaults. Attempting fallback US keyboard layout..."
            );
            keymap = xkb::Keymap::new_from_names(
                &context,
                "evdev",
                "pc105",
                "us",
                "",
                None,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            );
        }

        // PONYTAIL: If both default and US keymap compilations fail, we fail layout initialization.
        // Ceiling: Headless systems without any XKB rules database files installed cannot run the daemon.
        // Upgrade path: Support a stateless mock layout fallback.
        let keymap = keymap.ok_or_else(|| {
            "Failed to compile both system default and fallback (evdev/pc105/us) XKB keymaps"
                .to_string()
        })?;

        let state = xkb::State::new(&keymap);
        let mut reverse_map = HashMap::new();

        // Scan common keycodes (8..255) to build a reverse lookup table for ASCII/standard chars
        for keycode in 8u32..256u32 {
            let key = KeyCode::new((keycode - 8) as u16);

            // Level 0: No Shift
            let syms0 = keymap.key_get_syms_by_level(keycode.into(), 0, 0);
            for sym in syms0 {
                if let Some(c) = char::from_u32(xkb::keysym_to_utf32(*sym))
                    && (c.is_ascii() || !c.is_control())
                {
                    reverse_map.entry(c).or_insert((key, false));
                }
            }

            // Level 1: Shift (usually)
            let syms1 = keymap.key_get_syms_by_level(keycode.into(), 0, 1);
            for sym in syms1 {
                if let Some(c) = char::from_u32(xkb::keysym_to_utf32(*sym))
                    && (c.is_ascii() || !c.is_control())
                {
                    reverse_map.entry(c).or_insert((key, true));
                }
            }
        }

        // Ensure whitespace characters are explicitly mapped as failsafes.
        // These keys are universal across almost all keyboard layouts.
        reverse_map
            .entry(' ')
            .or_insert((KeyCode::KEY_SPACE, false));
        reverse_map.entry('\t').or_insert((KeyCode::KEY_TAB, false));
        reverse_map
            .entry('\n')
            .or_insert((KeyCode::KEY_ENTER, false));
        reverse_map
            .entry('\r')
            .or_insert((KeyCode::KEY_ENTER, false));

        Ok(Self {
            state: Some(state),
            reverse_map,
            ctrl_pressed: false,
            shift_pressed: false,
            alt_pressed: false,
            meta_pressed: false,
        })
    }

    pub fn get_reverse_map(&self) -> &HashMap<char, (KeyCode, bool)> {
        &self.reverse_map
    }

    pub fn process_key(
        &mut self,
        key: KeyCode,
        is_press: bool,
        engine_mode: EngineMode,
        action_key: taurine_core::settings::ActionKey,
    ) -> Option<EngineEvent> {
        // evdev keycodes map to XKB keycodes by adding 8.
        let keycode = key.code() as u32 + 8;

        // Update modifiers on key press/release
        if let Some(state) = &mut self.state {
            state.update_key(
                keycode.into(),
                if is_press {
                    xkb::KeyDirection::Down
                } else {
                    xkb::KeyDirection::Up
                },
            );
        } else {
            match key {
                KeyCode::KEY_LEFTCTRL | KeyCode::KEY_RIGHTCTRL => {
                    self.ctrl_pressed = is_press;
                }
                KeyCode::KEY_LEFTSHIFT | KeyCode::KEY_RIGHTSHIFT => {
                    self.shift_pressed = is_press;
                }
                KeyCode::KEY_LEFTALT | KeyCode::KEY_RIGHTALT => {
                    self.alt_pressed = is_press;
                }
                KeyCode::KEY_LEFTMETA | KeyCode::KEY_RIGHTMETA => {
                    self.meta_pressed = is_press;
                }
                _ => {}
            }
        }

        if is_press {
            let ctrl_active = self.is_ctrl_down();
            let alt_active = self.is_alt_down();

            match key {
                KeyCode::KEY_ESC => return Some(EngineEvent::Interrupt),
                KeyCode::KEY_BACKSPACE => {
                    if ctrl_active {
                        return Some(EngineEvent::WordBackspace);
                    } else {
                        return Some(EngineEvent::Backspace);
                    }
                }
                KeyCode::KEY_SPACE => {
                    if action_key == taurine_core::settings::ActionKey::Space {
                        return Some(EngineEvent::ActionKey);
                    }
                    return Some(EngineEvent::Char(' '));
                }
                // Structural keys — break any active typing sequence.
                KeyCode::KEY_ENTER | KeyCode::KEY_KPENTER => {
                    if action_key == taurine_core::settings::ActionKey::Enter {
                        return Some(EngineEvent::ActionKey);
                    }
                    if matches!(engine_mode, EngineMode::AiCapture { .. }) {
                        return Some(EngineEvent::Char('\n'));
                    }
                    return Some(EngineEvent::Interrupt);
                }
                KeyCode::KEY_TAB => return Some(EngineEvent::Interrupt),
                // Navigation keys — cursor moved, buffer is now desynchronized.
                KeyCode::KEY_UP
                | KeyCode::KEY_DOWN
                | KeyCode::KEY_LEFT
                | KeyCode::KEY_RIGHT
                | KeyCode::KEY_HOME
                | KeyCode::KEY_END
                | KeyCode::KEY_PAGEUP
                | KeyCode::KEY_PAGEDOWN => return Some(EngineEvent::Interrupt),
                _ => {}
            }

            // Any key pressed with a modifier (ctrl/alt) is a system chord — skip.
            if ctrl_active || alt_active {
                return None;
            }

            let opt_char = if let Some(state) = &self.state {
                let s = state.key_get_utf8(keycode.into());
                let mut chars = s.chars();
                if let Some(c) = chars.next()
                    && chars.next().is_none()
                {
                    Some(c)
                } else {
                    None
                }
            } else {
                keycode_to_char(key, self.is_shift_down())
            };

            if let Some(c) = opt_char {
                return Some(EngineEvent::Char(c));
            }
        }
        None
    }

    pub fn is_alt_down(&self) -> bool {
        if self.state.is_some() {
            self.modifier_is_active(xkb::MOD_NAME_ALT)
        } else {
            self.alt_pressed
        }
    }

    pub fn is_ctrl_down(&self) -> bool {
        if self.state.is_some() {
            self.modifier_is_active(xkb::MOD_NAME_CTRL)
        } else {
            self.ctrl_pressed
        }
    }

    pub fn is_shift_down(&self) -> bool {
        if self.state.is_some() {
            self.modifier_is_active(xkb::MOD_NAME_SHIFT)
        } else {
            self.shift_pressed
        }
    }

    pub fn is_meta_down(&self) -> bool {
        if self.state.is_some() {
            self.modifier_is_active(xkb::MOD_NAME_LOGO)
        } else {
            self.meta_pressed
        }
    }

    pub fn current_modifiers(&self) -> Modifiers {
        let mut modifiers = Modifiers::new();
        if self.is_ctrl_down() {
            let _ = modifiers.insert(Modifier::Ctrl);
        }
        if self.is_shift_down() {
            let _ = modifiers.insert(Modifier::Shift);
        }
        if self.is_alt_down() {
            let _ = modifiers.insert(Modifier::Alt);
        }
        if self.is_meta_down() {
            let _ = modifiers.insert(Modifier::Meta);
        }
        modifiers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xkb_mapper_creation() {
        let mapper = XkbMapper::new();
        if let Ok(mut mapper) = mapper {
            let event = mapper.process_key(
                KeyCode::KEY_SPACE,
                true,
                EngineMode::Normal,
                taurine_core::settings::ActionKey::Enter,
            );
            assert_eq!(event, Some(EngineEvent::Char(' ')));
        }
    }

    #[test]
    fn test_xkb_mapper_mock() {
        let mut mapper = XkbMapper::new_mock();

        // Test unshifted character
        let event = mapper.process_key(
            KeyCode::KEY_A,
            true,
            EngineMode::Normal,
            taurine_core::settings::ActionKey::Enter,
        );
        assert_eq!(event, Some(EngineEvent::Char('a')));

        // Test shifted character
        mapper.process_key(
            KeyCode::KEY_LEFTSHIFT,
            true,
            EngineMode::Normal,
            taurine_core::settings::ActionKey::Enter,
        );
        assert!(mapper.is_shift_down());

        let event2 = mapper.process_key(
            KeyCode::KEY_B,
            true,
            EngineMode::Normal,
            taurine_core::settings::ActionKey::Enter,
        );
        assert_eq!(event2, Some(EngineEvent::Char('B')));

        mapper.process_key(
            KeyCode::KEY_LEFTSHIFT,
            false,
            EngineMode::Normal,
            taurine_core::settings::ActionKey::Enter,
        );
        assert!(!mapper.is_shift_down());

        // Test modifier chords don't output characters
        mapper.process_key(
            KeyCode::KEY_LEFTCTRL,
            true,
            EngineMode::Normal,
            taurine_core::settings::ActionKey::Enter,
        );
        assert!(mapper.is_ctrl_down());
        let event3 = mapper.process_key(
            KeyCode::KEY_C,
            true,
            EngineMode::Normal,
            taurine_core::settings::ActionKey::Enter,
        );
        assert_eq!(event3, None);
    }
}
