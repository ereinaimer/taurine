use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum AbortHandle {
    Tokio(tokio::task::AbortHandle),
    CancelFlag(Arc<AtomicBool>),
}

impl AbortHandle {
    pub fn abort(&self) {
        match self {
            Self::Tokio(handle) => handle.abort(),
            Self::CancelFlag(flag) => flag.store(true, Ordering::SeqCst),
        }
    }
}

#[derive(Clone, Default)]
pub struct SnoozeController {
    version: Arc<AtomicU64>,
    active: Arc<AtomicBool>,
    current_abort: Arc<Mutex<Option<AbortHandle>>>,
}

impl SnoozeController {
    pub fn new() -> Self {
        Self {
            version: Arc::new(AtomicU64::new(0)),
            active: Arc::new(AtomicBool::new(false)),
            current_abort: Arc::new(Mutex::new(None)),
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

        let version = self.version.clone();
        let active = self.active.clone();
        let abort_holder = self.current_abort.clone();

        let handle = tokio::runtime::Handle::try_current()
            .ok()
            .or_else(|| crate::TOKIO_HANDLE.get().cloned());

        if let Some(rt) = handle {
            let task = rt.spawn(async move {
                tokio::time::sleep(duration).await;
                if version.load(Ordering::SeqCst) == token {
                    active.store(false, Ordering::SeqCst);
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
            let thread_cancel = Arc::new(AtomicBool::new(false));
            let thread_cancel_clone = thread_cancel.clone();
            if let Ok(mut guard) = self.current_abort.lock() {
                *guard = Some(AbortHandle::CancelFlag(thread_cancel));
            }
            std::thread::spawn(move || {
                let start = std::time::Instant::now();
                while start.elapsed() < duration {
                    if thread_cancel_clone.load(Ordering::SeqCst) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50).min(duration));
                }
                if !thread_cancel_clone.load(Ordering::SeqCst)
                    && version.load(Ordering::SeqCst) == token
                {
                    active.store(false, Ordering::SeqCst);
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
        self.active.store(false, Ordering::SeqCst);
        self.version.fetch_add(1, Ordering::SeqCst) + 1
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
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
        tokio::time::sleep(Duration::from_millis(100)).await;
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

        std::thread::sleep(Duration::from_millis(100));
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

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!fired1.load(Ordering::SeqCst));
        assert!(fired2.load(Ordering::SeqCst));
    }
}
