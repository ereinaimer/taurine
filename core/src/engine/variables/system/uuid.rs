use uuid::Uuid;

/// Resolves `uuid.*` system variables.
pub fn resolve(key: &str) -> Option<String> {
    if key == "uuid" {
        return Some(Uuid::new_v4().to_string());
    }

    if !key.starts_with("uuid.") {
        return None;
    }

    let sub_key = &key[5..];
    match sub_key {
        "v4" => Some(Uuid::new_v4().to_string()),
        "v7" => Some(Uuid::now_v7().to_string()),
        "simple" => Some(Uuid::new_v4().simple().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_uuid_v4() {
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

    #[test]
    fn test_resolve_uuid_simple() {
        let res = resolve("uuid.simple").unwrap();
        assert_eq!(res.len(), 32);
        assert!(!res.contains('-'));
    }
}
