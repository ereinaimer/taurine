use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardCaptureState {
    Healthy,
    Unhealthy,
    Unknown,
}

impl KeyboardCaptureState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Unhealthy => "unhealthy",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HookHealthSnapshot {
    pub listener_running: bool,
    pub hook_thread_started_at_unix_ms: u64,
    pub hook_entered_grab_at_unix_ms: u64,
    pub last_keyboard_event_at_unix_ms: u64,
    pub last_recovery_signal_at_unix_ms: u64,
    pub last_hook_exit_at_unix_ms: u64,
    pub last_hook_error: Option<String>,
    pub pending_recovery_reason: Option<String>,
}

impl HookHealthSnapshot {
    pub fn keyboard_capture_state(&self) -> KeyboardCaptureState {
        let awaiting_post_recovery_event = self.pending_recovery_reason.is_some()
            && self.last_keyboard_event_at_unix_ms < self.last_recovery_signal_at_unix_ms;

        let now = now_unix_ms();
        let stale = self.last_keyboard_event_at_unix_ms > 0
            && now > self.last_keyboard_event_at_unix_ms + 30_000;

        if !self.listener_running && self.hook_thread_started_at_unix_ms != 0 {
            KeyboardCaptureState::Unhealthy
        } else if self.hook_thread_started_at_unix_ms == 0
            || self.hook_entered_grab_at_unix_ms == 0
            || self.last_keyboard_event_at_unix_ms == 0
            || awaiting_post_recovery_event
            || stale
        {
            KeyboardCaptureState::Unknown
        } else {
            KeyboardCaptureState::Healthy
        }
    }

    pub fn recovery_suggestion(&self) -> Option<String> {
        let now = now_unix_ms();
        let stale = self.last_keyboard_event_at_unix_ms > 0
            && now > self.last_keyboard_event_at_unix_ms + 30_000;

        if !self.listener_running && self.hook_thread_started_at_unix_ms != 0 {
            Some("run `taurine restart` to reinitialize keyboard capture".to_string())
        } else if self.pending_recovery_reason.is_some()
            && self.last_keyboard_event_at_unix_ms < self.last_recovery_signal_at_unix_ms
        {
            Some("press a few keys; if capture does not recover, run `taurine restart`".to_string())
        } else if stale {
            Some("press a key to verify keyboard capture is active".to_string())
        } else {
            None
        }
    }
}

#[derive(Clone, Default)]
pub struct HookHealth {
    inner: Arc<HookHealthInner>,
}

#[derive(Default)]
struct HookHealthInner {
    listener_running: AtomicBool,
    hook_thread_started_at_unix_ms: AtomicU64,
    hook_entered_grab_at_unix_ms: AtomicU64,
    last_keyboard_event_at_unix_ms: AtomicU64,
    last_recovery_signal_at_unix_ms: AtomicU64,
    last_hook_exit_at_unix_ms: AtomicU64,
    last_hook_error: RwLock<Option<String>>,
    pending_recovery_reason: RwLock<Option<String>>,
}

impl HookHealth {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mark_listener_started(&self) {
        self.inner.listener_running.store(true, Ordering::SeqCst);
        self.inner
            .hook_thread_started_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
    }

    pub fn mark_listener_entering_grab(&self) {
        self.inner.listener_running.store(true, Ordering::SeqCst);
        self.inner
            .hook_entered_grab_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
    }

    pub fn record_keyboard_event(&self) {
        self.inner
            .last_keyboard_event_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
        if let Ok(mut reason) = self.inner.pending_recovery_reason.write() {
            *reason = None;
        }
    }

    pub fn mark_listener_exit(&self, error: Option<String>) {
        self.inner.listener_running.store(false, Ordering::SeqCst);
        self.inner
            .last_hook_exit_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
        if let Ok(mut last_error) = self.inner.last_hook_error.write() {
            *last_error = error;
        }
    }

    pub fn mark_recovery_signal(&self, reason: &str) {
        self.inner
            .last_recovery_signal_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
        if let Ok(mut pending_reason) = self.inner.pending_recovery_reason.write() {
            *pending_reason = Some(reason.to_string());
        }
    }

    pub fn snapshot(&self) -> HookHealthSnapshot {
        let last_hook_error = self
            .inner
            .last_hook_error
            .read()
            .map(|value| value.clone())
            .unwrap_or(None);
        let pending_recovery_reason = self
            .inner
            .pending_recovery_reason
            .read()
            .map(|value| value.clone())
            .unwrap_or(None);

        HookHealthSnapshot {
            listener_running: self.inner.listener_running.load(Ordering::SeqCst),
            hook_thread_started_at_unix_ms: self
                .inner
                .hook_thread_started_at_unix_ms
                .load(Ordering::SeqCst),
            hook_entered_grab_at_unix_ms: self
                .inner
                .hook_entered_grab_at_unix_ms
                .load(Ordering::SeqCst),
            last_keyboard_event_at_unix_ms: self
                .inner
                .last_keyboard_event_at_unix_ms
                .load(Ordering::SeqCst),
            last_recovery_signal_at_unix_ms: self
                .inner
                .last_recovery_signal_at_unix_ms
                .load(Ordering::SeqCst),
            last_hook_exit_at_unix_ms: self.inner.last_hook_exit_at_unix_ms.load(Ordering::SeqCst),
            last_hook_error,
            pending_recovery_reason,
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stale_hook_classification() {
        let now = now_unix_ms();
        let mut snapshot = HookHealthSnapshot {
            listener_running: true,
            hook_thread_started_at_unix_ms: now - 5000,
            hook_entered_grab_at_unix_ms: now - 4000,
            last_keyboard_event_at_unix_ms: now - 2000,
            last_recovery_signal_at_unix_ms: 0,
            last_hook_exit_at_unix_ms: 0,
            last_hook_error: None,
            pending_recovery_reason: None,
        };

        // Healthy state initially
        assert_eq!(
            snapshot.keyboard_capture_state(),
            KeyboardCaptureState::Healthy
        );

        // Force last keyboard event to be old (stale)
        snapshot.last_keyboard_event_at_unix_ms = now - 40_000;
        assert_eq!(
            snapshot.keyboard_capture_state(),
            KeyboardCaptureState::Unknown
        );
        assert!(
            snapshot
                .recovery_suggestion()
                .unwrap()
                .contains("press a key")
        );
    }
}
