use reqwest::blocking::Client;
use sha2::Digest;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use taurine_core::error::{Error, Result};
use taurine_core::paths::{get_install_bin_dir, get_install_exe_path};
use taurine_core::settings::SpinnerStyle;
use taurine_core::utils::spinner::{SpinnerRenderer, ThreadSpinnerHandle, spawn_threaded};
use tracing::{error, info};

fn platform_key() -> &'static str {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-aarch64"
    } else {
        panic!("unsupported platform")
    }
}

#[derive(serde::Deserialize)]
struct Manifest {
    version: String,
    artifacts: std::collections::HashMap<String, Artifact>,
}

#[derive(serde::Deserialize)]
struct Artifact {
    url: String,
    #[serde(default)]
    sha256: Option<String>,
}

struct StdoutStepRenderer {
    label: String,
}

impl SpinnerRenderer for StdoutStepRenderer {
    fn inject_frame(&mut self, frame: &str) {
        print!("\r{frame} {}", self.label);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
    fn backspace(&mut self, _: usize) {}
    fn move_left(&mut self, _: usize) {}
    fn move_right(&mut self, _: usize) {}
    fn finish(&mut self) {
        print!("\r");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

struct Stepper {
    label: String,
    handle: Option<ThreadSpinnerHandle>,
}

impl Stepper {
    fn start(label: &str) -> Self {
        let renderer = StdoutStepRenderer {
            label: label.to_string(),
        };
        let handle = spawn_threaded(SpinnerStyle::Braille, renderer);
        Self {
            label: label.to_string(),
            handle: Some(handle),
        }
    }
    fn step(&mut self, next_label: &str) {
        if let Some(h) = self.handle.take() {
            h.stop();
            info!("\x1b[32m✓\x1b[0m {}", self.label);
        }
        self.label = next_label.to_string();
        let renderer = StdoutStepRenderer {
            label: next_label.to_string(),
        };
        self.handle = Some(spawn_threaded(SpinnerStyle::Braille, renderer));
    }
    fn finish(mut self) {
        if let Some(h) = self.handle.take() {
            h.stop();
            info!("\x1b[32m✓\x1b[0m {}", self.label);
        }
    }
}

pub fn spawn_updater_process() {
    if let Ok(exe) = std::env::current_exe() {
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--auto-update");

        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let _ = cmd.spawn();
    }
}

pub fn run_auto_update() -> Result<()> {
    if let Err(e) = execute_inner(true) {
        error!("Auto-update check failed: {}", e);
        return Err(e);
    }
    Ok(())
}

pub fn execute() -> Result<()> {
    execute_inner(false)
}

fn execute_inner(silent: bool) -> Result<()> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Engine(e.to_string()))?;

    let manifest_url = std::env::var("TAURINE_UPDATE_MANIFEST_URL").unwrap_or_else(|_| {
        "https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json".to_string()
    });

    let manifest: Manifest = client
        .get(&manifest_url)
        .send()
        .map_err(|e| Error::Engine(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Engine(e.to_string()))?
        .json()
        .map_err(|e| Error::Engine(e.to_string()))?;

    let current_version = env!("CARGO_PKG_VERSION");
    let is_newer = is_newer_version(current_version, &manifest.version);

    // Ensure the tau alias is set up on every update invocation
    crate::platform::alias::ensure_tau_alias();

    if !is_newer {
        if !silent {
            info!("Taurine is already up to date (v{}).", current_version);
        }
        return Ok(());
    }

    let artifact = manifest
        .artifacts
        .get(platform_key())
        .ok_or_else(|| Error::Engine("Platform not supported by latest release".into()))?;

    // Stop the service BEFORE the spinner starts, so "Taurine is stopped."
    // (emitted via tracing::info! to stderr) doesn't interleave with the spinner's
    // stdout carriage-return frames.
    let _ = taurine_core::service::down();

    let mut sp = if !silent {
        Some(Stepper::start(&format!(
            "Fetching update v{}",
            manifest.version
        )))
    } else {
        None
    };

    if let Some(s) = sp.as_mut() {
        s.step("Downloading");
    }

    let temp_dir = taurine_core::system::paths::ensure_temp_dir();
    let archive_ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar"
    };
    let archive_path = temp_dir.join(format!(
        "taurine-update-{}.{}",
        uuid::Uuid::new_v4(),
        archive_ext
    ));
    let binary_path = temp_dir.join(format!("taurine-bin-{}", uuid::Uuid::new_v4()));

    let mut response = client
        .get(&artifact.url)
        .send()
        .map_err(|e| Error::Engine(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Engine(e.to_string()))?;
    let mut archive_file =
        fs::File::create(&archive_path).map_err(|e| Error::Engine(e.to_string()))?;
    std::io::copy(&mut response, &mut archive_file).map_err(|e| Error::Engine(e.to_string()))?;
    drop(archive_file);

    // Verify checksum if available in manifest
    if let Some(expected_sha256) = &artifact.sha256 {
        let computed = {
            let mut hasher = sha2::Sha256::new();
            let mut f = fs::File::open(&archive_path).map_err(|e| Error::Engine(e.to_string()))?;
            let mut buf = [0u8; 8192];
            loop {
                let n = f.read(&mut buf).map_err(|e| Error::Engine(e.to_string()))?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            hasher
                .finalize()
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>()
        };
        if computed != *expected_sha256 {
            let _ = fs::remove_file(&archive_path);
            return Err(Error::Engine(format!(
                "Checksum mismatch for downloaded update: expected {}, got {}",
                expected_sha256, computed
            )));
        }
    }

    if let Some(s) = sp.as_mut() {
        s.step("Extracting");
    }

    if cfg!(target_os = "windows") {
        let extract_dir = temp_dir.join(format!("taurine-ext-{}", uuid::Uuid::new_v4()));
        let status = std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!(
                "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                archive_path.display(),
                extract_dir.display()
            ))
            .status()
            .map_err(|e| Error::Engine(e.to_string()))?;
        if !status.success() {
            return Err(Error::Engine("Failed to extract update".into()));
        }
        fs::copy(extract_dir.join("taurine.exe"), &binary_path)
            .map_err(|e| Error::Engine(e.to_string()))?;
        let _ = fs::remove_dir_all(extract_dir);
    } else {
        let extract_dir = temp_dir.join(format!("taurine-ext-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&extract_dir).map_err(|e| Error::Engine(e.to_string()))?;
        let status = std::process::Command::new("tar")
            .arg("-xf")
            .arg(&archive_path)
            .arg("-C")
            .arg(&extract_dir)
            .status()
            .map_err(|e| Error::Engine(e.to_string()))?;
        if !status.success() {
            return Err(Error::Engine("Failed to extract update".into()));
        }
        fs::copy(extract_dir.join("taurine"), &binary_path)
            .map_err(|e| Error::Engine(e.to_string()))?;
        let _ = fs::remove_dir_all(extract_dir);
    }

    let _ = fs::remove_file(&archive_path);

    if let Some(s) = sp.as_mut() {
        s.step("Installing");
    }

    let current = std::env::current_exe().map_err(|e| Error::Engine(e.to_string()))?;
    let canonical = get_install_exe_path();

    let path_updated = if current == canonical {
        self_replace::self_replace(&binary_path).map_err(|e| Error::Engine(e.to_string()))?;
        false
    } else {
        fs::create_dir_all(get_install_bin_dir()).map_err(|e| Error::Engine(e.to_string()))?;
        fs::copy(&binary_path, &canonical).map_err(|e| Error::Engine(e.to_string()))?;
        ensure_on_path(get_install_bin_dir());
        true
    };

    let _ = fs::remove_file(&binary_path);

    if let Some(s) = sp.take() {
        s.finish();
        if !silent && path_updated {
            info!("PATH updated - restart your shell to use `taurine` directly");
        }
        info!("✓ taurine updated to v{}", manifest.version);
    }

    if let Ok(conn) = taurine_core::db::init::setup() {
        let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
        if settings.notify_on_update {
            let _ = notify_rust::Notification::new()
                .summary("Taurine Updated")
                .body(&format!(
                    "Taurine has been updated to v{}",
                    manifest.version
                ))
                .show();
        }
    }

    std::process::Command::new(&canonical)
        .arg("up")
        .spawn()
        .map_err(|e| Error::Engine(e.to_string()))?;

    Ok(())
}

fn is_newer_version(current: &str, manifest: &str) -> bool {
    let mut current_parts = current.split('-');
    let cur_base = current_parts.next().unwrap_or("0.0.0");
    let mut man_parts = manifest.split('-');
    let man_base = man_parts.next().unwrap_or("0.0.0");

    let cur_tuple: Vec<u32> = cur_base.split('.').filter_map(|s| s.parse().ok()).collect();
    let man_tuple: Vec<u32> = man_base.split('.').filter_map(|s| s.parse().ok()).collect();

    if man_tuple > cur_tuple {
        return true;
    }
    if man_tuple < cur_tuple {
        return false;
    }

    // Base versions are equal — compare pre-release identifiers per semver spec.
    // A missing pre-release is higher than any pre-release.
    let cur_pre = current_parts.next().unwrap_or("");
    let man_pre = man_parts.next().unwrap_or("");

    match (cur_pre.is_empty(), man_pre.is_empty()) {
        (true, false) => return false, // current is release, manifest is pre-release
        (false, true) => return true,  // current is pre-release, manifest is release
        (true, true) => return false,  // both are release — equal
        (false, false) => {}           // both are pre-release — compare numerically
    }

    // Compare pre-release identifiers field-by-field (e.g. "alpha.10" vs "alpha.9")
    let cur_fields: Vec<&str> = cur_pre.split('.').collect();
    let man_fields: Vec<&str> = man_pre.split('.').collect();

    let max_len = cur_fields.len().max(man_fields.len());
    for i in 0..max_len {
        let a = cur_fields.get(i).copied().unwrap_or("");
        let b = man_fields.get(i).copied().unwrap_or("");

        if a == b {
            continue;
        }

        // Numeric comparison when both fields are integers
        if let (Ok(an), Ok(bn)) = (a.parse::<u64>(), b.parse::<u64>()) {
            return bn > an;
        }

        // Lexicographic comparison for non-numeric fields (e.g. "alpha" vs "beta")
        return b > a;
    }

    // All fields equal up to the shorter length — the longer one is greater
    man_fields.len() > cur_fields.len()
}

fn ensure_on_path(dir: PathBuf) {
    if cfg!(target_os = "windows") {
        #[cfg(target_os = "windows")]
        {
            use winreg::RegKey;
            use winreg::enums::*;
            if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
                && let Ok(path) = hkcu.get_value::<String, _>("Path")
            {
                let dir_str = dir.to_string_lossy().to_string();
                if !path.contains(&dir_str) {
                    let new_path = if path.ends_with(';') {
                        format!("{}{}", path, dir_str)
                    } else {
                        format!("{};{}", path, dir_str)
                    };
                    let _ = hkcu.set_value("Path", &new_path);
                }
            }
        }
    } else {
        let dir_str = dir.to_string_lossy().to_string();
        for profile in crate::platform::shell::detect_shell_profiles() {
            let _ = crate::platform::shell::ensure_path_in_profile(&profile, &dir_str);
        }
    }
}
