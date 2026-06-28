use std::env;

/// Resolves `sys.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("sys.") {
        return None;
    }

    let sub_key = &key[4..];
    match sub_key {
        "os" => Some(env::consts::OS.to_string()),
        "osversion" => Some(os_info::get().to_string()),
        "arch" => Some(env::consts::ARCH.to_string()),
        "hostname" => Some(gethostname::gethostname().to_string_lossy().into_owned()),
        "user" => current_user(),
        _ => None,
    }
}

fn current_user() -> Option<String> {
    #[cfg(windows)]
    {
        env::var("USERNAME").ok()
    }

    #[cfg(not(windows))]
    {
        env::var("USER").ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_os() {
        let os = resolve("sys.os").unwrap();
        assert_eq!(os, env::consts::OS);
        assert!(matches!(os.as_str(), "windows" | "linux" | "macos"));
    }

    #[test]
    fn test_resolve_arch() {
        assert_eq!(resolve("sys.arch"), Some(env::consts::ARCH.to_string()));
    }

    #[test]
    fn test_resolve_user() {
        let expected = current_user().expect("current user env var should be present");
        assert_eq!(resolve("sys.user"), Some(expected));
    }

    #[test]
    fn test_resolve_hostname() {
        let expected = gethostname::gethostname().to_string_lossy().into_owned();
        assert!(!expected.trim().is_empty());
        assert_eq!(resolve("sys.hostname"), Some(expected));
    }

    #[test]
    fn test_resolve_osversion() {
        let osversion = resolve("sys.osversion").unwrap();
        assert_eq!(osversion, os_info::get().to_string());
        assert!(!osversion.trim().is_empty());
    }

    #[test]
    fn test_resolve_unknown_modifier() {
        assert_eq!(resolve("sys"), None);
        assert_eq!(resolve("sys.home"), None);
        assert_eq!(resolve("env(USER)"), None);
    }

    #[test]
    fn test_resolve_sys_transformer() {
        assert_eq!(
            crate::engine::variables::system::resolve("sys.os.upper"),
            Some(env::consts::OS.to_uppercase())
        );
    }
}
