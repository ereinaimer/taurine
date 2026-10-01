use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Standard RFC 2104 HMAC-SHA256 calculation.
pub(crate) fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut k_pad = [0u8; 64];
    if key.len() > 64 {
        let hash = Sha256::digest(key);
        k_pad[..32].copy_from_slice(&hash);
    } else {
        k_pad[..key.len()].copy_from_slice(key);
    }

    let mut k_ipad = [0x36u8; 64];
    let mut k_opad = [0x5cu8; 64];
    for i in 0..64 {
        k_ipad[i] ^= k_pad[i];
        k_opad[i] ^= k_pad[i];
    }

    let mut inner = Sha256::new();
    inner.update(k_ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(k_opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

/// Computes an RFC 2104 standard HMAC-SHA256 signature for quota ledger integrity.
///
/// Encodes the week epoch and remaining percentage into 16 bytes (little-endian)
/// and signs with the 32-byte secret key, returning a 64-character lowercase hexadecimal string.
pub fn compute_quota_hmac(key: &[u8; 32], week_epoch: i64, remaining_pct: f64) -> String {
    let mut data = [0u8; 16];
    data[..8].copy_from_slice(&week_epoch.to_le_bytes());
    data[8..].copy_from_slice(&remaining_pct.to_le_bytes());
    let digest = hmac_sha256(key, &data);
    hex::encode(digest)
}

/// Verifies an HMAC-SHA256 signature using constant-time equality to prevent timing attacks.
pub fn verify_quota_hmac(
    key: &[u8; 32],
    week_epoch: i64,
    remaining_pct: f64,
    signature: &str,
) -> bool {
    let expected = compute_quota_hmac(key, week_epoch, remaining_pct);
    if expected.len() != signature.len() {
        return false;
    }
    bool::from(expected.as_bytes().ct_eq(signature.as_bytes()))
}
