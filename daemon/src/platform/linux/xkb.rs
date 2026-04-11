use evdev::Key;
use taurine_core::engine::EngineEvent;
use tracing::error;
use xkbcommon::xkb;

pub struct XkbMapper {
    state: xkb::State,
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
        Ok(Self { state })
    }

    pub fn process_key(&mut self, key: Key, is_press: bool) -> Option<EngineEvent> {
        // evdev keycodes map to XKB keycodes by adding 8.
        let keycode = key.code() as u32 + 8;

        // Update modifiers on key press/release
        self.state.update_key(
            keycode,
            if is_press {
                xkb::KeyDirection::Down
            } else {
                xkb::KeyDirection::Up
            },
        );

        if is_press {
            match key {
                Key::KEY_ESC => return Some(EngineEvent::Interrupt),
                Key::KEY_BACKSPACE => {
                    let ctrl_active = self
                        .state
                        .mod_name_is_active(xkb::MOD_NAME_CTRL, xkb::STATE_MODS_EFFECTIVE);
                    if ctrl_active {
                        return Some(EngineEvent::WordBackspace);
                    } else {
                        return Some(EngineEvent::Backspace);
                    }
                }
                Key::KEY_SPACE => return Some(EngineEvent::Char(' ')),
                Key::KEY_ENTER | Key::KEY_KPENTER => return Some(EngineEvent::Interrupt),
                _ => {}
            }

            let s = self.state.key_get_utf8(keycode);
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
