use serde::{Deserialize, Serialize};

use super::crypto::{
    clear_vault_dek, decrypt_payload, derive_vault_keys, get_or_create_vault_dek, get_vault_dek,
    store_vault_dek,
};
use crate::testing::{TEST_LOCK, use_shared_test_keyring};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SamplePayload {
    id: String,
    content: String,
    execution_count: u32,
    tags: Vec<String>,
}

#[test]
fn test_derive_vault_keys_determinism_and_differentiation() {
    let password = "super-secret-password-123!";
    let user_id = "user-uuid-9876-5432-10";

    let (auth_hash_1, dek_1) =
        derive_vault_keys(password, user_id).expect("key derivation should succeed");
    let (auth_hash_2, dek_2) =
        derive_vault_keys(password, user_id).expect("key derivation should succeed");

    // Keys must be deterministic
    assert_eq!(auth_hash_1, auth_hash_2);
    assert_eq!(dek_1, dek_2);

    // Auth hash and DEK must be independent and not identical
    assert_ne!(auth_hash_1, dek_1);
    assert_ne!(auth_hash_1, [0u8; 32]);
    assert_ne!(dek_1, [0u8; 32]);

    // Different user_id (salt) produces different keys
    let (auth_hash_diff_user, dek_diff_user) =
        derive_vault_keys(password, "different-user-uuid").expect("key derivation should succeed");
    assert_ne!(auth_hash_1, auth_hash_diff_user);
    assert_ne!(dek_1, dek_diff_user);

    // Different password produces different keys
    let (auth_hash_diff_pw, dek_diff_pw) = derive_vault_keys("different-password-456!", user_id)
        .expect("key derivation should succeed");
    assert_ne!(auth_hash_1, auth_hash_diff_pw);
    assert_ne!(dek_1, dek_diff_pw);

    // Empty credentials rejected
    assert!(derive_vault_keys("", user_id).is_err());
    assert!(derive_vault_keys(password, "").is_err());
}

#[test]
fn test_payload_encryption_and_decryption_roundtrip() {
    let key = [0x5au8; 32];
    let payload = SamplePayload {
        id: "snippet-uuid-1".to_string(),
        content: "print('Hello world!')".to_string(),
        execution_count: 42,
        tags: vec!["python".to_string(), "script".to_string()],
    };

    let envelope =
        super::crypto::encrypt_payload(&key, &payload).expect("payload encryption should succeed");

    assert_eq!(envelope.v, 1);
    assert_eq!(envelope.nonce.len(), 24); // 12 bytes hex-encoded
    assert!(!envelope.ciphertext.is_empty());

    let decrypted: SamplePayload =
        decrypt_payload(&key, &envelope).expect("payload decryption should succeed");

    assert_eq!(decrypted, payload);
}

#[test]
fn test_payload_decryption_tampering_and_key_mismatch() {
    let key = [0x42u8; 32];
    let wrong_key = [0x43u8; 32];

    let payload = SamplePayload {
        id: "secret-1".to_string(),
        content: "classified".to_string(),
        execution_count: 1,
        tags: vec![],
    };

    let envelope =
        super::crypto::encrypt_payload(&key, &payload).expect("encryption should succeed");

    // Decryption with wrong key must fail
    let err_wrong_key = decrypt_payload::<SamplePayload>(&wrong_key, &envelope);
    assert!(err_wrong_key.is_err());

    // Tampered ciphertext must fail authentication
    let mut tampered_ct_envelope = envelope.clone();
    let mut ct_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        &tampered_ct_envelope.ciphertext,
    )
    .expect("base64 decode");
    if let Some(first) = ct_bytes.first_mut() {
        *first ^= 0xff;
    }
    tampered_ct_envelope.ciphertext =
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &ct_bytes);
    let err_tampered_ct = decrypt_payload::<SamplePayload>(&key, &tampered_ct_envelope);
    assert!(err_tampered_ct.is_err());

    // Tampered nonce must fail
    let mut tampered_nonce_envelope = envelope.clone();
    tampered_nonce_envelope.nonce = "00".repeat(12);
    let err_tampered_nonce = decrypt_payload::<SamplePayload>(&key, &tampered_nonce_envelope);
    assert!(err_tampered_nonce.is_err());

    // Unsupported envelope version must fail
    let mut invalid_version_envelope = envelope.clone();
    invalid_version_envelope.v = 99;
    let err_version = decrypt_payload::<SamplePayload>(&key, &invalid_version_envelope);
    assert!(err_version.is_err());
}

#[test]
fn test_vault_dek_keyring_lifecycle() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use_shared_test_keyring();

    // Ensure clean state
    clear_vault_dek().expect("clear_vault_dek should succeed");
    let initial = get_vault_dek().expect("get_vault_dek should succeed");
    assert_eq!(initial, None);

    let test_dek = [0x77u8; 32];
    store_vault_dek(&test_dek).expect("store_vault_dek should succeed");

    let retrieved = get_vault_dek()
        .expect("get_vault_dek should succeed")
        .expect("dek should exist");
    assert_eq!(retrieved, test_dek);

    // Overwrite with a new key
    let updated_dek = [0x88u8; 32];
    store_vault_dek(&updated_dek).expect("overwrite should succeed");
    let retrieved_updated = get_vault_dek()
        .expect("get_vault_dek should succeed")
        .expect("dek should exist");
    assert_eq!(retrieved_updated, updated_dek);

    // Clear
    clear_vault_dek().expect("clear_vault_dek should succeed");
    let after_clear = get_vault_dek().expect("get_vault_dek should succeed");
    assert_eq!(after_clear, None);

    // Clearing again is idempotent
    clear_vault_dek().expect("idempotent clear should succeed");
}

#[test]
fn test_get_or_create_vault_dek() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    use_shared_test_keyring();

    clear_vault_dek().expect("clear should succeed");

    // First call creates a new random DEK and stores it
    let dek1 = get_or_create_vault_dek().expect("get_or_create should create dek");
    assert_ne!(dek1, [0u8; 32]);

    // Second call retrieves the stored DEK
    let dek2 = get_or_create_vault_dek().expect("get_or_create should fetch existing dek");
    assert_eq!(dek1, dek2);

    // Clearing removes it
    clear_vault_dek().expect("clear should succeed");
    let dek3 = get_or_create_vault_dek().expect("get_or_create should create another dek");
    assert_ne!(dek3, [0u8; 32]);
}
