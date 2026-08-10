#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
use crate::constants::APP_NAME_SLUG;
use crate::constants::{APP_NAME, BIN_DIR_NAME, DB_FILENAME, LOGS_DIR_NAME, STARTUP_DIR_NAME};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, error};

#[cfg(target_os = "android")]
use std::sync::OnceLock;

#[cfg(target_os = "android")]
static ANDROID_DATA_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Initializes the app data directory for Android. This must be called from Flutter
/// via flutter_rust_bridge before any file-system operations.
/// Pass the app's internal data directory (e.g. context.filesDir.absolutePath).
#[cfg(target_os = "android")]
pub fn init_android_path(data_dir: String) {
    let _ = ANDROID_DATA_PATH.set(PathBuf::from(data_dir));
}

/// Resolves the base data directory for the app.
/// - Windows:  %LOCALAPPDATA%\APP_NAME
/// - macOS:    ~/Library/Application Support/APP_NAME
/// - Linux:    ~/.local/share/APP_NAME_SLUG
/// - Android:  Directory provided via `init_android_path()`.
pub fn get_data_dir() -> PathBuf {
    // Allow overriding via environment variable (for headless CI or tests)
    if let Ok(env_path) = std::env::var("TAURINE_DATA_DIR") {
        debug!("TAURINE_DATA_DIR override enabled");
        return PathBuf::from(env_path);
    }

    #[cfg(target_os = "android")]
    {
        match ANDROID_DATA_PATH.get() {
            Some(path) => path.clone(),
            None => {
                tracing::error!(
                    "Android data path not initialized via init_android_path() from Flutter before use; falling back to current directory"
                );
                PathBuf::from(".")
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        let base_dirs = match directories::BaseDirs::new() {
            Some(dir) => dir,
            None => {
                tracing::error!(
                    "Failed to resolve user data directories; falling back to current directory"
                );
                return PathBuf::from(".");
            }
        };
        let data_dir = base_dirs.data_local_dir();

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let app_folder = APP_NAME;

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
        let app_folder = APP_NAME_SLUG;

        data_dir.join(app_folder)
    }
}

/// The combined Ensure function.
///
/// Gets the app data directory and guarantees it exists on disk.
/// Call this when a safe place to write data is needed.
pub fn ensure_data_dir() -> PathBuf {
    let data_dir = get_data_dir();

    if !data_dir.exists() {
        debug!(
            "Creating {} data directory: {}",
            APP_NAME,
            data_dir.display()
        );
        if let Err(e) = fs::create_dir_all(&data_dir) {
            tracing::error!(
                "Failed to create {} data directory {}: {}",
                APP_NAME,
                data_dir.display(),
                e
            );
        }

        #[cfg(all(unix, not(target_os = "android")))]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(&data_dir) {
                let mut perms = metadata.permissions();
                if perms.mode() & 0o777 != 0o700 {
                    perms.set_mode(0o700);
                    if let Err(e) = fs::set_permissions(&data_dir, perms) {
                        debug!(
                            "Failed to set permissions 0700 on app data directory: {}",
                            e
                        );
                    }
                }
            }
        }
    }

    data_dir
}

/// Resolves the exact file path for the SQLite database.
pub fn get_db_path() -> PathBuf {
    // DB path override
    if let Ok(env_path) = std::env::var("TAURINE_DB_PATH") {
        debug!("TAURINE_DB_PATH override enabled");
        return PathBuf::from(env_path);
    }

    #[cfg(target_os = "android")]
    {
        // On Android the data dir is provided directly; append the db file name.
        match ANDROID_DATA_PATH.get() {
            Some(path) => path.join(DB_FILENAME),
            None => {
                tracing::error!(
                    "Android data path not initialized via init_android_path() from Flutter before use; falling back to current directory"
                );
                PathBuf::from(".").join(DB_FILENAME)
            }
        }
    }

    #[cfg(not(target_os = "android"))]
    {
        get_data_dir().join(DB_FILENAME)
    }
}

/// Resolves the logs directory path.
pub fn logs_dir() -> PathBuf {
    let data_dir = ensure_data_dir();
    data_dir.join(LOGS_DIR_NAME)
}

/// Resolves the app cache directory.
/// - Windows:  %LOCALAPPDATA%\APP_NAME\cache
/// - macOS:    ~/Library/Application Support/APP_NAME/cache
/// - Linux:    ~/.local/share/APP_NAME_SLUG/cache
/// - Android:  `ANDROID_DATA_PATH/cache`
pub fn get_cache_dir() -> PathBuf {
    get_data_dir().join("cache")
}

/// Gets the app cache directory and guarantees it exists on disk.
pub fn ensure_cache_dir() -> PathBuf {
    let cache_dir = get_cache_dir();
    if !cache_dir.exists() {
        debug!(
            "Creating {} cache directory: {}",
            APP_NAME,
            cache_dir.display()
        );
        if let Err(e) = fs::create_dir_all(&cache_dir) {
            error!("Failed to create app cache directory: {}", e);
        }
    }

    #[cfg(all(unix, not(target_os = "android")))]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&cache_dir) {
            let mut perms = metadata.permissions();
            if perms.mode() & 0o777 != 0o700 {
                perms.set_mode(0o700);
                if let Err(e) = fs::set_permissions(&cache_dir, perms) {
                    debug!(
                        "Failed to set permissions 0700 on app cache directory: {}",
                        e
                    );
                }
            }
        }
    }

    cache_dir
}

/// Resolves the app temp directory.
/// - Windows:  %LOCALAPPDATA%\APP_NAME\temp
/// - macOS:    ~/Library/Application Support/APP_NAME/temp
/// - Linux:    ~/.local/share/APP_NAME_SLUG/temp
/// - Android:  `ANDROID_DATA_PATH/temp`
pub fn get_temp_dir() -> PathBuf {
    get_data_dir().join("temp")
}

/// Gets the app temp directory and guarantees it exists on disk with 0700 permissions.
pub fn ensure_temp_dir() -> PathBuf {
    let temp_dir = get_temp_dir();
    if !temp_dir.exists() {
        debug!(
            "Creating {} temp directory: {}",
            APP_NAME,
            temp_dir.display()
        );
        if let Err(e) = fs::create_dir_all(&temp_dir) {
            tracing::error!(
                "Failed to create {} temp directory {}: {}",
                APP_NAME,
                temp_dir.display(),
                e
            );
        }
    }

    #[cfg(all(unix, not(target_os = "android")))]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(metadata) = fs::metadata(&temp_dir) {
            let mut perms = metadata.permissions();
            if perms.mode() & 0o777 != 0o700 {
                perms.set_mode(0o700);
                if let Err(e) = fs::set_permissions(&temp_dir, perms) {
                    debug!(
                        "Failed to set permissions 0700 on app temp directory: {}",
                        e
                    );
                }
            }
        }
    }

    temp_dir
}

/// Wipes all temporary files in the app temp directory and ensures 0700 permissions.
pub fn wipe_temp_dir() {
    let temp_dir = get_temp_dir();
    if temp_dir.exists() {
        debug!("Wiping {} temp directory: {}", APP_NAME, temp_dir.display());
        if let Err(e) = fs::remove_dir_all(&temp_dir) {
            tracing::warn!(
                "Failed to wipe temp directory {}: {}",
                temp_dir.display(),
                e
            );
        }
    }
    let _ = ensure_temp_dir();
}

/// Creates a new temporary file in the app temp directory with atomic create_new(true),
/// a random UUID suffix, and 0600 permissions.
pub fn create_temp_file(prefix: &str, ext: &str) -> std::io::Result<(PathBuf, std::fs::File)> {
    let dir = ensure_temp_dir();
    let filename = format!("{}_{}.{}", prefix, uuid::Uuid::new_v4(), ext);
    let path = dir.join(filename);

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);

    #[cfg(all(unix, not(target_os = "android")))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let file = options.open(&path)?;
    Ok((path, file))
}

/// Helper to atomically write binary content to a new secure temporary file.
pub fn write_temp_file(prefix: &str, ext: &str, content: &[u8]) -> std::io::Result<PathBuf> {
    use std::io::Write;
    let (path, mut file) = create_temp_file(prefix, ext)?;
    file.write_all(content)?;
    file.flush()?;
    Ok(path)
}

/// Resolves the exact file path for the daemon startup executable.
pub fn get_startup_exe_path() -> PathBuf {
    let data_dir = ensure_data_dir();
    let startup_dir = data_dir.join(STARTUP_DIR_NAME);
    if !startup_dir.exists() {
        debug!(
            "Creating {} startup directory: {}",
            APP_NAME,
            startup_dir.display()
        );
        if let Err(e) = fs::create_dir_all(&startup_dir) {
            tracing::error!(
                "Failed to create {} startup directory {}: {}",
                APP_NAME,
                startup_dir.display(),
                e
            );
        }
    }
    startup_dir.join("taurine-startup.exe")
}

/// Resolves the canonical directory where the taurine binary should be installed.
///
/// This is always a "bin/" subdirectory of get_data_dir():
/// - Windows:  %LOCALAPPDATA%\Taurine\bin\
/// - macOS:    ~/Library/Application Support/Taurine/bin/
/// - Linux:    ~/.local/share/taurine/bin/
///
/// Future GUI assets will go in get_data_dir().join("app"), keeping
/// all taurine files under a single parent directory per OS.
pub fn get_install_bin_dir() -> PathBuf {
    get_data_dir().join(BIN_DIR_NAME)
}

/// Resolves the canonical path of the taurine executable.
pub fn get_install_exe_path() -> PathBuf {
    let exe_name = if cfg!(target_os = "windows") {
        "taurine.exe"
    } else {
        "taurine"
    };
    get_install_bin_dir().join(exe_name)
}

/// Path to the file storing the last auto-update check timestamp (Unix seconds).
/// Located at get_cache_dir()/last_update_check
pub fn get_last_update_check_path() -> PathBuf {
    ensure_cache_dir().join("last_update_check")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    use crate::testing::TEST_LOCK;

    #[test]
    fn test_db_env_override() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();
        // Skip this test via an env var override
        if env::var("TAURINE_SKIP_DB_ENV_OVERRIDE_TEST").unwrap_or_default() == "true" {
            return;
        }

        let test_path = "some/custom/path/taurine.db";
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_DB_PATH", test_path) };

        let path = get_db_path();
        assert_eq!(path.to_str().unwrap(), test_path);

        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_DB_PATH") };
    }

    #[test]
    fn test_data_dir_env_override() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();
        let test_dir = "some/custom/app_dir";
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_DATA_DIR", test_dir) };

        let path = get_data_dir();
        assert_eq!(path.to_str().unwrap(), test_dir);

        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_DATA_DIR") };
    }

    #[test]
    fn test_default_desktop_path_resolution() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();
        // Skip this test via an env var override
        if env::var("TAURINE_SKIP_DEFAULT_PATH_RESOLUTION_TEST").unwrap_or_default() == "true" {
            return;
        }

        #[cfg(not(target_os = "android"))]
        {
            if directories::BaseDirs::new().is_none() {
                return;
            }

            let backup_db = env::var("TAURINE_DB_PATH").ok();
            let backup_data = env::var("TAURINE_DATA_DIR").ok();
            unsafe {
                env::remove_var("TAURINE_DB_PATH");
                env::remove_var("TAURINE_DATA_DIR");
            };

            let db_path = get_db_path();
            let data_dir = get_data_dir();

            assert!(db_path.ends_with("taurine.db"));
            assert_eq!(db_path.parent().unwrap(), data_dir.as_path());

            #[cfg(any(target_os = "windows", target_os = "macos"))]
            assert!(data_dir.ends_with("Taurine") || data_dir.ends_with("taurine"));

            #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
            assert!(data_dir.ends_with("taurine"));

            unsafe {
                if let Some(val) = backup_db {
                    env::set_var("TAURINE_DB_PATH", val);
                }
                if let Some(val) = backup_data {
                    env::set_var("TAURINE_DATA_DIR", val);
                }
            }
        }
    }

    #[test]
    fn test_startup_exe_path_creation() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();

        let test_dir = std::env::temp_dir().join("taurine_exe_test");
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        let exe_path = get_startup_exe_path();
        assert!(exe_path.ends_with("taurine-startup.exe"));

        let startup_dir = exe_path.parent().unwrap();
        assert!(startup_dir.ends_with("startup"));
        assert!(startup_dir.exists());

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir);
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_DATA_DIR") };
    }

    #[test]
    fn test_ensure_data_dir_permissions() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();

        let test_dir = std::env::temp_dir().join("taurine_perms_test");
        let _ = fs::remove_dir_all(&test_dir);

        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        let data_dir = ensure_data_dir();
        assert!(data_dir.exists());

        #[cfg(all(unix, not(target_os = "android")))]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&data_dir).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir);
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_DATA_DIR") };
    }

    #[test]
    fn test_ensure_cache_dir_permissions() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();

        let test_dir = std::env::temp_dir().join("taurine_cache_test");
        let _ = fs::remove_dir_all(&test_dir);

        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        let cache_dir = ensure_cache_dir();
        assert!(cache_dir.exists());
        assert!(cache_dir.ends_with("cache"));

        let update_path = get_last_update_check_path();
        assert_eq!(update_path.parent().unwrap(), cache_dir.as_path());
        assert!(update_path.ends_with("last_update_check"));

        #[cfg(all(unix, not(target_os = "android")))]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&cache_dir).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir);
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_DATA_DIR") };
    }

    #[test]
    fn test_ensure_cache_dir_error_handling() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();

        // Create a temporary file to block directory creation
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join(format!("taurine-test-file-{}", uuid::Uuid::new_v4()));
        fs::write(&file_path, "").unwrap();

        // Set TAURINE_DATA_DIR to a subdirectory of the file (which is invalid)
        let invalid_data_dir = file_path.join("invalid_dir");
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_DATA_DIR", &invalid_data_dir) };

        // Calling ensure_cache_dir should not panic, even though dir creation fails
        let cache_dir = ensure_cache_dir();
        assert_eq!(cache_dir, invalid_data_dir.join("cache"));

        // Clean up
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_DATA_DIR") };
        let _ = fs::remove_file(&file_path);
    }

    #[test]
    fn test_temp_dir_creation_wiping_and_file_security() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::testing::init_tracing_for_tests();

        let test_dir =
            std::env::temp_dir().join(format!("taurine_temp_sec_test_{}", uuid::Uuid::new_v4()));
        let _ = fs::remove_dir_all(&test_dir);

        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        let temp_dir = ensure_temp_dir();
        assert!(temp_dir.exists());
        assert!(temp_dir.ends_with("temp"));

        #[cfg(all(unix, not(target_os = "android")))]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&temp_dir).unwrap();
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }

        // Write a test file into temp_dir
        let file_path = write_temp_file("test_prefix", "txt", b"secret content")
            .expect("write temp file failed");
        assert!(file_path.exists());
        assert!(file_path.parent().unwrap().ends_with("temp"));
        assert!(
            file_path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("test_prefix_")
        );

        #[cfg(all(unix, not(target_os = "android")))]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_meta = fs::metadata(&file_path).unwrap();
            assert_eq!(file_meta.permissions().mode() & 0o777, 0o600);
        }

        // Verify wipe_temp_dir removes existing files
        wipe_temp_dir();
        assert!(ensure_temp_dir().exists());
        assert!(!file_path.exists());

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir);
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_DATA_DIR") };
    }
}
