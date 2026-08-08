use sha2::{Digest, Sha256, Sha512};

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if !args.is_empty() {
        return None;
    }

    match transformer {
        "sha256" => Some(hex::encode(Sha256::digest(content.as_bytes()))),
        "sha512" => Some(hex::encode(Sha512::digest(content.as_bytes()))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crypto_transformers() {
        assert_eq!(
            apply("sha256", &[], "hello"),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_string())
        );
        assert_eq!(
            apply("sha512", &[], "hello"),
            Some("9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043".to_string())
        );
    }
}
