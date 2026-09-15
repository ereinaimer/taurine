use uuid::Uuid;

/// Resolves the unified `uuid(...)` system variable.
///
/// `raw` is the argument list inside `uuid(...)` (`""` when bare),
/// bound as `(version=v4)` with `version` in `{v4, v7}`.
pub fn resolve(raw: &str) -> Option<String> {
    let spec = crate::engine::variables::registry::param_spec("uuid")?;
    let bound = crate::engine::variables::parser::bind_call("uuid", raw, &spec).ok()?;
    if bound.positional.len() > spec.params.len() {
        return None;
    }
    match bound.named.get("version").map(String::as_str) {
        Some("v4") => Some(Uuid::new_v4().to_string()),
        Some("v7") => Some(Uuid::now_v7().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_versions() {
        assert_eq!(resolve("").unwrap().len(), 36); // bare = v4
        assert_eq!(resolve("v4").unwrap().len(), 36);
        assert_eq!(resolve("v7").unwrap().len(), 36);
        assert_eq!(resolve("version=v4").unwrap().len(), 36);
        assert_eq!(resolve("version=v7").unwrap().len(), 36);
        assert_eq!(resolve("4"), None); // bare numbers rejected
        assert_eq!(resolve("7"), None);
        assert_eq!(resolve("v1"), None);
    }
}
