use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::random;
use zeroize::Zeroize;

use super::ENCRYPTED_MAGIC_HEADER;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const MIN_ENCRYPTED_BLOB_LEN: usize = 4 + SALT_LEN + NONCE_LEN + 16;

pub fn derive_key(password: &str, salt: &[u8]) -> crate::Result<[u8; KEY_LEN]> {
    if salt.len() != SALT_LEN {
        return Err(crate::Error::Config(format!(
            "Invalid salt length: expected {SALT_LEN} bytes, got {}",
            salt.len()
        )));
    }

    let params = Params::new(64 * 1024, 3, 4, Some(KEY_LEN)).map_err(|e| {
        crate::Error::Service(format!(
            "Invalid Argon2 parameters for exchange crypto: {e}"
        ))
    })?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; KEY_LEN];
    let mut password_bytes = password.as_bytes().to_vec();
    let result = argon2.hash_password_into(&password_bytes, salt, &mut key);
    password_bytes.zeroize();

    result.map_err(|e| crate::Error::Service(format!("Argon2id key derivation failed: {e}")))?;
    Ok(key)
}

pub fn encrypt(plaintext: &[u8], password: &str) -> crate::Result<Vec<u8>> {
    let salt: [u8; SALT_LEN] = random();
    let nonce_bytes: [u8; NONCE_LEN] = random();
    let mut key = derive_key(password, &salt)?;

    let cipher_res = Aes256Gcm::new_from_slice(&key);
    let nonce = Nonce::from(nonce_bytes);
    let encrypt_res = match cipher_res {
        Ok(cipher) => cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| crate::Error::Service("Failed to encrypt exchange payload".to_string())),
        Err(_) => Err(crate::Error::Service(
            "Invalid AES-256-GCM key length".to_string(),
        )),
    };
    key.zeroize();

    let ciphertext = encrypt_res?;

    let mut blob = Vec::with_capacity(4 + SALT_LEN + NONCE_LEN + ciphertext.len());
    blob.extend_from_slice(&ENCRYPTED_MAGIC_HEADER);
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce_bytes);
    blob.extend_from_slice(&ciphertext);
    Ok(blob)
}

pub fn decrypt(blob: &[u8], password: &str) -> crate::Result<Vec<u8>> {
    if blob.len() < MIN_ENCRYPTED_BLOB_LEN {
        return Err(crate::Error::Config("encrypted file too short".to_string()));
    }

    if blob[..4] != ENCRYPTED_MAGIC_HEADER {
        return Err(crate::Error::Config(
            "bad file header, expected TAU1".to_string(),
        ));
    }

    let salt = &blob[4..4 + SALT_LEN];
    let nonce = &blob[4 + SALT_LEN..4 + SALT_LEN + NONCE_LEN];
    let ciphertext = &blob[4 + SALT_LEN + NONCE_LEN..];

    let mut key = derive_key(password, salt)?;
    let cipher_res = Aes256Gcm::new_from_slice(&key);
    let nonce_arr_res: Result<[u8; NONCE_LEN], _> = nonce.try_into();
    let decrypt_res = match (cipher_res, nonce_arr_res) {
        (Ok(cipher), Ok(nonce_arr)) => cipher
            .decrypt(&Nonce::from(nonce_arr), ciphertext)
            .map_err(|_| crate::Error::Config("wrong password or corrupted file".to_string())),
        (Err(_), _) => Err(crate::Error::Service(
            "Invalid AES-256-GCM key length".to_string(),
        )),
        (_, Err(_)) => Err(crate::Error::Config("Invalid nonce length".to_string())),
    };
    key.zeroize();

    decrypt_res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_key_is_deterministic_for_same_password_and_salt() {
        let salt = [7u8; SALT_LEN];

        let key_a = derive_key("correct horse battery staple", &salt).unwrap();
        let key_b = derive_key("correct horse battery staple", &salt).unwrap();

        assert_eq!(key_a, key_b);
        assert_eq!(key_a.len(), KEY_LEN);
    }

    #[test]
    fn encrypt_decrypt_round_trips() {
        let blob = encrypt(br#"{"schema_version":1,"triggers":[]}"#, "hunter2").unwrap();
        assert_eq!(&blob[..4], &ENCRYPTED_MAGIC_HEADER);

        let plaintext = decrypt(&blob, "hunter2").unwrap();
        assert_eq!(plaintext, br#"{"schema_version":1,"triggers":[]}"#);
    }

    #[test]
    fn decrypt_rejects_wrong_password() {
        let blob = encrypt(b"top secret", "hunter2").unwrap();
        let err = decrypt(&blob, "wrong password").unwrap_err();

        assert!(err.to_string().contains("wrong password or corrupted file"));
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let mut blob = encrypt(b"top secret", "hunter2").unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x01;

        let err = decrypt(&blob, "hunter2").unwrap_err();
        assert!(err.to_string().contains("wrong password or corrupted file"));
    }

    #[test]
    fn decrypt_rejects_invalid_magic_bytes() {
        let mut blob = vec![0u8; MIN_ENCRYPTED_BLOB_LEN];
        blob[..4].copy_from_slice(b"TAUP");

        let err = decrypt(&blob, "hunter2").unwrap_err();
        assert!(err.to_string().contains("TAU1"));
    }
}
