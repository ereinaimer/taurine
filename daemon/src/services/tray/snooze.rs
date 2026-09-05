use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum AbortHandle {
    Tokio(tokio::task::AbortHandle),
    CancelSender(std::sync::mpsc::SyncSender<()>),
}

impl AbortHandle {
    pub fn abort(&self) {
        match self {
            Self::Tokio(handle) => handle.abort(),
            Self::CancelSender(sender) => {
                let _ = sender.try_send(());
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct SnoozeController {
    version: Arc<AtomicU64>,
    active: Arc<AtomicBool>,
    current_abort: Arc<Mutex<Option<AbortHandle>>>,
    target_time: Arc<Mutex<Option<std::time::Instant>>>,
}

impl SnoozeController {
    pub fn new() -> Self {
        Self {
            version: Arc::new(AtomicU64::new(0)),
            active: Arc::new(AtomicBool::new(false)),
            current_abort: Arc::new(Mutex::new(None)),
            target_time: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start_snooze<F>(&self, duration: Duration, on_expire: F) -> u64
    where
        F: FnOnce() + Send + 'static,
    {
        // Abort any existing running snooze task immediately
        if let Ok(mut guard) = self.current_abort.lock()
            && let Some(prev) = guard.take()
        {
            prev.abort();
        }

        let token = self.version.fetch_add(1, Ordering::SeqCst) + 1;
        self.active.store(true, Ordering::SeqCst);

        let deadline = std::time::Instant::now() + duration;
        if let Ok(mut guard) = self.target_time.lock() {
            *guard = Some(deadline);
        }

        let version = self.version.clone();
        let active = self.active.clone();
        let abort_holder = self.current_abort.clone();
        let target_holder = self.target_time.clone();

        let handle = tokio::runtime::Handle::try_current()
            .ok()
            .or_else(|| crate::TOKIO_HANDLE.get().cloned());

        if let Some(rt) = handle {
            let task = rt.spawn(async move {
                tokio::time::sleep(duration).await;
                if version.load(Ordering::SeqCst) == token {
                    active.store(false, Ordering::SeqCst);
                    if let Ok(mut guard) = target_holder.lock() {
                        *guard = None;
                    }
                    if let Ok(mut guard) = abort_holder.lock() {
                        *guard = None;
                    }
                    on_expire();
                }
            });
            if let Ok(mut guard) = self.current_abort.lock() {
                *guard = Some(AbortHandle::Tokio(task.abort_handle()));
            }
        } else {
            let (cancel_tx, cancel_rx) = std::sync::mpsc::sync_channel(1);
            if let Ok(mut guard) = self.current_abort.lock() {
                *guard = Some(AbortHandle::CancelSender(cancel_tx));
            }
            std::thread::spawn(move || {
                match cancel_rx.recv_timeout(duration) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                if version.load(Ordering::SeqCst) == token {
                    active.store(false, Ordering::SeqCst);
                    if let Ok(mut guard) = target_holder.lock() {
                        *guard = None;
                    }
                    if let Ok(mut guard) = abort_holder.lock() {
                        *guard = None;
                    }
                    on_expire();
                }
            });
        }

        token
    }

    pub fn cancel(&self) -> u64 {
        if let Ok(mut guard) = self.current_abort.lock()
            && let Some(prev) = guard.take()
        {
            prev.abort();
        }
        if let Ok(mut guard) = self.target_time.lock() {
            *guard = None;
        }
        self.active.store(false, Ordering::SeqCst);
        self.version.fetch_add(1, Ordering::SeqCst) + 1
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn remaining_time(&self) -> Option<Duration> {
        if !self.is_active() {
            return None;
        }
        let guard = self.target_time.lock().ok()?;
        let target = (*guard)?;
        let now = std::time::Instant::now();
        if target > now {
            Some(target - now)
        } else {
            Some(Duration::ZERO)
        }
    }

    pub fn resume_label(&self) -> String {
        if let Some(remaining) = self.remaining_time() {
            let total_secs = remaining.as_secs();
            let mins = total_secs / 60;
            let secs = total_secs % 60;
            if mins > 0 {
                format!("Resume ({}m {:02}s)", mins, secs)
            } else {
                format!("Resume ({}s)", secs)
            }
        } else {
            "Resume".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn test_snooze_fires_after_duration() {
        let controller = SnoozeController::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        controller.start_snooze(Duration::from_millis(50), move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        assert!(!fired.load(Ordering::SeqCst));
        let start = std::time::Instant::now();
        while !fired.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(2) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(fired.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_snooze_cancelled_by_new_snooze_or_cancel() {
        let controller = SnoozeController::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        controller.start_snooze(Duration::from_millis(50), move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        controller.cancel();

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!fired.load(Ordering::SeqCst));
    }

    #[test]
    fn test_snooze_from_plain_os_thread_without_tokio_context() {
        let controller = SnoozeController::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        controller.start_snooze(Duration::from_millis(50), move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        let start = std::time::Instant::now();
        while !fired.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(fired.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_snooze_aborts_previous_task_immediately() {
        let controller = SnoozeController::new();
        let fired1 = Arc::new(AtomicBool::new(false));
        let fired1_clone = fired1.clone();
        let fired2 = Arc::new(AtomicBool::new(false));
        let fired2_clone = fired2.clone();

        controller.start_snooze(Duration::from_millis(100), move || {
            fired1_clone.store(true, Ordering::SeqCst);
        });

        // Immediately start another snooze with a shorter duration
        tokio::time::sleep(Duration::from_millis(10)).await;
        controller.start_snooze(Duration::from_millis(50), move || {
            fired2_clone.store(true, Ordering::SeqCst);
        });

        let start = std::time::Instant::now();
        while !fired2.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(2) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(!fired1.load(Ordering::SeqCst));
        assert!(fired2.load(Ordering::SeqCst));
    }

    #[test]
    fn test_remaining_time_and_resume_label_when_inactive() {
        let controller = SnoozeController::new();
        assert!(!controller.is_active());
        assert_eq!(controller.remaining_time(), None);
        assert_eq!(controller.resume_label(), "Resume");
    }

    #[test]
    fn test_remaining_time_and_resume_label_during_snooze() {
        let controller = SnoozeController::new();
        controller.start_snooze(Duration::from_secs(15 * 60), || {});

        assert!(controller.is_active());
        let remaining = controller
            .remaining_time()
            .expect("active snooze remaining");
        assert!(remaining >= Duration::from_secs(14 * 60 + 50));
        assert!(remaining <= Duration::from_secs(15 * 60));

        let label = controller.resume_label();
        assert!(
            label.starts_with("Resume (14m ") || label == "Resume (15m 00s)",
            "Unexpected label: {label}"
        );
    }

    #[test]
    fn test_resume_label_under_one_minute() {
        let controller = SnoozeController::new();
        controller.start_snooze(Duration::from_secs(45), || {});

        let label = controller.resume_label();
        assert!(
            label.starts_with("Resume (4") && label.ends_with("s)"),
            "Unexpected under-minute label: {label}"
        );
    }

    #[test]
    fn test_remaining_time_cleared_on_cancel() {
        let controller = SnoozeController::new();
        controller.start_snooze(Duration::from_secs(30 * 60), || {});
        assert!(controller.remaining_time().is_some());

        controller.cancel();
        assert_eq!(controller.remaining_time(), None);
        assert_eq!(controller.resume_label(), "Resume");
    }
}
