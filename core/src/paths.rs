#[cfg(target_os = "android")]
use std::sync::OnceLock;
use std::path::PathBuf;

#[cfg(target_os = "android")]
static ANDROID_DB_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the path for Android. This must be called from Flutter
/// via flutter_rust_bridge before any database operations
#[cfg(target_os = "android")]
pub fn init_android_path(path: String) {
    let _ = ANDROID_DB_PATH.set(PathBuf::from(path));
}

/// Resolves the database path across different operating systems
/// - Windows: %APPDATA%\Taurine\taurine.db
/// - macOS/Linux: App-specific config directory (e.g., ~/.config/Taurine/taurine.db)
/// - Android: Path provided by Flutter initialization
pub fn get_db_path() -> PathBuf {
    // Allow overriding via environment variable (for headless CI)
    if let Ok(env_path) = std::env::var("TAURINE_DB_PATH") {
        return PathBuf::from(env_path);
    }

    #[cfg(target_os = "android")]
    {
        ANDROID_DB_PATH.get().expect("Android database path must be initialized via init_android_path() from Flutter before use!").clone()
    }

    #[cfg(not(target_os = "android"))]
    {
        let base_dirs = directories::BaseDirs::new().expect("Failed to get base directories");
        // config_dir evaluates to %APPDATA% on Windows, ~/.config on Linux, and ~/Library/Application Support on macOS.
        let config_dir = base_dirs.config_dir(); 

        #[cfg(any(target_os = "windows", target_os = "macos"))]
        let app_folder = "Taurine";

        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
        let app_folder = "taurine";

        config_dir.join(app_folder).join("taurine.db")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_env_override() {
        // Dynamically skip this test if the environment variable is set
        if env::var("SKIP_DB_PATH_TEST").unwrap_or_default() == "true" {
            println!("Skipping test_env_override due to SKIP_DB_PATH_TEST=true");
            return; 
        }

        let test_path = "some/custom/path/taurine.db";
        unsafe { env::set_var("TAURINE_DB_PATH", test_path) };
        
        let path = get_db_path();
        assert_eq!(path.to_str().unwrap(), test_path);
        
        unsafe { env::remove_var("TAURINE_DB_PATH") };
    }

    #[test]
    fn test_default_desktop_path_resolution() {
        // We only test the desktop resolution here. 
        // Android requires init_android_path() to be called first, which uses a global OnceLock.
        #[cfg(not(target_os = "android"))]
        {
            // Temporarily hide the env var if it exists, so we test the true default fallback
            let backup_env = env::var("TAURINE_DB_PATH").ok();
            unsafe { env::remove_var("TAURINE_DB_PATH") };

            let path = get_db_path();
            
            // We can't hardcode the exact path because it varies by developer machine (e.g., /Users/name vs C:\Users\name)
            // But we CAN verify it ends with the correct subdirectories.
            assert!(path.ends_with("taurine.db"));

            #[cfg(any(target_os = "windows", target_os = "macos"))]
            assert!(path.parent().unwrap().ends_with("Taurine"));

            #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "android")))]
            assert!(path.parent().unwrap().ends_with("taurine"));

            // Restore the env var if it was there
            if let Some(val) = backup_env {
                unsafe { env::set_var("TAURINE_DB_PATH", val) };
            }
        }
    }
}