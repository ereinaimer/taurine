use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use tracing::{error, trace};

pub static IS_INJECTING: AtomicBool = AtomicBool::new(false);
pub static INJECTION_ABORT: AtomicBool = AtomicBool::new(false);

pub(super) static INJECTION_SCOPE_DEPTH: AtomicUsize = AtomicUsize::new(0);
pub(super) static INJECTION_VISIBILITY_DEPTH: AtomicUsize = AtomicUsize::new(0);

#[allow(dead_code)]
pub static IS_SIMULATING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
pub(super) struct InjectionGate<'a> {
    is_injecting: &'a AtomicBool,
    abort: &'a AtomicBool,
    scope_depth: &'a AtomicUsize,
    visibility_depth: &'a AtomicUsize,
}

impl<'a> InjectionGate<'a> {
    pub(super) const fn new(
        is_injecting: &'a AtomicBool,
        abort: &'a AtomicBool,
        scope_depth: &'a AtomicUsize,
        visibility_depth: &'a AtomicUsize,
    ) -> Self {
        Self {
            is_injecting,
            abort,
            scope_depth,
            visibility_depth,
        }
    }

    pub(super) fn begin_scope(self) {
        let was_outermost_scope = self.scope_depth.fetch_add(1, Ordering::SeqCst) == 0;
        self.visibility_depth.fetch_add(1, Ordering::SeqCst);
        self.is_injecting.store(true, Ordering::SeqCst);
        if was_outermost_scope {
            self.abort.store(false, Ordering::SeqCst);
        }
    }

    pub(super) fn end_scope(self) {
        let previous_scope_depth = self.scope_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_scope_depth > 0, "scope depth underflow");
        let remaining_scope_depth = previous_scope_depth.saturating_sub(1);

        let previous_visibility_depth = self.visibility_depth.fetch_sub(1, Ordering::SeqCst);
        debug_assert!(previous_visibility_depth > 0, "visibility depth underflow");
        let remaining_visibility_depth = previous_visibility_depth.saturating_sub(1);

        self.is_injecting
            .store(remaining_visibility_depth > 0, Ordering::SeqCst);
        if remaining_scope_depth == 0 {
            self.abort.store(false, Ordering::SeqCst);
        }
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
        &INJECTION_ABORT,
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
            restored_abort = INJECTION_ABORT.load(Ordering::SeqCst),
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

pub fn abort_injection() {
    INJECTION_ABORT.store(true, Ordering::SeqCst);
}

pub(super) fn inject_mutex() -> &'static Mutex<()> {
    static M: OnceLock<Mutex<()>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(()))
}
