#[link(name = "CoreFoundation", kind = "framework")]
// SAFETY: The CoreFoundation run loop APIs are stable system APIs.
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> *mut std::ffi::c_void;
    fn CFRunLoopStop(rl: *mut std::ffi::c_void);
}

#[link(name = "CoreGraphics", kind = "framework")]
// SAFETY: The CoreGraphics event tap APIs are stable system APIs.
unsafe extern "C" {
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
}

#[derive(Copy, Clone)]
struct SendPtr(*mut std::ffi::c_void);

// SAFETY: CoreFoundation run loop references are thread-safe to send between threads.
unsafe impl Send for SendPtr {}

static MACOS_RUN_LOOP: std::sync::Mutex<Option<SendPtr>> = std::sync::Mutex::new(None);
static MACOS_EVENT_TAP: std::sync::Mutex<Option<SendPtr>> = std::sync::Mutex::new(None);

/// Record the current thread's run loop so `stop_run_loop` can interrupt `rdev::grab`.
pub(super) fn register_run_loop() {
    // SAFETY: CFRunLoopGetCurrent always returns a valid run loop reference for the current thread.
    let rl = unsafe { CFRunLoopGetCurrent() };
    if let Ok(mut lock) = MACOS_RUN_LOOP.lock() {
        *lock = Some(SendPtr(rl));
    }
}

/// Record the active event tap handle for auto-recovery.
#[allow(dead_code)]
pub fn register_event_tap(tap: *mut std::ffi::c_void) {
    if let Ok(mut lock) = MACOS_EVENT_TAP.lock() {
        *lock = Some(SendPtr(tap));
    }
}

/// Re-enable the active event tap if it was disabled by timeout or user input.
#[allow(dead_code)]
pub fn reenable_active_event_tap() -> bool {
    if let Ok(lock) = MACOS_EVENT_TAP.lock()
        && let Some(SendPtr(tap)) = *lock
    {
        // SAFETY: CGEventTapEnable is safe to call with a valid or previously valid Mach port tap handle.
        unsafe {
            CGEventTapEnable(tap, true);
        }
        return true;
    }
    false
}

/// Stop the registered run loop, if any, to tear down the listener.
pub(super) fn stop_run_loop() {
    // SAFETY: CFRunLoopStop is safe to call from any thread with a valid CFRunLoopRef.
    if let Ok(mut lock) = MACOS_RUN_LOOP.lock()
        && let Some(SendPtr(rl)) = lock.take()
    {
        unsafe {
            CFRunLoopStop(rl);
        }
    }
}
