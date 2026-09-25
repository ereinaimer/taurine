#![windows_subsystem = "windows"]

use std::env;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const CREATE_NO_WINDOW: u32 = 0x08000000;
const BOOT_REG_KEY: &str = r"Software\Taurine";
const BOOT_VALUE: &str = "StartupExe";

/// Reads a DEV-only environment override. Release builds ignore it, so the
/// logon launcher always targets the fixed install location.
/// Mirrors `taurine_core::paths::dev_env_var` — this crate is standalone and
/// cannot depend on core, keep the two in sync.
fn dev_env_var(name: &str) -> Option<String> {
    #[cfg(debug_assertions)]
    {
        env::var(name).ok().filter(|v| !v.trim().is_empty())
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = name;
        None
    }
}

/// Path recorded by the last `taurine up`: the exact binary to boot.
/// Returns `None` when unset or pointing at a file that no longer exists,
/// so callers fall back to the fixed install location.
fn read_boot_target() -> Option<PathBuf> {
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(BOOT_REG_KEY)
        .ok()?;
    let val: String = key.get_value(BOOT_VALUE).ok()?;
    let path = PathBuf::from(val.trim());
    if path.is_file() { Some(path) } else { None }
}

fn candidates_with(pinned: Option<PathBuf>, fallback: PathBuf) -> Vec<PathBuf> {
    match pinned {
        Some(p) if p != fallback && p.is_file() => vec![p, fallback],
        _ => vec![fallback],
    }
}

#[allow(clippy::unused_unit)] // pinned `-> ()` per Task 3 brief; `let _: () =` test enforces it
fn log_boot_event(data_dir: &Path, msg: &str) -> () {
    let _ = (|| -> std::io::Result<()> {
        let log = data_dir.join("logs").join("boot-launcher.log");
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?;
        writeln!(f, "[{secs}] {msg}")?;
        Ok(())
    })();
}

fn main() {
    // Single fixed install location. Mirrors `get_install_exe_path` in
    // core/src/system/paths.rs (duplicated: this crate cannot depend on core).
    let data_dir: PathBuf = match dev_env_var("TAURINE_DATA_DIR") {
        Some(dir) => dir.into(),
        None => match env::var("LOCALAPPDATA") {
            Ok(local) => PathBuf::from(local).join("Taurine"),
            Err(_) => return,
        },
    };

    let fallback = data_dir.join("bin").join("taurine.exe");
    let candidates = candidates_with(read_boot_target(), fallback);
    for target in candidates {
        log_boot_event(&data_dir, &format!("attempt: {}", target.display()));
        match Command::new(&target)
            .arg("--daemon")
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(_) => return,
            Err(e) => log_boot_event(&data_dir, &format!("failed: {}: {e}", target.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Unique temp dir (pid + counter, race-free across parallel runs),
    /// removed on drop on every exit path including panics.
    struct TestDir {
        path: std::path::PathBuf,
    }

    impl TestDir {
        fn new(name: &str) -> Self {
            let n = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!("{name}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).expect("temp dir must be created");
            Self { path }
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn candidates_prefer_live_pin_else_fallback() {
        let dir = TestDir::new("taurine_startup_candidates_test");
        let live = dir.path.join("live.exe");
        std::fs::write(&live, b"x").expect("live pin must be written");
        let missing = dir.path.join("missing.exe");
        let fb = dir.path.join("fallback.exe");

        let with_live = candidates_with(Some(live.clone()), fb.clone());
        assert!(!with_live.is_empty(), "candidates must not be empty");
        assert_eq!(with_live[0], live);
        assert_eq!(candidates_with(Some(missing), fb.clone()), vec![fb.clone()]);
        assert_eq!(candidates_with(None, fb.clone()), vec![fb]);
    }

    #[test]
    fn boot_event_logged_to_file() {
        let dir = TestDir::new("taurine_startup_boot_log_test");

        let _: () = log_boot_event(&dir.path, "hello-boot");

        let content = std::fs::read_to_string(dir.path.join("logs").join("boot-launcher.log"))
            .expect("boot log must exist");
        assert!(
            content.contains("hello-boot"),
            "boot log must contain the event"
        );
    }
}
