use rdev::Key;
use taurine_core::engine::{CycleDirection, EngineMode};

pub(crate) fn case_cycle_key_action(
    key: Key,
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
    engine_mode: EngineMode,
    enabled: bool,
) -> Option<CycleDirection> {
    if !enabled || matches!(engine_mode, EngineMode::AiCapture { .. }) {
        return None;
    }
    if shift || ctrl || alt || meta {
        return None;
    }
    match key {
        Key::LeftArrow => Some(CycleDirection::Prev),
        Key::RightArrow => Some(CycleDirection::Next),
        _ => None,
    }
}
