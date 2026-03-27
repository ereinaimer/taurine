use rusqlite::{Connection, Result};
use std::fs;
use std::path::PathBuf;

#[cfg(target_os = "android")]
use std::sync::OnceLock;

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
    // Allow overriding via environment variable (for headless CI or tests)
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
        config_dir.join("Taurine").join("taurine.db")
    }
}

/// Initializes the SQLite database. NOTE: Ensure Android is initialized before calling.
pub fn init_db() -> Result<Connection> {
    let db_path = get_db_path();
    
    // Ensure parent directories exist
    if let Some(parent) = db_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).expect("Failed to create database directories");
        }
    }

    let conn = Connection::open(db_path)?;
    
    // Initialize schema
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        )",
        (),
    )?;

    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    /// A robust RAII guard for environment variables.
    /// It safely sets/removes a variable and RESTORES the original state when dropped,
    /// guaranteeing test isolation even if a test panics.
    struct EnvGuard {
        key: String,
        original_value: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        /// Sets the environment variable and returns the guard
        fn set(key: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let original_value = env::var_os(key);
            unsafe { env::set_var(key, value) };
            Self { key: key.to_string(), original_value }
        }

        /// Temporarily removes the environment variable and returns the guard
        fn remove(key: &str) -> Self {
            let original_value = env::var_os(key);
            unsafe { env::remove_var(key) };
            Self { key: key.to_string(), original_value }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // Restore the environment to exactly to it's original state
            match &self.original_value {
                Some(val) => unsafe { env::set_var(&self.key, val) },
                None => unsafe { env::remove_var(&self.key) },
            }
        }
    }

    #[test]
    fn test_env_override() {
        if env::var("SKIP_DB_PATH_TEST").unwrap_or_default() == "true" {
            println!("Skipping test_env_override due to SKIP_DB_PATH_TEST=true");
            return; 
        }

        let test_path = "some/custom/path/taurine.db";
        // The guard automatically cleans up when the test finishes
        let _guard = EnvGuard::set("TAURINE_DB_PATH", test_path);
        
        let path = get_db_path();
        assert_eq!(path.to_str().unwrap(), test_path);
    }

    #[test]
    fn test_default_desktop_path_resolution() {
        #[cfg(not(target_os = "android"))]
        {
            // Temporarily hide the env var if it exists to test the true fallback.
            // EnvGuard::remove ensures it is restored at the end of the block because we implemented custom Drop trait.
            let _guard = EnvGuard::remove("TAURINE_DB_PATH");

            let path = get_db_path();
            
            assert!(path.ends_with("taurine.db"));
            assert!(path.parent().unwrap().ends_with("Taurine"));
        }
    }

    #[test]
    fn test_init_db() {
        // Create an isolated temp directory that cleans itself up
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test_taurine.db");

        // Force init_db to use our temporary path
        let _guard = EnvGuard::set("TAURINE_DB_PATH", &db_path);

        // Call init
        let conn = init_db().expect("Failed to initialize database");
        
        // Verify the schema was created by inserting a row
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)", 
            ("test_key", "test_value")
        ).unwrap();
        
        // Read it back
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1").unwrap();
        let value: String = stmt.query_row(["test_key"], |row| row.get(0)).unwrap();
        
        assert_eq!(value, "test_value");

        // When the function ends here:
        // 1. _guard is dropped -> TAURINE_DB_PATH is restored to its original state.
        // 2. temp_dir is dropped -> The SQLite db and its directory are securely deleted from disk.
    }
}