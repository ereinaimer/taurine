#[link(name = "CoreFoundation", kind = "framework")]
// SAFETY: The CoreFoundation run loop APIs are stable system APIs.
unsafe extern "C" {
    fn CFRunLoopGetCurrent() -> *mut std::ffi::c_void;
    fn CFRunLoopStop(rl: *mut std::ffi::c_void);
}

#[derive(Copy, Clone)]
struct SendPtr(*mut std::ffi::c_void);

// SAFETY: CoreFoundation run loop references are thread-safe to send between threads.
unsafe impl Send for SendPtr {}

static MACOS_RUN_LOOP: std::sync::Mutex<Option<SendPtr>> = std::sync::Mutex::new(None);

/// Record the current thread's run loop so `stop_run_loop` can interrupt `rdev::grab`.
pub(super) fn register_run_loop() {
    // SAFETY: CFRunLoopGetCurrent always returns a valid run loop reference for the current thread.
    let rl = unsafe { CFRunLoopGetCurrent() };
    if let Ok(mut lock) = MACOS_RUN_LOOP.lock() {
        *lock = Some(SendPtr(rl));
    }
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
