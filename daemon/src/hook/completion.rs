use std::sync::{Arc, Mutex};
use taurine_core::engine::Evaluator;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionKeyKind {
    Tab,
    Escape,
    Up,
    Down,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompletionKeyAction {
    CycleForward,
    CycleBackward,
    CancelAndSwallow,
    CancelAndPassThrough,
    PassThrough,
}

pub(crate) fn completion_key_action(
    key: CompletionKeyKind,
    shift_active: bool,
    ctrl_active: bool,
    alt_active: bool,
    meta_active: bool,
) -> CompletionKeyAction {
    match key {
        CompletionKeyKind::Tab => {
            if ctrl_active || alt_active || meta_active {
                CompletionKeyAction::CancelAndPassThrough
            } else if shift_active {
                CompletionKeyAction::CycleBackward
            } else {
                CompletionKeyAction::CycleForward
            }
        }
        CompletionKeyKind::Escape => CompletionKeyAction::CancelAndSwallow,
        CompletionKeyKind::Up | CompletionKeyKind::Down => CompletionKeyAction::PassThrough,
        CompletionKeyKind::Other => CompletionKeyAction::PassThrough,
    }
}

pub(crate) fn trigger_assist_key_action(
    state: &taurine_core::engine::EngineState,
    key: CompletionKeyKind,
    shift_active: bool,
    ctrl_active: bool,
    alt_active: bool,
    meta_active: bool,
) -> CompletionKeyAction {
    match completion_key_action(key, shift_active, ctrl_active, alt_active, meta_active) {
        CompletionKeyAction::CycleForward | CompletionKeyAction::CycleBackward
            if !state.inline_tab_completion_enabled() =>
        {
            CompletionKeyAction::PassThrough
        }
        action => action,
    }
}

pub(crate) fn completion_key_kind_from_tab_like(
    is_tab: bool,
    is_escape: bool,
    is_up: bool,
    is_down: bool,
) -> CompletionKeyKind {
    if is_tab {
        CompletionKeyKind::Tab
    } else if is_escape {
        CompletionKeyKind::Escape
    } else if is_up {
        CompletionKeyKind::Up
    } else if is_down {
        CompletionKeyKind::Down
    } else {
        CompletionKeyKind::Other
    }
}

pub(crate) fn should_swallow_trigger_assist_key_release(
    state: &taurine_core::engine::EngineState,
    key: CompletionKeyKind,
) -> bool {
    match key {
        CompletionKeyKind::Tab => state.inline_tab_completion_enabled(),
        CompletionKeyKind::Up | CompletionKeyKind::Down => false,
        CompletionKeyKind::Escape | CompletionKeyKind::Other => false,
    }
}

pub(super) fn completion_is_active(evaluator: &Arc<Mutex<Evaluator>>) -> bool {
    super::listener::with_evaluator_lock(evaluator, "completion_is_active", |lock| {
        lock.is_completion_active()
    })
    .unwrap_or(false)
}

pub(crate) fn trigger_assist_is_active(
    evaluator: &Arc<Mutex<Evaluator>>,
    state: &taurine_core::engine::EngineState,
) -> bool {
    state
        .completion_active
        .load(std::sync::atomic::Ordering::Relaxed)
        && completion_is_active(evaluator)
}
