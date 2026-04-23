use sha2::{Digest, Sha256};

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if !args.is_empty() {
        return None;
    }

    match transformer {
        "md5" => Some(format!("{:x}", md5::compute(content.as_bytes()))),
        "sha256" => Some(format!("{:x}", Sha256::digest(content.as_bytes()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_transformers() {
        assert_eq!(
            apply("md5", &[], "hello"),
            Some("5d41402abc4b2a76b9719d911017c592".to_string())
        );
        assert_eq!(
            apply("sha256", &[], "hello"),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_string())
        );
    }
}
