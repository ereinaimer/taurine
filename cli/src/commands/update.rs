use reqwest::blocking::Client;
use sha2::Digest;
use std::fs;
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use taurine_core::error::{Error, Result};
use taurine_core::paths::{get_install_bin_dir, get_install_exe_path};
use taurine_core::settings::SpinnerStyle;
use taurine_core::utils::spinner::{
    BRAILLE_FRAMES, SpinnerRenderer, ThreadSpinnerHandle, spawn_threaded,
};
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

pub(crate) fn parse_expected_sha256(raw: &str) -> &str {
    let trimmed = raw.trim();
    let after_colon = match trimmed.rsplit_once(':') {
        Some((_, hash)) => hash.trim(),
        None => trimmed,
    };
    after_colon.split_whitespace().next().unwrap_or(after_colon)
}

fn find_binary_in_dir(dir: &Path, target_name: &str) -> Option<PathBuf> {
    let direct = dir.join(target_name);
    if direct.is_file() {
        return Some(direct);
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find_binary_in_dir(&path, target_name) {
                    return Some(found);
                }
            } else if path.is_file()
                && path.file_name().and_then(|n| n.to_str()) == Some(target_name)
            {
                return Some(path);
            }
        }
    }
    None
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
    visual: bool,
    progress_open: bool,
    frame_idx: usize,
}

impl Stepper {
    fn start_visual(label: &str, visual: bool) -> Self {
        let handle = visual.then(|| {
            let renderer = StdoutStepRenderer {
                label: label.to_string(),
            };
            spawn_threaded(SpinnerStyle::Braille, renderer)
        });
        Self {
            label: label.to_string(),
            handle,
            visual,
            progress_open: false,
            frame_idx: 0,
        }
    }
    fn stop_thread(&mut self) {
        if let Some(h) = self.handle.take() {
            h.stop();
        }
    }
    fn step(&mut self, next_label: &str) {
        let done = std::mem::replace(&mut self.label, next_label.to_string());
        self.stop_thread();
        self.clear_progress();
        info!("✓ {done}");
        if self.visual {
            let renderer = StdoutStepRenderer {
                label: next_label.to_string(),
            };
            self.handle = Some(spawn_threaded(SpinnerStyle::Braille, renderer));
        }
    }
    /// Same as step, but logs a custom done line (used to report download
    /// totals instead of echoing the in-progress label).
    fn step_with_done(&mut self, next_label: &str, done_label: &str) {
        self.stop_thread();
        self.clear_progress();
        info!("✓ {done_label}");
        self.label = next_label.to_string();
        if self.visual {
            let renderer = StdoutStepRenderer {
                label: next_label.to_string(),
            };
            self.handle = Some(spawn_threaded(SpinnerStyle::Braille, renderer));
        }
    }
    /// Freeze the spinner and open the two-line download block: line 1 keeps
    /// the static label, line 2 carries the live `downloaded/total (%) @ speed`.
    /// The cursor is left on line 2 until clear_progress runs.
    fn start_attempt(&mut self, label: &str) {
        self.stop_thread();
        self.label = label.to_string();
        if !self.visual {
            info!("{label}...");
            return;
        }
        self.clear_progress();
        print!("\r{label}\x1b[K\n");
        let _ = std::io::stdout().flush();
        self.progress_open = true;
        self.frame_idx = 0;
    }
    /// Single-writer redraw of both download lines (spinner thread is stopped
    /// while the block is open, so no output races). Call throttled ~5Hz.
    fn draw_download(&mut self, downloaded: u64, total: Option<u64>, speed_bps: f64) {
        if !self.visual || !self.progress_open {
            return;
        }
        let frame = BRAILLE_FRAMES[self.frame_idx % BRAILLE_FRAMES.len()];
        self.frame_idx += 1;
        let line2 = download_progress_line(downloaded, total, speed_bps);
        print!("\x1b[1A\r{frame} {}\x1b[K\n\r{line2}\x1b[K", self.label);
        let _ = std::io::stdout().flush();
    }
    /// Clear the open download block (line 2, then back up to line 1).
    /// No-op unless a block is open.
    fn clear_progress(&mut self) {
        if !self.visual || !self.progress_open {
            return;
        }
        print!("\r\x1b[K\x1b[1A\r\x1b[K");
        let _ = std::io::stdout().flush();
        self.progress_open = false;
    }
    /// Clear a half-finished block without logging (error paths log via Err).
    fn abort_progress(&mut self) {
        self.stop_thread();
        self.clear_progress();
    }
    fn finish(mut self) {
        self.stop_thread();
        self.clear_progress();
        info!("✓ {}", self.label);
    }
}

const SIZE_UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];

fn size_unit(bytes: u64) -> usize {
    let mut unit = 0;
    let mut scaled = bytes;
    while scaled >= 1024 && unit < SIZE_UNITS.len() - 1 {
        scaled /= 1024;
        unit += 1;
    }
    unit
}

fn fmt_scaled(bytes: u64, unit: usize) -> String {
    if unit == 0 {
        format!("{bytes}")
    } else {
        format!("{:.1}", bytes as f64 / 1024f64.powi(unit as i32))
    }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    let unit = size_unit(bytes);
    format!("{} {}", fmt_scaled(bytes, unit), SIZE_UNITS[unit])
}

/// `downloaded/total` sharing one unit: `12.4/28.1 MB`.
pub(crate) fn format_pair(downloaded: u64, total: u64) -> String {
    let unit = size_unit(downloaded.max(total));
    format!(
        "{}/{} {}",
        fmt_scaled(downloaded, unit),
        fmt_scaled(total, unit),
        SIZE_UNITS[unit]
    )
}

pub(crate) fn format_speed(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() || bytes_per_sec <= 0.0 {
        return "0 B/s".to_string();
    }
    format!("{}/s", format_bytes(bytes_per_sec as u64))
}

pub(crate) fn format_duration(total_secs: u64) -> String {
    if total_secs < 60 {
        format!("{total_secs}s")
    } else {
        format!("{}m {}s", total_secs / 60, total_secs % 60)
    }
}

/// Line-2 content for the open download block.
pub(crate) fn download_progress_line(
    downloaded: u64,
    total: Option<u64>,
    speed_bps: f64,
) -> String {
    match total {
        Some(t) if t > 0 => {
            let pct = ((downloaded as f64 / t as f64) * 100.0)
                .floor()
                .clamp(0.0, 100.0) as u64;
            format!(
                "  {} ({pct}%) @ {}",
                format_pair(downloaded, t),
                format_speed(speed_bps)
            )
        }
        _ => format!(
            "  {} downloaded @ {}",
            format_bytes(downloaded),
            format_speed(speed_bps)
        ),
    }
}

struct DownloadStats {
    bytes: u64,
    elapsed: Duration,
}

/// Stream the archive to disk, redrawing the open download block ~5Hz.
/// Progress goes to stdout directly (never info!: per-tick log lines would
/// spam the log file and pick up timestamps/prefixes).
fn download_archive(
    client: &Client,
    url: &str,
    dest: &Path,
    mut sp: Option<&mut Stepper>,
) -> Result<DownloadStats> {
    let mut response = client
        .get(url)
        .send()
        .map_err(|e| Error::Engine(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Engine(e.to_string()))?;
    let total = response.content_length();
    let mut file = fs::File::create(dest).map_err(|e| Error::Engine(e.to_string()))?;
    let start = Instant::now();
    let mut downloaded = 0u64;
    let mut buf = [0u8; 65536];
    let mut last_draw = Instant::now();
    let mut window_start = start;
    let mut window_bytes = 0u64;
    let mut speed_bps = 0.0;
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| Error::Engine(e.to_string()))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| Error::Engine(e.to_string()))?;
        downloaded += n as u64;
        window_bytes += n as u64;
        let now = Instant::now();
        let window_secs = now.duration_since(window_start).as_secs_f64();
        if window_secs >= 1.0 {
            speed_bps = window_bytes as f64 / window_secs;
            window_start = now;
            window_bytes = 0;
        } else if speed_bps == 0.0 {
            let elapsed = now.duration_since(start).as_secs_f64();
            if elapsed > 0.0 {
                speed_bps = downloaded as f64 / elapsed;
            }
        }
        if let Some(s) = sp.as_mut()
            && now.duration_since(last_draw) >= Duration::from_millis(200)
        {
            s.draw_download(downloaded, total, speed_bps);
            last_draw = now;
        }
    }
    if let Some(s) = sp.as_mut() {
        s.draw_download(downloaded, total, speed_bps);
    }
    Ok(DownloadStats {
        bytes: downloaded,
        elapsed: start.elapsed(),
    })
}

pub use taurine_core::service::spawn_updater_process;

pub fn run_auto_update() -> Result<()> {
    if let Err(e) = execute_inner(true, false) {
        error!("Auto-update check failed: {}", e);
        return Err(e);
    }
    Ok(())
}

pub fn execute(json: bool) -> Result<()> {
    execute_inner(false, json)
}

fn execute_inner(silent: bool, json: bool) -> Result<()> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Engine(e.to_string()))?;

    let manifest_url = taurine_core::paths::dev_env_var("TAURINE_UPDATE_MANIFEST_URL")
        .unwrap_or_else(|| {
            "https://github.com/ereinaimer/taurine/releases/latest/download/manifest.json"
                .to_string()
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

    // The spinner + progress block write stdout directly via print! (per-tick
    // info! would spam the log file), so gate them explicitly: silent
    // auto-update and --json stay clean, and piped output gets single-shot
    // info! lines with no escape codes. -q suppresses the info! lines via
    // tracing, matching the pre-existing spinner semantics.
    let visual = !silent && !json && std::io::stdout().is_terminal();
    let mut sp = if !silent && !json {
        Some(Stepper::start_visual(
            &format!("Fetching update v{}", manifest.version),
            visual,
        ))
    } else {
        None
    };

    let base_label = format!("Downloading taurine v{}", manifest.version);

    let temp_dir = taurine_core::system::paths::ensure_temp_dir();
    let archive_ext = if artifact.url.ends_with(".tar.xz") {
        "tar.xz"
    } else if artifact.url.ends_with(".tar.gz") {
        "tar.gz"
    } else if artifact.url.ends_with(".zip") || cfg!(target_os = "windows") {
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

    // Long-timeout client for the archive body (the 10s manifest client above
    // would kill slow downloads mid-stream). Mirrors --max-time 300 in sh/ps1.
    let dl_client = Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| Error::Engine(e.to_string()))?;

    // Retry with exponential backoff, matching install.sh/ps1 (3 attempts).
    // Progress resets per attempt; the label carries the attempt count.
    let max_attempts = 3u32;
    let mut retry_delay = Duration::from_secs(2);
    let mut attempt = 1u32;
    let stats = loop {
        let attempt_label = if attempt == 1 {
            base_label.clone()
        } else {
            format!("{base_label} (attempt {attempt}/{max_attempts})")
        };
        if let Some(s) = sp.as_mut() {
            s.start_attempt(&attempt_label);
        }
        match download_archive(&dl_client, &artifact.url, &archive_path, sp.as_mut()) {
            Ok(stats) => break stats,
            Err(e) if attempt < max_attempts => {
                let _ = fs::remove_file(&archive_path);
                if let Some(s) = sp.as_mut() {
                    s.abort_progress();
                }
                if !silent {
                    info!(
                        "  Download failed ({e}). Retrying in {}s... ({attempt}/{max_attempts})",
                        retry_delay.as_secs()
                    );
                }
                std::thread::sleep(retry_delay);
                retry_delay *= 2;
                attempt += 1;
            }
            Err(e) => {
                if let Some(s) = sp.as_mut() {
                    s.abort_progress();
                }
                let _ = fs::remove_file(&archive_path);
                return Err(e);
            }
        }
    };

    if let Some(s) = sp.as_mut() {
        s.step_with_done(
            "Extracting",
            &format!(
                "Downloaded taurine v{} ({} in {})",
                manifest.version,
                format_bytes(stats.bytes),
                format_duration(stats.elapsed.as_secs())
            ),
        );
    }

    // Verify checksum if available in manifest
    if let Some(expected_sha256_raw) = &artifact.sha256 {
        let expected = parse_expected_sha256(expected_sha256_raw);
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
        if !computed.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&archive_path);
            return Err(Error::Engine(format!(
                "Checksum mismatch for downloaded update: expected {}, got {}",
                expected, computed
            )));
        }
    }

    // (Spinner already sits on "Extracting" — the download completion above
    // transitioned it with the totals line.)

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
        let source_bin = find_binary_in_dir(&extract_dir, "taurine.exe")
            .ok_or_else(|| Error::Engine("Extracted archive did not contain taurine.exe".into()))?;
        fs::copy(source_bin, &binary_path).map_err(|e| Error::Engine(e.to_string()))?;
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
        let source_bin = find_binary_in_dir(&extract_dir, "taurine").ok_or_else(|| {
            Error::Engine("Extracted archive did not contain taurine binary".into())
        })?;
        fs::copy(source_bin, &binary_path).map_err(|e| Error::Engine(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&binary_path, fs::Permissions::from_mode(0o755));
        }
        let _ = fs::remove_dir_all(extract_dir);
    }

    let _ = fs::remove_file(&archive_path);

    if let Some(s) = sp.as_mut() {
        s.step("Installing");
    }

    // Stop the running service only after the archive is fully downloaded,
    // validated, and extracted, minimizing service downtime and avoiding stopping
    // if download or checksum fails.
    let _ = taurine_core::service::down();

    let current = std::env::current_exe().map_err(|e| Error::Engine(e.to_string()))?;
    let canonical = get_install_exe_path();

    let path_updated = if current == canonical {
        self_replace::self_replace(&binary_path).map_err(|e| Error::Engine(e.to_string()))?;
        false
    } else {
        fs::create_dir_all(get_install_bin_dir()).map_err(|e| Error::Engine(e.to_string()))?;
        if canonical.exists() {
            let backup_path = temp_dir.join(format!("taurine-old-{}", uuid::Uuid::new_v4()));
            if let Err(e) = fs::rename(&canonical, &backup_path) {
                tracing::debug!("Failed to rename canonical binary before copy: {}", e);
            } else {
                let _ = fs::remove_file(backup_path);
            }
        }
        fs::copy(&binary_path, &canonical).map_err(|e| Error::Engine(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&canonical, fs::Permissions::from_mode(0o755));
        }
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

    let mut up_cmd = std::process::Command::new(&canonical);
    up_cmd.arg("up");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        up_cmd.creation_flags(CREATE_NO_WINDOW);
    }
    up_cmd.spawn().map_err(|e| Error::Engine(e.to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_key_returns_valid_supported_platform() {
        let key = platform_key();
        assert!(
            key == "windows-x86_64"
                || key == "linux-x86_64"
                || key == "macos-x86_64"
                || key == "macos-aarch64",
            "unexpected platform key: {key}"
        );
    }

    #[test]
    fn test_manifest_deserialization() {
        let json = r#"{
            "version": "1.0.0",
            "artifacts": {
                "windows-x86_64": {
                    "url": "https://example.com/taurine-windows.zip",
                    "sha256": "abcdef1234567890"
                },
                "linux-x86_64": {
                    "url": "https://example.com/taurine-linux.tar"
                }
            }
        }"#;

        let manifest: Manifest = serde_json::from_str(json).expect("manifest should deserialize");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.artifacts.len(), 2);

        let win_artifact = manifest.artifacts.get("windows-x86_64").unwrap();
        assert_eq!(win_artifact.url, "https://example.com/taurine-windows.zip");
        assert_eq!(win_artifact.sha256.as_deref(), Some("abcdef1234567890"));

        let linux_artifact = manifest.artifacts.get("linux-x86_64").unwrap();
        assert_eq!(linux_artifact.url, "https://example.com/taurine-linux.tar");
        assert_eq!(linux_artifact.sha256, None);
    }

    #[test]
    fn test_is_newer_version_major_bump() {
        assert!(is_newer_version("1.0.0", "2.0.0"));
        assert!(!is_newer_version("2.0.0", "1.0.0"));
    }

    #[test]
    fn test_is_newer_version_minor_bump() {
        assert!(is_newer_version("1.0.0", "1.1.0"));
        assert!(!is_newer_version("1.1.0", "1.0.0"));
    }

    #[test]
    fn test_is_newer_version_patch_bump() {
        assert!(is_newer_version("1.0.0", "1.0.1"));
        assert!(!is_newer_version("1.0.1", "1.0.0"));
    }

    #[test]
    fn test_is_newer_version_identical_release() {
        assert!(!is_newer_version("1.0.0", "1.0.0"));
        assert!(!is_newer_version("2.3.4", "2.3.4"));
    }

    #[test]
    fn test_is_newer_version_release_vs_prerelease() {
        // A release version is newer than any pre-release of the same base
        assert!(is_newer_version("1.0.0-alpha.1", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.0-alpha.1"));
    }

    #[test]
    fn test_is_newer_version_prerelease_numeric_increments() {
        // e.g. alpha.9 vs alpha.10 (numeric comparison, not lexicographic)
        assert!(is_newer_version("1.0.0-alpha.9", "1.0.0-alpha.10"));
        assert!(!is_newer_version("1.0.0-alpha.10", "1.0.0-alpha.9"));
        assert!(is_newer_version("1.0.0-alpha.17", "1.0.0-alpha.18"));
        assert!(!is_newer_version("1.0.0-alpha.18", "1.0.0-alpha.17"));
    }

    #[test]
    fn test_is_newer_version_prerelease_ident_comparison() {
        // alpha vs beta
        assert!(is_newer_version("1.0.0-alpha.1", "1.0.0-beta.1"));
        assert!(!is_newer_version("1.0.0-beta.1", "1.0.0-alpha.1"));
    }

    #[test]
    fn test_is_newer_version_prerelease_length_tiebreak() {
        // alpha.1 vs alpha.1.1
        assert!(is_newer_version("1.0.0-alpha.1", "1.0.0-alpha.1.1"));
        assert!(!is_newer_version("1.0.0-alpha.1.1", "1.0.0-alpha.1"));
    }

    #[test]
    fn test_is_newer_version_identical_prerelease() {
        assert!(!is_newer_version("1.0.0-alpha.17", "1.0.0-alpha.17"));
    }

    #[test]
    fn test_is_newer_version_higher_base_even_with_lower_prerelease() {
        assert!(is_newer_version("1.0.0", "1.1.0-alpha.1"));
        assert!(is_newer_version("1.0.0-alpha.10", "2.0.0-alpha.1"));
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        // 12.4 MiB and 28.1 MiB — the contract example values.
        assert_eq!(format_bytes(13_002_342), "12.4 MB");
        assert_eq!(format_bytes(29_464_986), "28.1 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn test_format_pair_shares_unit() {
        assert_eq!(format_pair(13_002_342, 29_464_986), "12.4/28.1 MB");
        assert_eq!(format_pair(29_464_986, 29_464_986), "28.1/28.1 MB");
        assert_eq!(format_pair(0, 29_464_986), "0.0/28.1 MB");
    }

    #[test]
    fn test_download_progress_line_known_total() {
        let line = download_progress_line(13_002_342, Some(29_464_986), 3.2 * 1024.0 * 1024.0);
        assert_eq!(line, "  12.4/28.1 MB (44%) @ 3.2 MB/s");
    }

    #[test]
    fn test_download_progress_line_unknown_total() {
        let line = download_progress_line(13_002_342, None, 3.1 * 1024.0 * 1024.0);
        assert_eq!(line, "  12.4 MB downloaded @ 3.1 MB/s");
        // Zero total falls back the same way (missing Content-Length).
        let line = download_progress_line(512, Some(0), 1024.0);
        assert_eq!(line, "  512 B downloaded @ 1.0 KB/s");
    }

    #[test]
    fn test_download_progress_line_clamps_percent() {
        let line = download_progress_line(30_000_000, Some(29_464_986), 1024.0);
        assert!(line.contains("(100%)"), "got: {line}");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(8), "8s");
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(72), "1m 12s");
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(0.0), "0 B/s");
        assert_eq!(format_speed(-1.0), "0 B/s");
        assert_eq!(format_speed(3.2 * 1024.0 * 1024.0), "3.2 MB/s");
    }

    #[test]
    fn test_parse_expected_sha256() {
        // Formats seen in release manifests and checksum files:
        // 1. colon-prefixed from multi-file grep
        assert_eq!(
            parse_expected_sha256(
                "checksums/checksums-windows-x86_64.sha256:6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
            ),
            "6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
        );
        // 2. sha256: prefix
        assert_eq!(
            parse_expected_sha256(
                "sha256:6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
            ),
            "6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
        );
        // 3. raw hash string
        assert_eq!(
            parse_expected_sha256(
                "6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
            ),
            "6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
        );
        // 4. standard sha256sum line with filename
        assert_eq!(
            parse_expected_sha256(
                "6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be  taurine-v1.0.0-windows-x86_64.zip"
            ),
            "6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
        );
        // 5. with leading/trailing whitespace
        assert_eq!(
            parse_expected_sha256(
                "   6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be \n\t"
            ),
            "6c7be92d1ab87ad13c9ed3af58ac6ca956b3b9a994ecf83909f6b280c45595be"
        );
    }
}
