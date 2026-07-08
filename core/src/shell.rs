//! Utilities for manipulating shell RC / profile files.
//!
//! These functions are shared between the CLI's update command, completions command,
//! and alias setup logic — anyplace that needs to append a line to `.bashrc`,
//! `.zshrc`, `.zprofile`, or similar files.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Append a line to a shell RC file if it is not already present.
///
/// Creates the file (and parent directories) if it does not exist.
/// Returns `true` if the line was appended, `false` if it already existed.
pub fn append_line_to_rc_file(path: &Path, line: &str) -> std::io::Result<bool> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(path)?;
        writeln!(file, "{line}")?;
        debug!("Created and wrote to: {}", path.display());
        return Ok(true);
    }

    let content = fs::read_to_string(path)?;

    if content.lines().any(|l| l.trim() == line) {
        debug!("File {} already contains the line.", path.display());
        return Ok(false);
    }

    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    writeln!(file, "\n{line}")?;
    debug!("Appended to: {}", path.display());
    Ok(true)
}

/// Remove every line containing `line_prefix` from a shell RC file.
///
/// Returns `true` if any lines were removed, `false` otherwise.
pub fn remove_lines_from_rc_file(path: &Path, line_prefix: &str) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(path)?;

    if !content.contains(line_prefix) {
        return Ok(false);
    }

    let new_content: String = content
        .lines()
        .filter(|line| !line.contains(line_prefix))
        .collect::<Vec<_>>()
        .join("\n");

    fs::write(path, &new_content)?;
    debug!(
        "Removed lines containing '{line_prefix}' from: {}",
        path.display()
    );
    Ok(true)
}

/// Check whether `line` is present in a shell RC file.
pub fn line_exists_in_rc_file(path: &Path, line: &str) -> bool {
    fs::read_to_string(path)
        .map(|content| content.lines().any(|l| l.trim() == line))
        .unwrap_or(false)
}

/// Return a list of shell profile file paths for the current OS.
///
/// The list may contain paths that do not yet exist on disk; callers should
/// check existence or pass them directly to [`append_line_to_rc_file`] (which
/// creates files on demand).
pub fn detect_shell_profiles() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let home_path = PathBuf::from(&home);
    let mut profiles = Vec::new();

    if cfg!(target_os = "windows") {
        // Windows does not have Unix-style RC files.
        // PowerShell profile is handled directly in completions.rs / install.ps1.
    } else if cfg!(target_os = "macos") {
        profiles.push(home_path.join(".zprofile"));
        profiles.push(home_path.join(".zshrc"));
        profiles.push(home_path.join(".bash_profile"));
    } else {
        // Linux / others
        let shell = std::env::var("SHELL").unwrap_or_default();
        if shell.contains("zsh") {
            profiles.push(home_path.join(".zshrc"));
        } else if shell.contains("fish") {
            profiles.push(home_path.join(".config/fish/config.fish"));
        } else {
            profiles.push(home_path.join(".bashrc"));
        }
        // Also include the alternatives so we catch multi-shell setups
        profiles.push(home_path.join(".bashrc"));
        profiles.push(home_path.join(".zshrc"));
        profiles.push(home_path.join(".bash_profile"));
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    profiles.retain(|p| seen.insert(p.clone()));

    profiles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_append_line_to_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".testrc");

        let added = append_line_to_rc_file(&path, "alias foo='bar'").unwrap();
        assert!(added);

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("alias foo='bar'"));
    }

    #[test]
    fn test_append_line_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".testrc");

        append_line_to_rc_file(&path, "alias foo='bar'").unwrap();
        let added = append_line_to_rc_file(&path, "alias foo='bar'").unwrap();
        assert!(!added, "second append should return false");
    }

    #[test]
    fn test_remove_lines_from_rc_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".testrc");
        fs::write(&path, "alias foo='bar'\nalias baz='qux'\n").unwrap();

        let removed = remove_lines_from_rc_file(&path, "alias foo").unwrap();
        assert!(removed);

        let content = fs::read_to_string(&path).unwrap();
        assert!(!content.contains("alias foo"));
        assert!(content.contains("alias baz"));
    }

    #[test]
    fn test_remove_lines_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".nonexistent");
        let removed = remove_lines_from_rc_file(&path, "alias").unwrap();
        assert!(!removed);
    }

    #[test]
    fn test_line_exists_in_rc_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".testrc");
        fs::write(&path, "alias tau='taurine'\n").unwrap();

        // Exact line match
        assert!(line_exists_in_rc_file(&path, "alias tau='taurine'"));
        // Substring alone should not match (we match whole lines)
        assert!(!line_exists_in_rc_file(&path, "alias tau="));
    }

    #[test]
    fn test_line_does_not_exist_when_in_comment() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".testrc");
        // The string appears only in a comment, not as an actual executable line
        fs::write(&path, "# alias tau='taurine'\n").unwrap();

        assert!(
            !line_exists_in_rc_file(&path, "alias tau='taurine'"),
            "should not match a commented-out line"
        );
    }

    #[test]
    fn test_append_line_to_existing_file_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".testrc");
        fs::write(&path, "export EDITOR=vim\n").unwrap();

        let added = append_line_to_rc_file(&path, "alias bar='baz'").unwrap();
        assert!(added, "new line should be appended");

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("alias bar='baz'"));
        assert!(content.contains("export EDITOR=vim"));

        // Idempotent
        let added_again = append_line_to_rc_file(&path, "alias bar='baz'").unwrap();
        assert!(!added_again, "second append should be no-op");
    }

    #[test]
    fn test_detect_shell_profiles_returns_some_paths() {
        let profiles = detect_shell_profiles();
        // Should always return at least one candidate path on non-Windows
        if !cfg!(target_os = "windows") {
            assert!(!profiles.is_empty(), "should detect at least one profile");
        }
    }
}
