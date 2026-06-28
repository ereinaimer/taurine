use std::env;

/// Resolves `env(...)` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("env(") || !key.ends_with(')') {
        return None;
    }

    let raw_var = key[4..key.len() - 1].trim();
    let var_name = crate::engine::variables::system::strip_quotes(raw_var).unwrap_or(raw_var);
    if var_name.is_empty() {
        return None;
    }
    env::var(var_name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_var() {
        let _guard = crate::testing::TEST_LOCK.lock().unwrap();
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
        unsafe { env::remove_var("TAURINE_TEST_VAR") };
    }

    #[test]
    fn test_resolve_missing_env_var() {
        assert_eq!(resolve("env(NON_EXISTENT_VAR_12345)"), None);
    }
}
