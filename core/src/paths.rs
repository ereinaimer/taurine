use crate::constants::{APP_NAME, DB_FILENAME, LOGS_DIR_NAME, STARTUP_DIR_NAME};
use std::fs;
use std::path::PathBuf;
use tracing::debug;

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
        ANDROID_DATA_PATH
            .get()
            .expect("Android data path must be initialized via init_android_path() from Flutter before use!")
            .clone()
    }

    #[cfg(not(target_os = "android"))]
    {
        let base_dirs = directories::BaseDirs::new().expect("Failed to get base directories");
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
        fs::create_dir_all(&data_dir).expect("Failed to create app data directories");
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
        ANDROID_DATA_PATH
            .get()
            .expect("Android data path must be initialized via init_android_path() from Flutter before use!")
            .join(DB_FILENAME)
    }

    #[cfg(not(target_os = "android"))]
    {
        let db_path = get_data_dir().join(DB_FILENAME);
        debug!("Database path: {}", db_path.display());
        db_path
    }
}

/// Resolves the logs directory path.
pub fn logs_dir() -> PathBuf {
    let data_dir = ensure_data_dir();
    data_dir.join(LOGS_DIR_NAME)
}

/// Resolves the exact file path for the daemon startup VBScript.
pub fn get_startup_vbs_path() -> PathBuf {
    let data_dir = ensure_data_dir();
    let startup_dir = data_dir.join(STARTUP_DIR_NAME);
    if !startup_dir.exists() {
        debug!(
            "Creating {} startup directory: {}",
            APP_NAME,
            startup_dir.display()
        );
        fs::create_dir_all(&startup_dir).expect("Failed to create startup directory");
    }
    startup_dir.join("daemon-startup.vbs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_db_env_override() {
        crate::testing::init_tracing_for_tests();
        // Skip this test via an env var override
        if env::var("TAURINE_SKIP_DB_ENV_OVERRIDE_TEST").unwrap_or_default() == "true" {
            return;
        }

        let test_path = "some/custom/path/taurine.db";
        unsafe { env::set_var("TAURINE_DB_PATH", test_path) };

        let path = get_db_path();
        assert_eq!(path.to_str().unwrap(), test_path);

        unsafe { env::remove_var("TAURINE_DB_PATH") };
    }

    #[test]
    fn test_data_dir_env_override() {
        crate::testing::init_tracing_for_tests();
        let test_dir = "some/custom/app_dir";
        unsafe { env::set_var("TAURINE_DATA_DIR", test_dir) };

        let path = get_data_dir();
        assert_eq!(path.to_str().unwrap(), test_dir);

        unsafe { env::remove_var("TAURINE_DATA_DIR") };
    }

    #[test]
    fn test_default_desktop_path_resolution() {
        crate::testing::init_tracing_for_tests();
        // Skip this test via an env var override
        if env::var("TAURINE_SKIP_DEFAULT_PATH_RESOLUTION_TEST").unwrap_or_default() == "true" {
            return;
        }

        #[cfg(not(target_os = "android"))]
        {
            let backup_env = env::var("TAURINE_DB_PATH").ok();
            unsafe { env::remove_var("TAURINE_DB_PATH") };

            let db_path = get_db_path();
            let data_dir = get_data_dir();

            assert!(db_path.ends_with("taurine.db"));
            assert_eq!(db_path.parent().unwrap(), data_dir.as_path());

            #[cfg(any(target_os = "windows", target_os = "macos"))]
            assert!(data_dir.ends_with("Taurine") || data_dir.ends_with("taurine"));

            #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
            assert!(data_dir.ends_with("taurine"));

            if let Some(val) = backup_env {
                unsafe { env::set_var("TAURINE_DB_PATH", val) };
            }
        }
    }

    #[test]
    fn test_startup_vbs_path_creation() {
        crate::testing::init_tracing_for_tests();

        let test_dir = std::env::temp_dir().join("taurine_vbs_test");
        unsafe { env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };

        let vbs_path = get_startup_vbs_path();
        assert!(vbs_path.ends_with("daemon-startup.vbs"));

        let startup_dir = vbs_path.parent().unwrap();
        assert!(startup_dir.ends_with("startup"));
        assert!(startup_dir.exists());

        // Cleanup
        let _ = fs::remove_dir_all(&test_dir);
        unsafe { env::remove_var("TAURINE_DATA_DIR") };
    }
}
