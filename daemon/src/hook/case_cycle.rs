use nite_core::engine::CycleDirection;
use rdev::Key;

pub(crate) fn case_cycle_key_action(
    key: Key,
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
    enabled: bool,
) -> Option<CycleDirection> {
    if !enabled {
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
