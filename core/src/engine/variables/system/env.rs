use std::env;

/// Resolves `env.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if !key.starts_with("env.") {
        return None;
    }

    let var_name = &key[4..];
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
            resolve("env.TAURINE_TEST_VAR"),
            Some("hello_world".to_string())
        );
        unsafe { env::remove_var("TAURINE_TEST_VAR") };
    }

    #[test]
    fn test_resolve_missing_env_var() {
        assert_eq!(resolve("env.NON_EXISTENT_VAR_12345"), None);
    }
}
