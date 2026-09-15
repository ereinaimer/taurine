use std::env;

/// Resolves the unified `env(...)` system variable.
///
/// `raw` is the argument list inside `env(...)`, bound as `(name, [default])`.
pub fn resolve(raw: &str) -> Option<String> {
    let spec = crate::engine::variables::registry::param_spec("env")?;
    let bound = crate::engine::variables::parser::bind_call("env", raw, &spec).ok()?;
    if bound.positional.len() > spec.params.len() {
        return None;
    }
    let var_name = bound.named.get("name").map(String::as_str).unwrap_or("");
    let default_val = bound.named.get("default").map(String::as_str).unwrap_or("");
    if var_name.is_empty() {
        return (!default_val.is_empty()).then(|| default_val.to_string());
    }

    match env::var(var_name) {
        Ok(val) => Some(val),
        Err(_) => (!default_val.is_empty()).then(|| default_val.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_unified() {
        let _guard = crate::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::set_var("TAURINE_TEST_VAR", "hello_world") };
        assert_eq!(resolve("TAURINE_TEST_VAR"), Some("hello_world".to_string()));
        assert_eq!(
            resolve("TAURINE_TEST_VAR, fallback"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            resolve("name=TAURINE_TEST_VAR, default=fallback"),
            Some("hello_world".to_string())
        );
        assert_eq!(
            resolve("NON_EXISTENT_VAR_12345, fallback"),
            Some("fallback".to_string())
        );
        assert_eq!(resolve("NON_EXISTENT_VAR_12345"), None);
        assert_eq!(resolve(""), None);
        assert_eq!(resolve("X=y"), None);
        // SAFETY: Serialized via TEST_LOCK to prevent concurrent environment modification races.
        unsafe { env::remove_var("TAURINE_TEST_VAR") };
    }
}
