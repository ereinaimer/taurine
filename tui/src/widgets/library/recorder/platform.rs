//! Host keyboard state the terminal event stream does not carry.
//!
//! Windows crossterm drops modifier-only key events, so the recorder polls
//! `GetAsyncKeyState` to learn which side of Ctrl/Shift/Alt/Win is down.

/// Sided virtual-key codes, in canonical chord order.
#[cfg(any(windows, test))]
const SIDED_KEYS: [(i32, &str); 8] = [
    (0xA2, "lctrl"),
    (0xA3, "rctrl"),
    (0xA0, "lshift"),
    (0xA1, "rshift"),
    (0xA4, "lalt"),
    (0xA5, "ralt"),
    (0x5B, "lmeta"),
    (0x5C, "rmeta"),
];

/// Sided modifier ids for the keys the probe reports as down.
#[cfg(any(windows, test))]
fn held_from(down: impl Fn(i32) -> bool) -> Vec<&'static str> {
    SIDED_KEYS
        .iter()
        .filter(|(code, _)| down(*code))
        .map(|(_, id)| *id)
        .collect()
}

#[cfg(windows)]
pub(crate) fn held_modifiers() -> Vec<&'static str> {
    // SAFETY: GetAsyncKeyState reads the calling thread's asynchronous
    // keyboard state; it takes a virtual-key code by value and touches no
    // caller memory, so every call below is valid.
    held_from(|code| unsafe { GetAsyncKeyState(code) } < 0)
}

#[cfg(not(windows))]
pub(crate) fn held_modifiers() -> Vec<&'static str> {
    // honey: only Windows exposes a portable side query here; kitty-capable
    // terminals report sides as modifier key events instead.
    Vec::new()
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_maps_each_virtual_key_to_its_side() {
        let held = held_from(|code| matches!(code, 0xA0 | 0xA3 | 0x5B));
        assert_eq!(held, vec!["rctrl", "lshift", "lmeta"]);
    }

    #[test]
    fn probe_reports_nothing_when_all_keys_are_up() {
        assert!(held_from(|_| false).is_empty());
    }
}
