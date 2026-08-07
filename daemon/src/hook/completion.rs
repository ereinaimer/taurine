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
    HistoryOlder,
    HistoryNewer,
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
        CompletionKeyKind::Up => CompletionKeyAction::HistoryOlder,
        CompletionKeyKind::Down => CompletionKeyAction::HistoryNewer,
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
        CompletionKeyAction::HistoryOlder | CompletionKeyAction::HistoryNewer
            if !state.inline_history_enabled() =>
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
        CompletionKeyKind::Up | CompletionKeyKind::Down => state.inline_history_enabled(),
        CompletionKeyKind::Escape | CompletionKeyKind::Other => false,
    }
}

pub(crate) const DOUBLE_TAP_INTERVAL_MS: u64 = 250;

#[derive(Clone, Copy, Default)]
struct DoubleTapLane {
    last_release_ms: Option<u64>,
    held: bool,
    reset_pending: bool,
}

#[derive(Default)]
pub(crate) struct DoubleTapTracker {
    up: DoubleTapLane,
    down: DoubleTapLane,
}

impl DoubleTapTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns true when the current press should be swallowed because it is a double-tap.
    ///
    /// A press counts as a double-tap only when the same key has already been pressed and
    /// released, and the press arrives strictly after that release with a gap of at most
    /// `DOUBLE_TAP_INTERVAL_MS`. Same-instant release->press (gap 0) does not form a pair.
    /// A press while the key is still held (key auto-repeat) records nothing and never fires.
    /// After a genuine double-tap the lane is disarmed so a third fast press cannot fire again:
    /// the double-tap's own release clears `last_release_ms` instead of re-arming it.
    pub(crate) fn on_press(&mut self, is_up: bool, now_ms: u64) -> bool {
        let lane = if is_up { &mut self.up } else { &mut self.down };
        if lane.held {
            return false;
        }
        lane.held = true;
        let is_double_tap = lane.last_release_ms.is_some_and(|release| {
            now_ms > release && now_ms.saturating_sub(release) <= DOUBLE_TAP_INTERVAL_MS
        });
        lane.reset_pending = is_double_tap;
        is_double_tap
    }

    pub(crate) fn on_release(&mut self, is_up: bool, now_ms: u64) {
        let lane = if is_up { &mut self.up } else { &mut self.down };
        if !lane.held {
            return;
        }
        lane.held = false;
        if lane.reset_pending {
            lane.last_release_ms = None;
            lane.reset_pending = false;
        } else {
            lane.last_release_ms = Some(now_ms);
        }
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
    !matches!(
        state.engine_mode(),
        taurine_core::engine::EngineMode::AiCapture { .. }
    ) && state
        .completion_active
        .load(std::sync::atomic::Ordering::Relaxed)
        && completion_is_active(evaluator)
}
