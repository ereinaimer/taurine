use evdev::KeyCode;
use std::collections::HashMap;
use taurine_core::engine::{EngineEvent, EngineMode};
use taurine_core::keys::{Modifier, Modifiers};
use tracing::warn;
use xkbcommon::xkb;

pub struct XkbMapper {
    state: xkb::State,
    reverse_map: HashMap<char, (KeyCode, bool)>,
}

impl Default for XkbMapper {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            panic!("Failed to initialize XKB mapper: {}", e);
        })
    }
}

impl XkbMapper {
    fn modifier_is_active(&self, modifier: &str) -> bool {
        self.state
            .mod_name_is_active(modifier, xkb::STATE_MODS_EFFECTIVE)
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

        Ok(Self { state, reverse_map })
    }

    pub fn get_reverse_map(&self) -> &HashMap<char, (KeyCode, bool)> {
        &self.reverse_map
    }

    pub fn process_key(
        &mut self,
        key: KeyCode,
        is_press: bool,
        engine_mode: EngineMode,
        action_delimiter: taurine_core::settings::ActionDelimiter,
    ) -> Option<EngineEvent> {
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
                    if action_delimiter == taurine_core::settings::ActionDelimiter::Space {
                        return Some(EngineEvent::ActionDelimiter);
                    }
                    return Some(EngineEvent::Char(' '));
                }
                // Structural keys — break any active typing sequence.
                KeyCode::KEY_ENTER | KeyCode::KEY_KPENTER => {
                    if action_delimiter == taurine_core::settings::ActionDelimiter::Enter {
                        return Some(EngineEvent::ActionDelimiter);
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

            let s = self.state.key_get_utf8(keycode.into());
            if s.chars().count() == 1 {
                return Some(EngineEvent::Char(s.chars().next().unwrap()));
            }
        }
        None
    }

    pub fn is_alt_down(&self) -> bool {
        self.modifier_is_active(xkb::MOD_NAME_ALT)
    }

    pub fn is_ctrl_down(&self) -> bool {
        self.modifier_is_active(xkb::MOD_NAME_CTRL)
    }

    pub fn is_shift_down(&self) -> bool {
        self.modifier_is_active(xkb::MOD_NAME_SHIFT)
    }

    pub fn is_meta_down(&self) -> bool {
        self.modifier_is_active(xkb::MOD_NAME_LOGO)
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
                EngineMode::Standard,
                taurine_core::settings::ActionDelimiter::Enter,
            );
            assert_eq!(event, Some(EngineEvent::Char(' ')));
        }
    }
}
