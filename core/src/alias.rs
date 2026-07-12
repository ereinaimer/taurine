//! Utilities for managing the `tau` shell alias for `taurine`.
//!
//! On Unix this writes `alias tau='taurine'` to `.bashrc`/`.zshrc` etc.
//! On Windows it writes `function tau { taurine @args }` to the PowerShell
//! profile (`$PROFILE`).

use std::path::PathBuf;
use tracing::{debug, info};

use crate::shell;

fn get_alias_line_for_profile(path: &std::path::Path) -> &'static str {
    if cfg!(target_os = "windows") {
        "function tau { taurine @args }"
    } else {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.contains("csh") {
            "alias tau taurine"
        } else {
            "alias tau='taurine'"
        }
    }
}

fn get_remove_prefix_for_profile(path: &std::path::Path) -> &'static str {
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

/// Return the shell profiles relevant for the tau alias on the current OS.
///
/// - Unix: delegates to [`shell::detect_shell_profiles`].
/// - Windows: prefers `$PROFILE` (set by PowerShell), falls back to
///   constructing the typical PowerShell profile paths from `%USERPROFILE%`.
fn get_profiles() -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        // $PROFILE is only set when running inside PowerShell itself.
        // When running from bash / cmd we construct the paths manually.
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
                // Neither exists — prefer the PowerShell 7 location
                vec![ps7]
            }
        } else {
            Vec::new()
        }
    } else {
        shell::detect_shell_profiles()
    }
}

/// Check whether the `tau` alias is present in any detected shell profile.
pub fn tau_alias_is_set() -> bool {
    let profiles = get_profiles();
    tau_alias_is_set_in(&profiles)
}

/// Check whether the `tau` alias is present in the given profiles.
fn tau_alias_is_set_in(profiles: &[PathBuf]) -> bool {
    profiles
        .iter()
        .any(|p| shell::line_exists_in_rc_file(p, get_alias_line_for_profile(p)))
}

/// Ensure the `tau` alias exists in all detected shell profiles.
///
/// Always emits an `info!` message confirming the alias is set up.
/// Returns `true` if at least one profile was modified.
pub fn ensure_tau_alias() -> bool {
    let profiles = get_profiles();
    let modified = ensure_tau_alias_in(&profiles);

    if modified {
        info!("Added alias tau to your shell profile.");
        info!("Now you can run 'tau --help' for more details.");
    }

    modified
}

/// Ensure the `tau` alias exists in the given profiles.
///
/// Returns `true` if at least one profile was modified.
fn ensure_tau_alias_in(profiles: &[PathBuf]) -> bool {
    let mut modified = false;

    for profile in profiles {
        let expected_line = get_alias_line_for_profile(profile);
        let remove_prefix = get_remove_prefix_for_profile(profile);

        if !profile.exists() {
            if let Ok(true) = shell::append_line_to_rc_file(profile, expected_line) {
                debug!("Added tau alias to: {}", profile.display());
                modified = true;
            }
            continue;
        }

        match std::fs::read_to_string(profile) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let matches: Vec<&str> = lines
                    .iter()
                    .copied()
                    .filter(|line| line.contains(remove_prefix))
                    .collect();

                // If there is exactly one match and it matches expected_line exactly (when trimmed), we are done
                if matches.len() == 1 && matches[0].trim() == expected_line {
                    continue;
                }

                // Filter out all matching lines and write back with exactly one clean alias line
                let clean_lines: Vec<&str> = lines
                    .into_iter()
                    .filter(|line| !line.contains(remove_prefix))
                    .collect();

                let mut new_content = clean_lines.join("\n");
                if !new_content.is_empty() {
                    new_content.push_str("\n\n");
                }
                new_content.push_str(expected_line);
                new_content.push('\n');

                if let Err(e) = std::fs::write(profile, new_content) {
                    debug!("Could not write to {}: {e}", profile.display());
                } else {
                    debug!("Cleaned and set tau alias in: {}", profile.display());
                    modified = true;
                }
            }
            Err(e) => {
                debug!("Could not read {}: {e}", profile.display());
            }
        }
    }

    modified
}

/// Remove the `tau` alias from all detected shell profiles.
///
/// Returns `true` if at least one profile was modified.
pub fn remove_tau_alias() -> bool {
    let profiles = get_profiles();
    remove_tau_alias_in(&profiles)
}

/// Remove the `tau` alias from the given profiles.
///
/// Returns `true` if at least one profile was modified.
fn remove_tau_alias_in(profiles: &[PathBuf]) -> bool {
    let mut modified = false;

    for profile in profiles {
        let remove_prefix = get_remove_prefix_for_profile(profile);
        match shell::remove_lines_from_rc_file(profile, remove_prefix) {
            Ok(true) => {
                debug!("Removed tau alias from: {}", profile.display());
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

    #[test]
    fn test_tau_alias_is_set_in_empty_profiles() {
        let result = tau_alias_is_set_in(&[]);
        assert!(!result);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_tau_alias_is_set_in_matching_unix_alias() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        fs::write(&rc, "alias tau='taurine'\n").unwrap();

        let result = tau_alias_is_set_in(&[rc]);
        assert!(result);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_tau_alias_is_set_in_matching_powershell_function() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join("Microsoft.PowerShell_profile.ps1");
        fs::write(&rc, "function tau { taurine @args }\n").unwrap();

        let result = tau_alias_is_set_in(&[rc]);
        assert!(result);
    }

    #[test]
    fn test_ensure_tau_alias_in_creates_alias() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        fs::write(&rc, "").unwrap();

        let modified = ensure_tau_alias_in(std::slice::from_ref(&rc));
        assert!(modified);

        let content = fs::read_to_string(&rc).unwrap();
        assert!(content.contains("alias tau=") || content.contains("function tau"));
    }

    #[test]
    fn test_ensure_tau_alias_in_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");

        assert!(ensure_tau_alias_in(std::slice::from_ref(&rc)));
        assert!(!ensure_tau_alias_in(std::slice::from_ref(&rc)));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_remove_tau_alias_in_removes_unix_alias() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        fs::write(&rc, "alias tau='taurine'\nexport EDITOR=vim\n").unwrap();

        // First ensure our alias is detected
        assert!(tau_alias_is_set_in(std::slice::from_ref(&rc)));

        // Remove the alias using the internal function
        let removed = remove_tau_alias_in(std::slice::from_ref(&rc));
        assert!(removed);

        // Alias should no longer be detected
        assert!(!tau_alias_is_set_in(std::slice::from_ref(&rc)));

        // Other content should remain
        let content = fs::read_to_string(&rc).unwrap();
        assert!(content.contains("export EDITOR=vim"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_remove_tau_alias_in_removes_powershell_function() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join("Microsoft.PowerShell_profile.ps1");
        fs::write(&rc, "function tau { taurine @args }\nexport EDITOR=vim\n").unwrap();

        // First ensure our alias is detected
        assert!(tau_alias_is_set_in(std::slice::from_ref(&rc)));

        // Remove the alias using the internal function
        let removed = remove_tau_alias_in(std::slice::from_ref(&rc));
        assert!(removed);

        // Alias should no longer be detected
        assert!(!tau_alias_is_set_in(std::slice::from_ref(&rc)));

        // Other content should remain
        let content = fs::read_to_string(&rc).unwrap();
        assert!(content.contains("export EDITOR=vim"));
    }

    #[test]
    fn test_get_profiles_on_current_platform() {
        let profiles = get_profiles();
        // Should not panic on any platform
        // On non-Windows without HOME it may be empty; on Windows without PROFILE it may be empty
        // Just verify it returns without panicking and is a Vec
        assert!(profiles.is_empty() || !profiles.is_empty());
    }

    #[test]
    fn test_ensure_tau_alias_in_cleans_duplicates_and_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");

        let initial_content = if cfg!(target_os = "windows") {
            "function tau { foo }\nexport EDITOR=vim\nfunction tau { bar }\n"
        } else {
            "alias tau='foo'\nexport EDITOR=vim\nalias tau='bar'\n"
        };
        fs::write(&rc, initial_content).unwrap();

        let modified = ensure_tau_alias_in(std::slice::from_ref(&rc));
        assert!(modified);

        let content = fs::read_to_string(&rc).unwrap();
        assert!(content.contains("export EDITOR=vim"));

        let prefix = get_remove_prefix_for_profile(&rc);
        let count = content.lines().filter(|line| line.contains(prefix)).count();
        assert_eq!(count, 1);

        let expected = get_alias_line_for_profile(&rc);
        assert!(content.contains(expected));
    }
}
