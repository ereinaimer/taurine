#![cfg(windows)]

//! Windows Vectored Exception Handling (VEH) filter for crash telemetry and immediate recovery.
//!
//! Intercepts unhandled native access violations (0xC0000005), illegal instructions,
//! and hardware faults before Windows Error Reporting (WER) displays a blocking modal dialog.
//! Immediately logs diagnostics and exits non-zero, allowing the OS Task Scheduler
//! restart policy to resurrect the daemon cleanly.

use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::System::Diagnostics::Debug::{
    AddVectoredExceptionHandler, EXCEPTION_POINTERS,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

const EXCEPTION_CONTINUE_SEARCH: i32 = 0;
const STATUS_ACCESS_VIOLATION: u32 = 0xC0000005;
const STATUS_IN_PAGE_ERROR: u32 = 0xC0000006;
const STATUS_ILLEGAL_INSTRUCTION: u32 = 0xC000001D;
const STATUS_INTEGER_DIVIDE_BY_ZERO: u32 = 0xC0000094;
const STATUS_STACK_OVERFLOW: u32 = 0xC00000FD;
const STATUS_HEAP_CORRUPTION: u32 = 0xC0000374;

static VEH_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Native exception handler callback invoked directly by the Windows kernel.
// SAFETY: Vectored exception handler callback required by Win32 AddVectoredExceptionHandler.
// The OS passes a valid or null pointer to an EXCEPTION_POINTERS structure on the faulting thread stack.
unsafe extern "system" fn vectored_exception_handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    if info.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    // SAFETY: Dereferencing info is safe because we checked for null above.
    let record = unsafe { (*info).ExceptionRecord };
    if record.is_null() {
        return EXCEPTION_CONTINUE_SEARCH;
    }

    // SAFETY: Dereferencing record is safe because we checked for null above.
    let code = unsafe { (*record).ExceptionCode } as u32;
    // SAFETY: Dereferencing record is safe because we checked for null above.
    let address = unsafe { (*record).ExceptionAddress } as usize;

    match code {
        STATUS_ACCESS_VIOLATION
        | STATUS_IN_PAGE_ERROR
        | STATUS_ILLEGAL_INSTRUCTION
        | STATUS_INTEGER_DIVIDE_BY_ZERO
        | STATUS_STACK_OVERFLOW
        | STATUS_HEAP_CORRUPTION => {
            let name = match code {
                STATUS_ACCESS_VIOLATION => "STATUS_ACCESS_VIOLATION (0xC0000005)",
                STATUS_IN_PAGE_ERROR => "STATUS_IN_PAGE_ERROR (0xC0000006)",
                STATUS_ILLEGAL_INSTRUCTION => "STATUS_ILLEGAL_INSTRUCTION (0xC000001D)",
                STATUS_INTEGER_DIVIDE_BY_ZERO => "STATUS_INTEGER_DIVIDE_BY_ZERO (0xC0000094)",
                STATUS_STACK_OVERFLOW => "STATUS_STACK_OVERFLOW (0xC00000FD)",
                STATUS_HEAP_CORRUPTION => "STATUS_HEAP_CORRUPTION (0xC0000374)",
                _ => "UNKNOWN_NATIVE_EXCEPTION",
            };

            eprintln!(
                "[FATAL] Native OS exception caught by Taurine VEH filter: {} at address 0x{:X}",
                name, address
            );

            // Terminate the process unconditionally with code 101.
            // TerminateProcess stops all threads immediately without executing DLL detach routines
            // (DLL_PROCESS_DETACH), avoiding loader lock deadlocks on corrupted heaps and ensuring
            // an immediate, non-blocking handoff to Windows Task Scheduler.
            // SAFETY: TerminateProcess terminates the process immediately and unconditionally.
            unsafe {
                let current = GetCurrentProcess();
                TerminateProcess(current, 101);
            }
            0
        }
        _ => EXCEPTION_CONTINUE_SEARCH,
    }
}

/// Initializes the Windows Vectored Exception Handler (VEH) filter once per process lifecycle.
pub fn init_veh_handler() {
    if VEH_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }

    // SAFETY: AddVectoredExceptionHandler registers our first-chance exception handler callback
    // with the Windows kernel. The handler conforms to the system ABI.
    unsafe {
        let handle = AddVectoredExceptionHandler(1, Some(vectored_exception_handler));
        if handle.is_null() {
            tracing::warn!("Failed to install Windows Vectored Exception Handler");
        } else {
            tracing::debug!("Windows Vectored Exception Handler installed successfully");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_veh_handler_idempotent() {
        init_veh_handler();
        init_veh_handler();
        assert!(VEH_INITIALIZED.load(Ordering::SeqCst));
    }
}
