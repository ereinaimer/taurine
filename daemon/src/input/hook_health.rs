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
            && now > self.last_keyboard_event_at_unix_ms + 35_000;

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
            && now > self.last_keyboard_event_at_unix_ms + 35_000;

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
    restart_count: std::sync::atomic::AtomicU64,
    consecutive_missed_raw_inputs: std::sync::atomic::AtomicU32,
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
        self.inner.restart_count.fetch_add(1, Ordering::SeqCst);
        self.inner
            .consecutive_missed_raw_inputs
            .store(0, Ordering::Relaxed);
    }

    pub fn mark_listener_entering_grab(&self) {
        self.inner.listener_running.store(true, Ordering::SeqCst);
        self.inner
            .hook_entered_grab_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
        self.inner
            .consecutive_missed_raw_inputs
            .store(0, Ordering::Relaxed);
    }

    pub fn record_keyboard_event(&self) {
        self.inner
            .last_keyboard_event_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
        self.inner
            .consecutive_missed_raw_inputs
            .store(0, Ordering::Relaxed);
        if let Ok(mut reason) = self.inner.pending_recovery_reason.write() {
            *reason = None;
        }
    }

    pub fn check_raw_input_keystroke_and_evaluate(
        &self,
        is_physical_press: bool,
        hook_grace_ms: u64,
        threshold_misses: u32,
    ) -> bool {
        if !is_physical_press {
            return false;
        }

        let now = now_unix_ms();
        let last_hook = self
            .inner
            .last_keyboard_event_at_unix_ms
            .load(Ordering::Relaxed);

        if last_hook == 0 {
            let grab_time = self
                .inner
                .hook_entered_grab_at_unix_ms
                .load(Ordering::Relaxed);
            if grab_time > 0 && now.saturating_sub(grab_time) < 1000 {
                return false;
            }
        }

        if now.saturating_sub(last_hook) >= hook_grace_ms {
            let misses = self
                .inner
                .consecutive_missed_raw_inputs
                .fetch_add(1, Ordering::SeqCst)
                + 1;
            if misses >= threshold_misses {
                self.inner
                    .consecutive_missed_raw_inputs
                    .store(0, Ordering::Relaxed);
                return true;
            }
        } else {
            self.inner
                .consecutive_missed_raw_inputs
                .store(0, Ordering::Relaxed);
        }

        false
    }

    pub fn mark_listener_exit(&self, error: Option<String>) {
        self.inner.listener_running.store(false, Ordering::SeqCst);
        self.inner
            .last_hook_exit_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
        if let Ok(mut last_error) = self.inner.last_hook_error.write() {
            *last_error = error.clone();
        }
        if let Some(ref err) = error {
            self.log_failure(err);
        }
    }

    pub fn mark_recovery_signal(&self, reason: &str) {
        self.inner
            .last_recovery_signal_at_unix_ms
            .store(now_unix_ms(), Ordering::SeqCst);
        if let Ok(mut pending_reason) = self.inner.pending_recovery_reason.write() {
            *pending_reason = Some(reason.to_string());
        }
        self.log_recovery(reason);
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

    pub fn restart_count(&self) -> u64 {
        self.inner.restart_count.load(Ordering::SeqCst)
    }

    pub fn log_periodic_health(&self) {
        let snapshot = self.snapshot();
        let now = now_unix_ms();
        tracing::info!(target: "taurine::hook",
            listener_running = snapshot.listener_running,
            ms_since_thread_start = now.saturating_sub(snapshot.hook_thread_started_at_unix_ms),
            ms_since_grab = now.saturating_sub(snapshot.hook_entered_grab_at_unix_ms),
            ms_since_last_event = now.saturating_sub(snapshot.last_keyboard_event_at_unix_ms),
            total_restarts = self.restart_count(),
            "health"
        );
    }

    pub fn log_recovery(&self, reason: &str) {
        tracing::warn!(target: "taurine::hook", reason, "recovery");
    }

    pub fn log_failure(&self, error: &str) {
        tracing::error!(target: "taurine::hook", error, "failure");
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

    #[test]
    fn test_watchdog_does_not_trigger_on_normal_idle_pause() {
        let health = HookHealth::new();
        health.mark_listener_started();
        health.mark_listener_entering_grab();

        // User types, then pauses for 60 seconds
        health.record_keyboard_event();

        // Simulate Raw Input receiving first key after 60s pause
        let should_recover = health.check_raw_input_keystroke_and_evaluate(
            true, // is_physical_press
            300,  // hook_event_grace_ms
            3,    // threshold_misses
        );

        assert!(
            !should_recover,
            "First key after an idle pause must NOT trigger hook recovery"
        );
    }

    #[test]
    fn test_watchdog_triggers_after_consecutive_unacknowledged_presses() {
        let health = HookHealth::new();
        health.mark_listener_started();
        health.mark_listener_entering_grab();
        health.record_keyboard_event();

        // 1st missed press
        assert!(!health.check_raw_input_keystroke_and_evaluate(true, 0, 3));
        // 2nd missed press
        assert!(!health.check_raw_input_keystroke_and_evaluate(true, 0, 3));
        // 3rd missed press without any hook events in between
        assert!(health.check_raw_input_keystroke_and_evaluate(true, 0, 3));
    }
}
