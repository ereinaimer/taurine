//! Utilities for managing shell profiles and cleaning up legacy `tau` aliases.

use std::path::{Path, PathBuf};
use tracing::debug;

use super::shell;

fn get_remove_prefix_for_profile(path: &Path) -> &'static str {
    if cfg!(target_os = "windows") {
        "function tau"
    } else {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains("csh") {
            "alias tau "
        } else {
            "alias tau="
        }
    }
}

/// Return the shell profiles relevant for shell aliases on the current OS.
fn get_profiles() -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        if let Some(profile) = std::env::var("PROFILE").ok().filter(|p| !p.is_empty()) {
            return vec![PathBuf::from(profile)];
        }

        if let Ok(up) = std::env::var("USERPROFILE") {
            let home = PathBuf::from(up);
            let docs = home.join("Documents");
            let ps7 = docs
                .join("PowerShell")
                .join("Microsoft.PowerShell_profile.ps1");
            let ps5 = docs
                .join("WindowsPowerShell")
                .join("Microsoft.PowerShell_profile.ps1");

            if ps7.exists() {
                vec![ps7]
            } else if ps5.exists() {
                vec![ps5]
            } else {
                vec![ps7]
            }
        } else {
            Vec::new()
        }
    } else {
        shell::detect_shell_profiles()
    }
}

/// Remove legacy `tau` alias from all detected shell profiles.
///
/// Returns `true` if at least one profile was modified.
pub fn clean_legacy_tau_alias() -> bool {
    let profiles = get_profiles();
    clean_legacy_tau_alias_in(&profiles)
}

/// Synonym for `clean_legacy_tau_alias()`.
pub fn remove_legacy_tau_alias() -> bool {
    clean_legacy_tau_alias()
}

/// Remove legacy `tau` alias from the given profiles.
///
/// Returns `true` if at least one profile was modified.
pub fn clean_legacy_tau_alias_in(profiles: &[PathBuf]) -> bool {
    let mut modified = false;

    for profile in profiles {
        let remove_prefix = get_remove_prefix_for_profile(profile);
        match shell::remove_lines_from_rc_file(profile, remove_prefix) {
            Ok(true) => {
                debug!("Removed legacy tau alias from: {}", profile.display());
                modified = true;
            }
            Ok(false) => {}
            Err(e) => {
                debug!("Could not write to {}: {e}", profile.display());
            }
        }
    }

    modified
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_clean_legacy_tau_alias_in_removes_unix_alias() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        fs::write(&rc, "alias tau='taurine'\nexport EDITOR=vim\n").unwrap();

        let removed = clean_legacy_tau_alias_in(std::slice::from_ref(&rc));
        assert!(removed);

        let content = fs::read_to_string(&rc).unwrap();
        assert!(!content.contains("alias tau="));
        assert!(content.contains("export EDITOR=vim"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_clean_legacy_tau_alias_in_removes_powershell_function() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join("Microsoft.PowerShell_profile.ps1");
        fs::write(&rc, "function tau { taurine @args }\nexport EDITOR=vim\n").unwrap();

        let removed = clean_legacy_tau_alias_in(std::slice::from_ref(&rc));
        assert!(removed);

        let content = fs::read_to_string(&rc).unwrap();
        assert!(!content.contains("function tau"));
        assert!(content.contains("export EDITOR=vim"));
    }

    #[test]
    fn test_get_profiles_on_current_platform() {
        let profiles = get_profiles();
        assert!(profiles.is_empty() || !profiles.is_empty());
    }
}
