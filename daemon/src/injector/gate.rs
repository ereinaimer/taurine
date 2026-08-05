use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use tracing::{error, trace};

pub static IS_INJECTING: AtomicBool = AtomicBool::new(false);

/// Monotonically increasing generation counter.
///
/// Each call to `abort_injection()` bumps the generation.  Injection tasks
/// capture the generation at creation time — if it changes (another task
/// was aborted) the task knows its own injection has been superseded.
///
/// This gives per-task abort isolation with a single atomic counter,
/// avoiding the cross‑task contamination that a boolean flag causes when
/// multiple pool threads run concurrently.
pub static INJECTION_GENERATION: AtomicU64 = AtomicU64::new(0);

pub(super) static INJECTION_SCOPE_DEPTH: AtomicUsize = AtomicUsize::new(0);
pub(super) static INJECTION_VISIBILITY_DEPTH: AtomicUsize = AtomicUsize::new(0);

#[allow(dead_code)]
pub static IS_SIMULATING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub(super) struct InjectionGate<'a> {
    is_injecting: &'a AtomicBool,
    scope_depth: &'a AtomicUsize,
    visibility_depth: &'a AtomicUsize,
}

impl<'a> InjectionGate<'a> {
    pub(super) const fn new(
        is_injecting: &'a AtomicBool,
        scope_depth: &'a AtomicUsize,
        visibility_depth: &'a AtomicUsize,
    ) -> Self {
        Self {
            is_injecting,
            scope_depth,
            visibility_depth,
        }
    }

    pub(super) fn begin_scope(self) {
        self.scope_depth.fetch_add(1, Ordering::SeqCst);
        self.visibility_depth.fetch_add(1, Ordering::SeqCst);
        self.is_injecting.store(true, Ordering::SeqCst);
    }

    pub(super) fn end_scope(self) {
        let previous_scope_depth = self.scope_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_scope_depth > 0, "scope depth underflow");
        let _remaining_scope_depth = previous_scope_depth.saturating_sub(1);

        let previous_visibility_depth = self.visibility_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_visibility_depth > 0, "visibility depth underflow");
        let remaining_visibility_depth = previous_visibility_depth.saturating_sub(1);

        self.is_injecting
            .store(remaining_visibility_depth > 0, Ordering::SeqCst);
    }

    pub(super) fn begin_visibility(self) {
        self.visibility_depth.fetch_add(1, Ordering::SeqCst);
        self.is_injecting.store(true, Ordering::SeqCst);
    }

    pub(super) fn end_visibility(self) {
        let previous_visibility_depth = self.visibility_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_visibility_depth > 0, "visibility depth underflow");
        let remaining_visibility_depth = previous_visibility_depth.saturating_sub(1);
        self.is_injecting
            .store(remaining_visibility_depth > 0, Ordering::SeqCst);
    }
}

fn injection_gate() -> InjectionGate<'static> {
    InjectionGate::new(
        &IS_INJECTING,
        &INJECTION_SCOPE_DEPTH,
        &INJECTION_VISIBILITY_DEPTH,
    )
}

pub struct InjectionFlagGuard {
    gate: InjectionGate<'static>,
}

impl InjectionFlagGuard {
    pub fn begin() -> Self {
        let gate = injection_gate();
        gate.begin_scope();

        trace!(
            scope_depth = INJECTION_SCOPE_DEPTH.load(Ordering::SeqCst),
            visibility_depth = INJECTION_VISIBILITY_DEPTH.load(Ordering::SeqCst),
            "Injection guard armed"
        );

        Self { gate }
    }
}

impl Drop for InjectionFlagGuard {
    fn drop(&mut self) {
        self.gate.end_scope();

        trace!(
            remaining_scope_depth = INJECTION_SCOPE_DEPTH.load(Ordering::SeqCst),
            remaining_visibility_depth = INJECTION_VISIBILITY_DEPTH.load(Ordering::SeqCst),
            restored_injecting = IS_INJECTING.load(Ordering::SeqCst),
            "Injection guard reset"
        );
    }
}

pub struct InjectionVisibilityGuard {
    gate: InjectionGate<'static>,
}

impl InjectionVisibilityGuard {
    pub fn begin() -> Self {
        let gate = injection_gate();
        gate.begin_visibility();
        Self { gate }
    }
}

impl Drop for InjectionVisibilityGuard {
    fn drop(&mut self) {
        self.gate.end_visibility();
        trace!(
            remaining_visibility_depth = INJECTION_VISIBILITY_DEPTH.load(Ordering::SeqCst),
            restored_injecting = IS_INJECTING.load(Ordering::SeqCst),
            "Injection visibility guard reset"
        );
    }
}

pub fn spawn_guarded_injection_thread<F>(thread_name: &str, task: F)
where
    F: FnOnce() + Send + 'static,
{
    if let Some(tx) = INJECTION_POOL.get() {
        let task = Box::new(task);
        if tx.send(task).is_err() {
            error!(thread_name, "Injection pool channel closed, dropping task");
        }
    } else {
        // Fallback: direct spawn (happens before pool is initialized)
        let guard = InjectionFlagGuard::begin();
        let spawn_result = thread::Builder::new()
            .name(thread_name.to_string())
            .spawn(move || {
                let _guard = guard;
                task();
            });

        if let Err(error) = spawn_result {
            error!(
                thread_name,
                error = %error,
                "Failed to spawn guarded injection thread"
            );
        }
    }
}

/// Bump the injection generation, signalling all currently-running
/// injection tasks that their work has been superseded.
pub fn abort_injection() {
    INJECTION_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Capture the current generation.  Tasks should call this at creation
/// and then use `is_aborted()` at each check point.
pub fn capture_generation() -> u64 {
    INJECTION_GENERATION.load(Ordering::SeqCst)
}

/// Returns `true` when the generation has advanced past `captured`,
/// meaning another task was aborted since this task started.
pub fn is_aborted(captured: u64) -> bool {
    INJECTION_GENERATION.load(Ordering::SeqCst) != captured
}

static INJECTION_POOL: OnceLock<mpsc::Sender<Box<dyn FnOnce() + Send>>> = OnceLock::new();

pub fn init_injection_pool() {
    let (tx, rx) = mpsc::channel::<Box<dyn FnOnce() + Send>>();
    let rx = Arc::new(Mutex::new(rx));
    for i in 0..2 {
        let rx = rx.clone();
        if let Err(error) = std::thread::Builder::new()
            .name(format!("tau-inject-{i}"))
            .spawn(move || {
                loop {
                    let task = match rx.lock() {
                        Ok(guard) => guard.recv(),
                        Err(_) => {
                            tracing::error!(
                                "injection pool receiver mutex poisoned; stopping worker {i}"
                            );
                            break;
                        }
                    };
                    match task {
                        Ok(task) => {
                            let _guard = InjectionFlagGuard::begin();
                            task();
                        }
                        Err(_) => break,
                    }
                }
            })
        {
            tracing::error!(
                error = %error,
                "failed to spawn injection pool worker {i}"
            );
        }
    }
    INJECTION_POOL.set(tx).ok();
}

pub(super) fn inject_mutex() -> &'static Mutex<()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_injection_pool_executes_task() {
        init_injection_pool();
        let executed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let executed_clone = executed.clone();
        spawn_guarded_injection_thread("test-pool", move || {
            executed_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(executed.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn test_injection_pool_sets_injecting_flag() {
        init_injection_pool();
        let flag_was_set = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag_clone = flag_was_set.clone();
        spawn_guarded_injection_thread("test-flag", move || {
            flag_clone.store(
                IS_INJECTING.load(std::sync::atomic::Ordering::SeqCst),
                std::sync::atomic::Ordering::SeqCst,
            );
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(flag_was_set.load(std::sync::atomic::Ordering::SeqCst));
    }

    // Regression: tab-complete-then-enter fails to expand.
    //
    // The generation counter gives per-task abort isolation.  When
    // abort_injection() bumps the generation, only tasks that captured
    // the old generation see the abort.  A newly-started task captures
    // the current generation and is never affected by aborts that
    // happened before it started.
    //
    // Previously INJECTION_ABORT was a boolean flag that leaked across
    // concurrent pool tasks, causing expansions to silently fail.

    #[test]
    fn injection_abort_bumps_generation() {
        let gen1 = INJECTION_GENERATION.load(Ordering::SeqCst);
        abort_injection();
        let gen2 = INJECTION_GENERATION.load(Ordering::SeqCst);
        assert!(gen2 > gen1, "abort_injection() must increment generation");
    }

    #[test]
    fn new_task_does_not_see_old_abort_generation() {
        // Simulate: an abort happened while a previous task was running.
        abort_injection();
        let captured = capture_generation();

        // A new task starting now should NOT see the abort — it
        // captured the current generation, and no new abort has happened.
        assert!(
            !is_aborted(captured),
            "new task must not see abort from previous generation"
        );
    }

    #[test]
    fn concurrent_pool_tasks_do_not_inherit_stale_abort() {
        init_injection_pool();

        // Simulate: the listener aborted a previous injection.
        abort_injection();

        // A new task dispatched to the pool captures the current
        // generation — it must not be affected by the old abort.
        let expansion_ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let expansion_ran_clone = expansion_ran.clone();
        spawn_guarded_injection_thread("test-stale-abort", move || {
            let captured = capture_generation();
            let abort_seen = is_aborted(captured);
            expansion_ran_clone.store(true, std::sync::atomic::Ordering::SeqCst);
            assert!(
                !abort_seen,
                "pool task must not inherit stale abort generation"
            );
        });

        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            expansion_ran.load(std::sync::atomic::Ordering::SeqCst),
            "expansion task must have executed"
        );
    }

    #[test]
    fn running_task_sees_new_abort_generation() {
        // A task captures gen, then an abort bumps the gen — the task
        // must detect the change.
        let captured = capture_generation();
        abort_injection();
        assert!(
            is_aborted(captured),
            "running task must detect generation change after abort"
        );
    }
}
