//! Zero-Knowledge End-to-End Encryption (E2EE) module for Taurine Cloud.
//!
//! Provides client-side AEAD encryption (AES-256-GCM) for synchronization payloads,
//! deterministic Argon2id key derivation from user credentials, and OS keyring persistence
//! for the user's Data Encryption Key (DEK).

use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rand::random;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;
const ENVELOPE_VERSION: u32 = 1;

const KEYRING_SERVICE: &str = "taurine-cloud";
const KEYRING_USER_DEK: &str = "vault-dek";

/// An authenticated, encrypted envelope wrapping synchronized payload data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedEnvelope {
    pub v: u32,
    pub nonce: String,
    pub ciphertext: String,
}

/// Derives a pair of 256-bit cryptographic keys deterministically using Argon2id:
/// 1. `auth_hash`: Sent to the server for authentication / verification.
/// 2. `vault_dek`: Kept strictly local for client-side Zero-Knowledge payload encryption.
pub fn derive_vault_keys(
    password: &str,
    user_id: &str,
) -> crate::Result<([u8; KEY_LEN], [u8; KEY_LEN])> {
    if password.trim().is_empty() {
        return Err(crate::Error::Config(
            "Password cannot be empty for key derivation".to_string(),
        ));
    }
    if user_id.trim().is_empty() {
        return Err(crate::Error::Config(
            "User ID cannot be empty for key derivation".to_string(),
        ));
    }

    // Derive deterministic 16-byte salt from user_id using SHA-256 domain separation
    let mut hasher = Sha256::new();
    hasher.update(b"taurine:vault:salt:v1:");
    hasher.update(user_id.as_bytes());
    let salt_hash = hasher.finalize();
    let salt = &salt_hash[..SALT_LEN];

    // Configure Argon2id: 64MB memory, 3 iterations, 4 parallelism, 64 bytes output
    let params = Params::new(64 * 1024, 3, 4, Some(KEY_LEN * 2)).map_err(|e| {
        crate::Error::Service(format!("Invalid Argon2 parameters for vault crypto: {e}"))
    })?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut output_key = [0u8; KEY_LEN * 2];
    let mut password_bytes = password.as_bytes().to_vec();
    let result = argon2.hash_password_into(&password_bytes, salt, &mut output_key);
    password_bytes.zeroize();

    result.map_err(|e| crate::Error::Service(format!("Argon2id key derivation failed: {e}")))?;

    let mut auth_hash = [0u8; KEY_LEN];
    let mut vault_dek = [0u8; KEY_LEN];

    auth_hash.copy_from_slice(&output_key[..KEY_LEN]);
    vault_dek.copy_from_slice(&output_key[KEY_LEN..]);
    output_key.zeroize();

    Ok((auth_hash, vault_dek))
}

/// Encrypts an arbitrary serializable payload into an `EncryptedEnvelope` using AES-256-GCM.
pub fn encrypt_payload<T: Serialize>(
    key: &[u8; KEY_LEN],
    data: &T,
) -> crate::Result<EncryptedEnvelope> {
    let json_bytes = serde_json::to_vec(data)?;
    let nonce_bytes: [u8; NONCE_LEN] = random();

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| crate::Error::Service(format!("Failed to create cipher: {e}")))?;

    let ciphertext_bytes = cipher
        .encrypt(&Nonce::from(nonce_bytes), json_bytes.as_ref())
        .map_err(|e| crate::Error::Service(format!("AES-256-GCM encryption failed: {e}")))?;

    Ok(EncryptedEnvelope {
        v: ENVELOPE_VERSION,
        nonce: hex::encode(nonce_bytes),
        ciphertext: STANDARD.encode(&ciphertext_bytes),
    })
}

/// Decrypts an `EncryptedEnvelope` into the target deserializable payload using AES-256-GCM.
pub fn decrypt_payload<T: DeserializeOwned>(
    key: &[u8; KEY_LEN],
    envelope: &EncryptedEnvelope,
) -> crate::Result<T> {
    if envelope.v != ENVELOPE_VERSION {
        return Err(crate::Error::Service(format!(
            "Unsupported encrypted envelope version: {}",
            envelope.v
        )));
    }

    let nonce_bytes = hex::decode(&envelope.nonce)
        .map_err(|e| crate::Error::Service(format!("Invalid envelope nonce hex: {e}")))?;
    if nonce_bytes.len() != NONCE_LEN {
        return Err(crate::Error::Service(format!(
            "Invalid envelope nonce length: expected {NONCE_LEN}, got {}",
            nonce_bytes.len()
        )));
    }
    let mut nonce_fixed = [0u8; NONCE_LEN];
    nonce_fixed.copy_from_slice(&nonce_bytes);

    let ciphertext_bytes = STANDARD
        .decode(&envelope.ciphertext)
        .map_err(|e| crate::Error::Service(format!("Invalid envelope ciphertext base64: {e}")))?;

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| crate::Error::Service(format!("Failed to create cipher: {e}")))?;

    let plaintext_bytes = cipher
        .decrypt(&Nonce::from(nonce_fixed), ciphertext_bytes.as_ref())
        .map_err(|e| crate::Error::Service(format!("AES-256-GCM decryption failed: {e}")))?;

    let result = serde_json::from_slice(&plaintext_bytes)?;
    Ok(result)
}

/// Stores the user's Data Encryption Key (DEK) in the OS credential vault.
pub fn store_vault_dek(dek: &[u8; KEY_LEN]) -> crate::Result<()> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DEK).map_err(keystore_unavailable)?;
    let hex_dek = hex::encode(dek);
    entry.set_password(&hex_dek).map_err(keystore_unavailable)?;
    Ok(())
}

/// Retrieves the user's Data Encryption Key (DEK) from the OS credential vault, if present.
pub fn get_vault_dek() -> crate::Result<Option<[u8; KEY_LEN]>> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DEK).map_err(keystore_unavailable)?;
    match entry.get_password() {
        Ok(hex_dek) => {
            let bytes = hex::decode(hex_dek.trim()).map_err(|e| {
                crate::Error::Service(format!("Corrupt vault DEK in keystore: {e}"))
            })?;
            if bytes.len() != KEY_LEN {
                return Err(crate::Error::Service(format!(
                    "Invalid vault DEK length in keystore: expected {KEY_LEN}, got {}",
                    bytes.len()
                )));
            }
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&bytes);
            Ok(Some(key))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(keystore_unavailable(err)),
    }
}

/// Clears the user's Data Encryption Key (DEK) from the OS credential vault.
pub fn clear_vault_dek() -> crate::Result<()> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER_DEK).map_err(keystore_unavailable)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(keystore_unavailable(err)),
    }
}

/// Retrieves the existing vault DEK or securely creates a new random DEK and stores it in the OS keyring.
pub fn get_or_create_vault_dek() -> crate::Result<[u8; KEY_LEN]> {
    if let Some(existing) = get_vault_dek()? {
        return Ok(existing);
    }
    let new_dek: [u8; KEY_LEN] = random();
    store_vault_dek(&new_dek)?;
    Ok(new_dek)
}

fn keystore_unavailable(err: keyring::Error) -> crate::Error {
    crate::Error::Service(format!("Vault DEK keystore error: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TEST_LOCK, use_shared_test_keyring};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestData {
        title: String,
        count: u64,
    }

    #[test]
    fn test_internal_encryption_roundtrip() {
        let key = [0x99u8; 32];
        let data = TestData {
            title: "Internal test".to_string(),
            count: 100,
        };
        let envelope = encrypt_payload(&key, &data).unwrap();
        let decrypted: TestData = decrypt_payload(&key, &envelope).unwrap();
        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_internal_keyring_dek() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        use_shared_test_keyring();

        clear_vault_dek().unwrap();
        assert_eq!(get_vault_dek().unwrap(), None);

        let dek = [0x55u8; 32];
        store_vault_dek(&dek).unwrap();
        assert_eq!(get_vault_dek().unwrap(), Some(dek));

        clear_vault_dek().unwrap();
        assert_eq!(get_vault_dek().unwrap(), None);
    }
}
