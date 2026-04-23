use crc32fast::Hasher;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};

pub fn apply(transformer: &str, args: &[&str], content: &str) -> Option<String> {
    if !args.is_empty() {
        return None;
    }

    match transformer {
        "md5" => Some(format!("{:x}", md5::compute(content.as_bytes()))),
        "sha1" => Some(format!("{:x}", Sha1::digest(content.as_bytes()))),
        "sha256" => Some(format!("{:x}", Sha256::digest(content.as_bytes()))),
        "sha512" => Some(format!("{:x}", Sha512::digest(content.as_bytes()))),
        "crc32" => Some(crc32(content)),
        "rot13" => Some(rot13(content)),
        _ => None,
    }
}

fn crc32(content: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(content.as_bytes());
    format!("{:08x}", hasher.finalize())
}

fn rot13(content: &str) -> String {
    content
        .chars()
        .map(|ch| match ch {
            'a'..='z' => (((ch as u8 - b'a' + 13) % 26) + b'a') as char,
            'A'..='Z' => (((ch as u8 - b'A' + 13) % 26) + b'A') as char,
            _ => ch,
        })
        .collect()
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
            apply("sha1", &[], "hello"),
            Some("aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d".to_string())
        );
        assert_eq!(
            apply("sha256", &[], "hello"),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_string())
        );
        assert_eq!(
            apply("sha512", &[], "hello"),
            Some("9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043".to_string())
        );
        assert_eq!(apply("crc32", &[], "hello"), Some("3610a686".to_string()));
        assert_eq!(
            apply("rot13", &[], "Hello, World!"),
            Some("Uryyb, Jbeyq!".to_string())
        );
    }
}
