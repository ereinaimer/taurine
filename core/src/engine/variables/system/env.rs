use std::env;

/// Resolves `env(...)` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("env(") || !key.ends_with(')') {
        return None;
    }

    let raw_var = key[4..key.len() - 1].trim();
    let mut parts = raw_var.splitn(2, '=');
    let var_name_part = parts.next().unwrap_or("").trim();
    let default_val = parts.next().map(|s| s.trim());

    let var_name =
        crate::engine::variables::system::strip_quotes(var_name_part).unwrap_or(var_name_part);
    if var_name.is_empty() {
        return default_val.map(|s| {
            crate::engine::variables::system::strip_quotes(s)
                .unwrap_or(s)
                .to_string()
        });
    }

    match env::var(var_name) {
        Ok(val) => Some(val),
        Err(_) => default_val.map(|s| {
            crate::engine::variables::system::strip_quotes(s)
                .unwrap_or(s)
                .to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_var() {
        let _guard = crate::testing::TEST_LOCK.lock().unwrap();
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_TEST_VAR", "hello_world") };
        assert_eq!(
            resolve("env(TAURINE_TEST_VAR)"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            resolve("env(\"TAURINE_TEST_VAR\")"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            resolve("env('TAURINE_TEST_VAR')"),
            Some("hello_world".to_string())
        );
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_TEST_VAR") };
    }

    #[test]
    fn test_resolve_missing_env_var() {
        assert_eq!(resolve("env(NON_EXISTENT_VAR_12345)"), None);
    }

    #[test]
    fn test_resolve_env_var_with_default() {
        assert_eq!(
            resolve("env(NON_EXISTENT_VAR_12345=admin)"),
            Some("admin".to_string())
        );
        assert_eq!(
            resolve("env(NON_EXISTENT_VAR_12345=\"admin space\")"),
            Some("admin space".to_string())
        );
    }
}
