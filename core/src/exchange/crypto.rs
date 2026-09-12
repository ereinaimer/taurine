use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::random;
use subtle::ConstantTimeEq;
use zeroize::Zeroize;

// honey: obscurity only, not secrecy. Real confidentiality comes from the
// optional user password. Single shared key so every build reads every file.
const APP_KEY_MASK: [u8; 32] = [
    58, 184, 96, 160, 4, 242, 74, 216, 89, 107, 78, 197, 25, 174, 62, 51, 196, 149, 3, 106, 35, 31,
    167, 162, 204, 241, 197, 0, 19, 45, 44, 19,
];
const APP_KEY_OBFUSCATED: [u8; 32] = [
    104, 82, 99, 147, 127, 197, 217, 240, 83, 148, 81, 117, 51, 30, 200, 192, 61, 246, 159, 83,
    202, 246, 36, 6, 1, 16, 219, 72, 36, 218, 116, 184,
];

fn app_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    for i in 0..32 {
        key[i] = APP_KEY_MASK[i] ^ APP_KEY_OBFUSCATED[i];
    }
    key
}

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const WRAPPED_DEK_LEN: usize = KEY_LEN + 16;
const FLAG_HAS_PASSWORD: u8 = 0x01;
const MIN_NOPW_BLOB_LEN: usize = 4 + 1 + NONCE_LEN + NONCE_LEN + WRAPPED_DEK_LEN + 16;
const MIN_PW_BLOB_LEN: usize = MIN_NOPW_BLOB_LEN + SALT_LEN + NONCE_LEN + WRAPPED_DEK_LEN;

pub const MIN_EXPORT_PASSWORD_LEN: usize = 8;

pub fn validate_export_password(password: &str) -> crate::Result<()> {
    let trimmed = password.trim();
    if trimmed.is_empty() {
        return Err(crate::Error::Config(
            "Encryption password is required.".to_string(),
        ));
    }
    if password.len() < MIN_EXPORT_PASSWORD_LEN {
        return Err(crate::Error::Config(format!(
            "Encryption password must be at least {MIN_EXPORT_PASSWORD_LEN} characters long"
        )));
    }
    Ok(())
}

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

pub fn encrypt(plaintext: &[u8], password: Option<&str>) -> crate::Result<Vec<u8>> {
    if let Some(pw) = password {
        validate_export_password(pw)?;
    }
    let has_password = password.is_some();

    let mut dek: [u8; KEY_LEN] = random();
    let payload_nonce: [u8; NONCE_LEN] = random();
    let payload_ct = seal(&dek, &payload_nonce, plaintext)?;

    let nonce_a: [u8; NONCE_LEN] = random();
    let mut app = app_key();
    let wrap_app = seal(&app, &nonce_a, &dek)?;
    app.zeroize();

    let (salt, nonce_p, wrap_pw) = if let Some(pw) = password {
        let salt: [u8; SALT_LEN] = random();
        let mut pw_key = derive_key(pw, &salt)?;
        let nonce_p: [u8; NONCE_LEN] = random();
        let wrap_pw = seal(&pw_key, &nonce_p, &dek)?;
        pw_key.zeroize();
        (Some(salt), Some(nonce_p), Some(wrap_pw))
    } else {
        (None, None, None)
    };
    dek.zeroize();

    let mut blob = Vec::with_capacity(payload_ct.len() + MIN_PW_BLOB_LEN);
    blob.extend_from_slice(&super::TAU_MAGIC);
    blob.push(if has_password { FLAG_HAS_PASSWORD } else { 0 });
    if let Some(salt) = salt {
        blob.extend_from_slice(&salt);
    }
    blob.extend_from_slice(&payload_nonce);
    blob.extend_from_slice(&nonce_a);
    blob.extend_from_slice(&wrap_app);
    if let (Some(nonce_p), Some(wrap_pw)) = (nonce_p, wrap_pw) {
        blob.extend_from_slice(&nonce_p);
        blob.extend_from_slice(&wrap_pw);
    }
    blob.extend_from_slice(&payload_ct);
    Ok(blob)
}

pub fn decrypt(blob: &[u8], password: Option<&str>) -> crate::Result<Vec<u8>> {
    if blob.len() < MIN_NOPW_BLOB_LEN {
        return Err(crate::Error::Config("encrypted file too short".to_string()));
    }
    if blob[..4] != super::TAU_MAGIC {
        return Err(crate::Error::Config(
            "bad file header, expected TAU".to_string(),
        ));
    }
    let flags = blob[4];
    if flags & !FLAG_HAS_PASSWORD != 0 {
        return Err(crate::Error::Config("unsupported file flags".to_string()));
    }
    let has_password = flags & FLAG_HAS_PASSWORD != 0;

    let mut off = 5;
    let salt = if has_password {
        if blob.len() < MIN_PW_BLOB_LEN {
            return Err(crate::Error::Config("encrypted file too short".to_string()));
        }
        let s = &blob[off..off + SALT_LEN];
        off += SALT_LEN;
        Some(s)
    } else {
        None
    };
    let payload_nonce: [u8; NONCE_LEN] = blob[off..off + NONCE_LEN]
        .try_into()
        .map_err(|_| crate::Error::Config("Invalid nonce length".to_string()))?;
    off += NONCE_LEN;
    let nonce_a: [u8; NONCE_LEN] = blob[off..off + NONCE_LEN]
        .try_into()
        .map_err(|_| crate::Error::Config("Invalid nonce length".to_string()))?;
    off += NONCE_LEN;
    let wrap_app = &blob[off..off + WRAPPED_DEK_LEN];
    off += WRAPPED_DEK_LEN;
    let wrap_pw = if has_password {
        let nonce_p: [u8; NONCE_LEN] = blob[off..off + NONCE_LEN]
            .try_into()
            .map_err(|_| crate::Error::Config("Invalid nonce length".to_string()))?;
        off += NONCE_LEN;
        let ct = &blob[off..off + WRAPPED_DEK_LEN];
        off += WRAPPED_DEK_LEN;
        Some((nonce_p, ct))
    } else {
        None
    };
    let payload_ct = &blob[off..];

    if has_password && password.is_none_or(|p| p.trim().is_empty()) {
        return Err(crate::Error::Config(
            "password required for encrypted file".to_string(),
        ));
    }

    let mut app = app_key();
    let dek_app = match open(&app, &nonce_a, wrap_app) {
        Ok(dek) => dek,
        Err(_) => {
            app.zeroize();
            return Err(crate::Error::Config(
                "file is corrupted and cannot be imported".to_string(),
            ));
        }
    };
    app.zeroize();
    let mut dek_app_vec = dek_app;
    let mut dek_app_arr: [u8; KEY_LEN] = dek_app_vec.as_slice().try_into().map_err(|_| {
        dek_app_vec.zeroize();
        crate::Error::Config("file is corrupted and cannot be imported".to_string())
    })?;
    dek_app_vec.zeroize();

    let mut dek: [u8; KEY_LEN] = if has_password {
        let pw = password.unwrap_or("");
        let salt = salt.unwrap_or(&[]);
        let mut pw_key = derive_key(pw, salt)?;
        let (nonce_p, wrap_pw_ct) = wrap_pw.ok_or_else(|| {
            dek_app_arr.zeroize();
            crate::Error::Config("file is corrupted and cannot be imported".to_string())
        })?;
        let dek_pw = match open(&pw_key, &nonce_p, wrap_pw_ct) {
            Ok(dek) => dek,
            Err(_) => {
                pw_key.zeroize();
                dek_app_arr.zeroize();
                return Err(crate::Error::Config("wrong password".to_string()));
            }
        };
        pw_key.zeroize();
        if dek_pw.as_slice().ct_eq(dek_app_arr.as_slice()).unwrap_u8() != 1 {
            dek_app_arr.zeroize();
            return Err(crate::Error::Config(
                "file is corrupted and cannot be imported".to_string(),
            ));
        }
        dek_app_arr.zeroize();
        let mut dek_pw_vec = dek_pw;
        let arr: [u8; KEY_LEN] = dek_pw_vec.as_slice().try_into().map_err(|_| {
            dek_pw_vec.zeroize();
            crate::Error::Config("file is corrupted and cannot be imported".to_string())
        })?;
        dek_pw_vec.zeroize();
        arr
    } else {
        dek_app_arr
    };

    let plain = open(&dek, &payload_nonce, payload_ct).map_err(|_| {
        crate::Error::Config("file is corrupted and cannot be imported".to_string())
    })?;
    dek.zeroize();
    Ok(plain)
}

fn seal(key: &[u8], nonce: &[u8; NONCE_LEN], plaintext: &[u8]) -> crate::Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| crate::Error::Service("Invalid AES-256-GCM key length".to_string()))?;
    cipher
        .encrypt(&Nonce::from(*nonce), plaintext)
        .map_err(|_| crate::Error::Service("Failed to encrypt exchange payload".to_string()))
}

fn open(key: &[u8], nonce: &[u8; NONCE_LEN], ciphertext: &[u8]) -> crate::Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|_| crate::Error::Service("Invalid AES-256-GCM key length".to_string()))?;
    cipher
        .decrypt(&Nonce::from(*nonce), ciphertext)
        .map_err(|_| crate::Error::Config("file is corrupted and cannot be imported".to_string()))
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
    fn encrypt_decrypt_round_trips_passwordless() {
        let blob = encrypt(br#"{"schema_version":1,"triggers":[]}"#, None).unwrap();
        assert_eq!(&blob[..4], b"TAU\x00");
        assert_eq!(blob[4], 0x00);

        let plaintext = decrypt(&blob, None).unwrap();
        assert_eq!(plaintext, br#"{"schema_version":1,"triggers":[]}"#);
    }

    #[test]
    fn encrypt_decrypt_round_trips_with_password() {
        let blob = encrypt(br#"{"schema_version":1,"triggers":[]}"#, Some("hunter22")).unwrap();
        assert_eq!(&blob[..4], b"TAU\x00");
        assert_eq!(blob[4] & 0x01, 0x01);

        let plaintext = decrypt(&blob, Some("hunter22")).unwrap();
        assert_eq!(plaintext, br#"{"schema_version":1,"triggers":[]}"#);
    }

    #[test]
    fn encrypted_blob_is_opaque() {
        let blob = encrypt(br#"{"schema_version":1}"#, None).unwrap();
        assert!(
            !blob
                .windows(b"schema_version".len())
                .any(|window| window == b"schema_version"),
        );
    }

    #[test]
    fn validate_export_password_rejects_empty_and_short_passwords() {
        assert!(validate_export_password("").is_err());
        assert!(validate_export_password("   ").is_err());
        assert!(validate_export_password("1234567").is_err());
        assert!(validate_export_password("12345678").is_ok());
    }

    #[test]
    fn encrypt_fails_when_password_too_short() {
        let res = encrypt(b"test data", Some("short"));
        assert!(res.is_err());
        assert!(
            res.unwrap_err()
                .to_string()
                .contains("at least 8 characters")
        );
    }

    #[test]
    fn decrypt_rejects_wrong_password() {
        let blob = encrypt(b"top secret", Some("hunter22")).unwrap();
        let err = decrypt(&blob, Some("wrong password")).unwrap_err();

        assert_eq!(err.to_string(), "wrong password");
    }

    #[test]
    fn decrypt_requires_password_when_flag_set() {
        let blob = encrypt(b"top secret", Some("hunter22")).unwrap();
        let err = decrypt(&blob, None).unwrap_err();

        assert!(err.to_string().contains("password required"));
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let mut blob = encrypt(b"top secret", Some("hunter22")).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x01;

        let err = decrypt(&blob, Some("hunter22")).unwrap_err();
        assert!(
            err.to_string()
                .contains("file is corrupted and cannot be imported")
        );
    }

    #[test]
    fn decrypt_rejects_tampered_passwordless_ciphertext() {
        let mut blob = encrypt(b"top secret", None).unwrap();
        let last = blob.len() - 1;
        blob[last] ^= 0x01;

        let err = decrypt(&blob, None).unwrap_err();
        assert!(
            err.to_string()
                .contains("file is corrupted and cannot be imported")
        );
    }

    #[test]
    fn decrypt_rejects_invalid_magic_bytes() {
        let mut blob = vec![0u8; 128];
        blob[..4].copy_from_slice(b"TAUP");

        let err = decrypt(&blob, None).unwrap_err();
        assert!(err.to_string().contains("TAU"));
    }

    #[test]
    fn encrypt_decrypt_round_trips_empty_plaintext() {
        for password in [None, Some("hunter22")] {
            let blob = encrypt(b"", password).unwrap();
            assert_eq!(decrypt(&blob, password).unwrap(), b"");
        }
    }

    #[test]
    fn decrypt_rejects_unknown_flags() {
        let mut blob = encrypt(b"top secret", None).unwrap();
        blob[4] = 0x02;

        let err = decrypt(&blob, None).unwrap_err();
        assert!(err.to_string().contains("unsupported file flags"));
    }

    #[test]
    fn decrypt_treats_whitespace_password_as_missing() {
        let blob = encrypt(b"top secret", Some("hunter22")).unwrap();
        let err = decrypt(&blob, Some("   ")).unwrap_err();
        assert!(err.to_string().contains("password required"));
    }

    #[test]
    fn decrypt_rejects_truncated_header() {
        let blob = encrypt(b"top secret", Some("hunter22")).unwrap();
        let err = decrypt(&blob[..20], Some("hunter22")).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }
}
