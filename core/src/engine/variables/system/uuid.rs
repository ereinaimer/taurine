use uuid::Uuid;

/// Resolves `uuid` and `uuid.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if key == "uuid" || key == "uuid.v4" {
        return Some(Uuid::new_v4().to_string());
    }
    if key == "uuid.v7" {
        return Some(Uuid::now_v7().to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_uuid_bare_returns_v4() {
        let res = resolve("uuid").unwrap();
        assert_eq!(res.len(), 36);
        assert!(res.contains('-'));
    }

    #[test]
    fn test_resolve_uuid_v4_explicit() {
        let res = resolve("uuid.v4").unwrap();
        assert_eq!(res.len(), 36);
        assert!(res.contains('-'));
    }

    #[test]
    fn test_resolve_uuid_v7() {
        let res = resolve("uuid.v7").unwrap();
        assert_eq!(res.len(), 36);
        assert!(res.contains('-'));
    }
}
