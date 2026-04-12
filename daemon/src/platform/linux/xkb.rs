use evdev::KeyCode;
use std::collections::HashMap;
use taurine_core::engine::EngineEvent;
use xkbcommon::xkb;

pub struct XkbMapper {
    state: xkb::State,
    reverse_map: HashMap<char, (KeyCode, bool)>,
}

impl Default for XkbMapper {
    fn default() -> Self {
        Self::new().expect("Failed to initialize XKB mapper")
    }
}

impl XkbMapper {
    pub fn new() -> Result<Self, String> {
        let context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);
        let keymap = xkb::Keymap::new_from_names(
            &context,
            "",
            "",
            "",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or_else(|| "Failed to create xkb keymap".to_string())?;

        let state = xkb::State::new(&keymap);
        let mut reverse_map = HashMap::new();

        // Scan common keycodes (8..255) to build a reverse lookup table for ASCII/standard chars
        for keycode in 8u32..256u32 {
            let key = KeyCode::new((keycode - 8) as u16);

            // Level 0: No Shift
            let syms0 = keymap.key_get_syms_by_level(keycode.into(), 0, 0);
            for sym in syms0 {
                if let Some(c) = char::from_u32(xkb::keysym_to_utf32(*sym)) {
                    if c.is_ascii() || !c.is_control() {
                        reverse_map.entry(c).or_insert((key, false));
                    }
                }
            }

            // Level 1: Shift (usually)
            let syms1 = keymap.key_get_syms_by_level(keycode.into(), 0, 1);
            for sym in syms1 {
                if let Some(c) = char::from_u32(xkb::keysym_to_utf32(*sym)) {
                    if c.is_ascii() || !c.is_control() {
                        reverse_map.entry(c).or_insert((key, true));
                    }
                }
            }
        }

        Ok(Self { state, reverse_map })
    }

    pub fn get_reverse_map(&self) -> &HashMap<char, (KeyCode, bool)> {
        &self.reverse_map
    }

    pub fn process_key(&mut self, key: KeyCode, is_press: bool) -> Option<EngineEvent> {
        // evdev keycodes map to XKB keycodes by adding 8.
        let keycode = key.code() as u32 + 8;

        // Update modifiers on key press/release
        self.state.update_key(
            keycode.into(),
            if is_press {
                xkb::KeyDirection::Down
            } else {
                xkb::KeyDirection::Up
            },
        );

        if is_press {
            match key {
                KeyCode::KEY_ESC => return Some(EngineEvent::Interrupt),
                KeyCode::KEY_BACKSPACE => {
                    let ctrl_active = self
                        .state
                        .mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
                    if ctrl_active {
                        return Some(EngineEvent::WordBackspace);
                    } else {
                        return Some(EngineEvent::Backspace);
                    }
                }
                KeyCode::KEY_SPACE => return Some(EngineEvent::Char(' ')),
                // Structural keys — break any active typing sequence.
                KeyCode::KEY_ENTER | KeyCode::KEY_KPENTER => return Some(EngineEvent::Interrupt),
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
            let ctrl_active = self
                .state
                .mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
            let alt_active = self
                .state
                .mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE);
            if ctrl_active || alt_active {
                return None;
            }

            let s = self.state.key_get_utf8(keycode.into());
            if s.chars().count() == 1 {
                return Some(EngineEvent::Char(s.chars().next().unwrap()));
            }
        }
        None
    }

    pub fn is_alt_down(&self) -> bool {
        self.state
            .mod_name_is_active(xkb::MOD_NAME_ALT, xkb::STATE_MODS_EFFECTIVE)
    }
}
