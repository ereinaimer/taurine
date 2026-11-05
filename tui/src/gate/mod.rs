//! Account gates: the signed-out door and the depleted-quota notice.
//!
//! Both render above the workspace. The signed-out gate owns the screen
//! because nothing behind it is usable; the quota gate sits over the
//! dimmed workspace so the user can still see and edit their library.

mod auth;
mod ui;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::widgets::library::ButtonSelection;

pub(crate) use ui::{hit, render_quota, render_signed_out};

/// The active account gate, if any.
pub(crate) enum Gate {
    SignedOut(SignInState),
    Quota(QuotaState),
}

pub(crate) struct SignInState {
    selected: ButtonSelection,
    phase: Phase,
    cancel: Arc<AtomicBool>,
    receiver: Option<Receiver<auth::LoginResult>>,
}

enum Phase {
    Idle,
    Waiting { started: Instant },
    Failed(String),
}

pub(crate) struct QuotaState {
    selected: ButtonSelection,
    reset_epoch: i64,
}

/// What the gate wants the app to do after a key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateAction {
    None,
    Quit,
    Dismiss,
}

impl Gate {
    pub(crate) fn signed_out() -> Self {
        Self::SignedOut(SignInState {
            // Google is the primary action, so it starts selected.
            selected: ButtonSelection::Confirm,
            phase: Phase::Idle,
            cancel: Arc::new(AtomicBool::new(false)),
            receiver: None,
        })
    }

    pub(crate) fn quota(reset_epoch: i64) -> Self {
        Self::Quota(QuotaState {
            selected: ButtonSelection::Confirm,
            reset_epoch,
        })
    }

    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> GateAction {
        // Ctrl+C quits from either gate, matching the rest of the TUI.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return GateAction::Quit;
        }
        match self {
            Self::SignedOut(state) => state.handle_key(key),
            Self::Quota(state) => state.handle_key(key),
        }
    }

    pub(crate) fn select(&mut self, selection: ButtonSelection) {
        match self {
            Self::SignedOut(state) => state.selected = selection,
            Self::Quota(state) => state.selected = selection,
        }
    }

    /// Polls a running sign-in once per tick. `Some(Ok(email))` closes
    /// the gate; `Some(Err)` leaves it open with the failure shown.
    pub(crate) fn poll(&mut self) -> Option<auth::LoginResult> {
        let Self::SignedOut(state) = self else {
            return None;
        };
        let receiver = state.receiver.as_ref()?;
        match receiver.try_recv() {
            Ok(result) => {
                state.receiver = None;
                if let Err(error) = &result {
                    state.phase = Phase::Failed(error.clone());
                }
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                state.receiver = None;
                let message = "Sign-in ended unexpectedly.".to_string();
                state.phase = Phase::Failed(message.clone());
                Some(Err(message))
            }
        }
    }

    pub(crate) const fn quota_reset_epoch(&self) -> Option<i64> {
        match self {
            Self::Quota(state) => Some(state.reset_epoch),
            Self::SignedOut(_) => None,
        }
    }
}

impl SignInState {
    fn handle_key(&mut self, key: KeyEvent) -> GateAction {
        match &self.phase {
            Phase::Waiting { .. } => {
                if key.code == KeyCode::Esc {
                    self.cancel.store(true, Ordering::Relaxed);
                    self.receiver = None;
                    self.phase = Phase::Idle;
                }
                GateAction::None
            }
            Phase::Idle | Phase::Failed(_) => match key.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                    self.selected = match self.selected {
                        ButtonSelection::Confirm => ButtonSelection::Cancel,
                        ButtonSelection::Cancel => ButtonSelection::Confirm,
                    };
                    GateAction::None
                }
                KeyCode::Enter => match self.selected {
                    ButtonSelection::Confirm => {
                        self.start();
                        GateAction::None
                    }
                    ButtonSelection::Cancel => GateAction::Quit,
                },
                KeyCode::Esc | KeyCode::Char('q') => GateAction::Quit,
                _ => GateAction::None,
            },
        }
    }

    fn start(&mut self) {
        // Retire any previous attempt before handing the new one its own
        // cancel flag; reusing the old flag would revive the old thread.
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.receiver = Some(auth::spawn(Arc::clone(&self.cancel)));
        self.phase = Phase::Waiting {
            started: Instant::now(),
        };
    }
}

impl QuotaState {
    fn handle_key(&mut self, key: KeyEvent) -> GateAction {
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                self.selected = match self.selected {
                    ButtonSelection::Confirm => ButtonSelection::Cancel,
                    ButtonSelection::Cancel => ButtonSelection::Confirm,
                };
                GateAction::None
            }
            KeyCode::Enter => match self.selected {
                ButtonSelection::Confirm => GateAction::Dismiss,
                ButtonSelection::Cancel => GateAction::Quit,
            },
            KeyCode::Esc | KeyCode::Char('c') => GateAction::Dismiss,
            KeyCode::Char('q') => GateAction::Quit,
            _ => GateAction::None,
        }
    }
}

impl Drop for SignInState {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Whether a key is one of the gate's navigation keys, used by tests.
#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn signed_out_starts_on_google_and_toggles() {
        let mut gate = Gate::signed_out();
        assert_eq!(gate.handle_key(key(KeyCode::Tab)), GateAction::None);
        // Cancel (Quit) is now selected; Enter quits.
        assert_eq!(gate.handle_key(key(KeyCode::Enter)), GateAction::Quit);
    }

    #[test]
    fn signed_out_esc_quits() {
        let mut gate = Gate::signed_out();
        assert_eq!(gate.handle_key(key(KeyCode::Esc)), GateAction::Quit);
    }

    #[test]
    fn quota_enter_dismisses_and_esc_dismisses() {
        let mut gate = Gate::quota(86_400);
        assert_eq!(gate.handle_key(key(KeyCode::Enter)), GateAction::Dismiss);

        let mut gate = Gate::quota(86_400);
        assert_eq!(gate.handle_key(key(KeyCode::Esc)), GateAction::Dismiss);
    }

    #[test]
    fn quota_q_quits() {
        let mut gate = Gate::quota(86_400);
        assert_eq!(gate.handle_key(key(KeyCode::Char('q'))), GateAction::Quit);
    }

    #[test]
    fn quota_reports_its_reset_epoch() {
        let gate = Gate::quota(1234);
        assert_eq!(gate.quota_reset_epoch(), Some(1234));
        assert_eq!(Gate::signed_out().quota_reset_epoch(), None);
    }
}
