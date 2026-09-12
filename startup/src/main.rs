#![windows_subsystem = "windows"]

use std::env;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

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

    let _ = Command::new(data_dir.join("bin").join("taurine.exe"))
        .arg("--daemon")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}
