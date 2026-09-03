// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

#[cfg(target_os = "windows")]
mod platform {
    use std::ptr;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::{CreateMutexW, OpenMutexW};

    const SYNCHRONIZE: u32 = 0x00100000;

    fn get_mutex_name_wide() -> Vec<u16> {
        let name = std::env::var("TAURINE_SERVICE_LIVENESS_NAME")
            .unwrap_or_else(|_| r"Local\TaurineServiceLiveness".to_string());
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// RAII guard holding the active service liveness mutex handle.
    #[derive(Debug)]
    pub struct ServiceLivenessGuard {
        handle: HANDLE,
    }

    // SAFETY: The Win32 mutex HANDLE is safe to send between threads.
    unsafe impl Send for ServiceLivenessGuard {}
    // SAFETY: The Win32 mutex HANDLE is safe to share references across threads.
    unsafe impl Sync for ServiceLivenessGuard {}

    impl Drop for ServiceLivenessGuard {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                // SAFETY: self.handle is a non-null, valid open Win32 handle created by CreateMutexW.
                unsafe {
                    CloseHandle(self.handle);
                }
                self.handle = ptr::null_mut();
            }
        }
    }

    /// Attempts to acquire service liveness for the current process.
    /// Returns `Some(ServiceLivenessGuard)` if this is the only active service instance,
    /// or `None` if another service instance is already running.
    pub fn acquire_service_liveness() -> Option<ServiceLivenessGuard> {
        let wide_name = get_mutex_name_wide();

        // SAFETY: We provide null security attributes, false for initial ownership,
        // and a valid null-terminated UTF-16 wide string pointer.
        let handle = unsafe { CreateMutexW(ptr::null(), 0, wide_name.as_ptr()) };
        if handle.is_null() {
            return None;
        }

        // SAFETY: Calling GetLastError immediately after CreateMutexW to check if the mutex already existed.
        let last_error = unsafe { windows_sys::Win32::Foundation::GetLastError() };
        if last_error == ERROR_ALREADY_EXISTS {
            // Another service instance created and owns the mutex object.
            // SAFETY: Closing the duplicate handle.
            unsafe {
                CloseHandle(handle);
            }
            return None;
        }

        Some(ServiceLivenessGuard { handle })
    }

    /// Queries whether the background service is currently running in memory with zero disk I/O.
    pub fn is_service_running() -> bool {
        let wide_name = get_mutex_name_wide();

        // SAFETY: We pass SYNCHRONIZE access, false for handle inheritance,
        // and a valid null-terminated UTF-16 wide string pointer.
        let handle = unsafe { OpenMutexW(SYNCHRONIZE, 0, wide_name.as_ptr()) };
        if handle.is_null() {
            return false;
        }

        // SAFETY: handle is a valid Win32 handle opened via OpenMutexW; closing it immediately.
        unsafe {
            CloseHandle(handle);
        }
        true
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use std::fs::{File, OpenOptions};
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    use std::path::PathBuf;

    fn get_lock_path() -> PathBuf {
        if let Ok(custom) = std::env::var("TAURINE_SERVICE_LIVENESS_PATH")
            && !custom.trim().is_empty()
        {
            PathBuf::from(custom)
        } else if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR")
            && !runtime_dir.trim().is_empty()
        {
            PathBuf::from(runtime_dir).join("taurine.lock")
        } else {
            let data_dir = crate::paths::get_data_dir();
            let _ = std::fs::create_dir_all(&data_dir);
            data_dir.join("taurine.lock")
        }
    }

    /// RAII guard holding the open lockfile descriptor.
    #[derive(Debug)]
    pub struct ServiceLivenessGuard {
        _file: File,
    }

    /// Attempts to acquire service liveness for the current process.
    /// Returns `Some(ServiceLivenessGuard)` on success, or `None` if another process holds the lock.
    pub fn acquire_service_liveness() -> Option<ServiceLivenessGuard> {
        let lock_path = get_lock_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&lock_path)
            .ok()?;

        let fd = file.as_raw_fd();
        // SAFETY: fd is a valid open file descriptor. flock with LOCK_EX | LOCK_NB performs a non-blocking lock.
        let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if res != 0 {
            return None;
        }

        Some(ServiceLivenessGuard { _file: file })
    }

    /// Queries whether the background service is currently running in memory with zero disk I/O.
    pub fn is_service_running() -> bool {
        let lock_path = get_lock_path();
        let file = match OpenOptions::new().read(true).write(true).open(&lock_path) {
            Ok(f) => f,
            Err(_) => return false,
        };

        let fd = file.as_raw_fd();
        // SAFETY: fd is a valid open file descriptor.
        let res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if res == 0 {
            // We acquired the lock, meaning no active service is running.
            // SAFETY: Releasing the advisory lock immediately.
            unsafe {
                libc::flock(fd, libc::LOCK_UN);
            }
            false
        } else {
            // Lock is held by another process.
            let err = std::io::Error::last_os_error().raw_os_error();
            err == Some(libc::EWOULDBLOCK) || err == Some(libc::EAGAIN)
        }
    }
}

pub use platform::{ServiceLivenessGuard, acquire_service_liveness, is_service_running};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_liveness_lifecycle() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let test_id = uuid::Uuid::new_v4().to_string();

        #[cfg(target_os = "windows")]
        unsafe {
            std::env::set_var(
                "TAURINE_SERVICE_LIVENESS_NAME",
                format!("Local\\TaurineTestLiveness_{}", test_id),
            );
        }

        #[cfg(not(target_os = "windows"))]
        let temp_dir = tempfile::tempdir().unwrap();
        #[cfg(not(target_os = "windows"))]
        let lock_file = temp_dir
            .path()
            .join(format!("taurine_test_{}.lock", test_id));
        #[cfg(not(target_os = "windows"))]
        unsafe {
            std::env::set_var(
                "TAURINE_SERVICE_LIVENESS_PATH",
                lock_file.to_string_lossy().to_string(),
            );
        }

        // 1. Before acquisition: service should not be running
        assert!(
            !is_service_running(),
            "Service should not be running initially"
        );

        // 2. Acquire liveness
        let liveness = acquire_service_liveness();
        assert!(
            liveness.is_some(),
            "Should successfully acquire liveness guard"
        );

        // 3. While guard is alive: service should be running
        assert!(
            is_service_running(),
            "Service should be reported as running while guard is held"
        );

        // 4. Duplicate acquisition should fail
        let duplicate = acquire_service_liveness();
        assert!(
            duplicate.is_none(),
            "Duplicate acquisition should return None"
        );

        // 5. Drop guard: service should no longer be running
        drop(liveness);
        assert!(
            !is_service_running(),
            "Service should no longer be running after guard drop"
        );

        #[cfg(target_os = "windows")]
        unsafe {
            std::env::remove_var("TAURINE_SERVICE_LIVENESS_NAME");
        }
        #[cfg(not(target_os = "windows"))]
        unsafe {
            std::env::remove_var("TAURINE_SERVICE_LIVENESS_PATH");
        }
    }

    #[test]
    fn test_concurrent_liveness_queries() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        let test_id = uuid::Uuid::new_v4().to_string();

        #[cfg(target_os = "windows")]
        unsafe {
            std::env::set_var(
                "TAURINE_SERVICE_LIVENESS_NAME",
                format!("Local\\TaurineTestLiveness_Conc_{}", test_id),
            );
        }

        #[cfg(not(target_os = "windows"))]
        let temp_dir = tempfile::tempdir().unwrap();
        #[cfg(not(target_os = "windows"))]
        let lock_file = temp_dir
            .path()
            .join(format!("taurine_test_conc_{}.lock", test_id));
        #[cfg(not(target_os = "windows"))]
        unsafe {
            std::env::set_var(
                "TAURINE_SERVICE_LIVENESS_PATH",
                lock_file.to_string_lossy().to_string(),
            );
        }

        let guard = acquire_service_liveness();
        assert!(guard.is_some());

        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..50 {
                        assert!(is_service_running());
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        drop(guard);
        assert!(!is_service_running());

        #[cfg(target_os = "windows")]
        unsafe {
            std::env::remove_var("TAURINE_SERVICE_LIVENESS_NAME");
        }
        #[cfg(not(target_os = "windows"))]
        unsafe {
            std::env::remove_var("TAURINE_SERVICE_LIVENESS_PATH");
        }
    }
}
