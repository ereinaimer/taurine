//! Panic isolation and unwinding helpers for background worker threads.

use std::panic::{self, UnwindSafe};

/// Catches panics in a background worker thread closure, logs the error, and returns a Result.
///
/// This isolates background threads (such as updater, audio, clipboard, or AI streaming)
/// so that an unexpected panic in a background task does not crash the entire daemon.
pub fn catch_worker_panic<F, R>(worker_name: &str, f: F) -> Result<R, String>
where
    F: FnOnce() -> R + UnwindSafe,
{
    match panic::catch_unwind(f) {
        Ok(val) => Ok(val),
        Err(err) => {
            let err_msg = if let Some(s) = err.downcast_ref::<&str>() {
                (*s).to_string()
            } else if let Some(s) = err.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic payload".to_string()
            };
            tracing::error!(worker = %worker_name, error = %err_msg, "Background worker panicked");
            Err(err_msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catch_worker_panic_ok() {
        let res = catch_worker_panic("test-ok", || 42);
        assert_eq!(res, Ok(42));
    }

    #[test]
    fn test_catch_worker_panic_str_payload() {
        let res = catch_worker_panic("test-panic-str", || {
            panic!("worker failed with static str");
        });
        assert_eq!(res, Err("worker failed with static str".to_string()));
    }

    #[test]
    fn test_catch_worker_panic_string_payload() {
        let res = catch_worker_panic("test-panic-string", || {
            panic!("worker failed with code: {}", 500);
        });
        assert_eq!(res, Err("worker failed with code: 500".to_string()));
    }

    #[test]
    fn test_catch_worker_panic_unknown_payload() {
        let res = catch_worker_panic("test-panic-unknown", || {
            panic::panic_any(12345);
        });
        assert_eq!(res, Err("unknown panic payload".to_string()));
    }
}
