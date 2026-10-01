//! Capability lease engine and monotonic clock drift verification.

use crate::cloud::device::get_device_hardware_id;
use crate::db::crud::tier::UserTier;
use crate::system::error::{Error, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Maximum lease validity TTL in seconds (72 hours).
pub const MAX_LEASE_TTL_SECONDS: i64 = 72 * 3600;

/// An offline capability lease signed by Taurine Cloud with up to 72 hours TTL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityLease {
    pub user_id: String,
    pub device_hw_id: String,
    pub tier: UserTier,
    pub issued_at: i64,    // Unix timestamp (seconds)
    pub expires_at: i64,   // Unix timestamp (seconds, max 72h from issued_at)
    pub signature: String, // Hex-encoded Ed25519 signature
}

/// Pairs monotonic `Instant` with wall-clock `SystemTime` to detect clock tampering,
/// backwards rollbacks, and excessive forward jumps.
#[derive(Debug, Clone)]
pub struct TimeAnchor {
    pub instant: Instant,
    pub system_time: SystemTime,
}

impl Default for TimeAnchor {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeAnchor {
    /// Creates a new `TimeAnchor` capturing the current monotonic and wall-clock times.
    pub fn new() -> Self {
        Self {
            instant: Instant::now(),
            system_time: SystemTime::now(),
        }
    }

    /// Verifies that the system clock has not been tampered with or rolled back relative to
    /// the monotonic time anchor.
    pub fn check_clock_tampering(
        &self,
        now_instant: Instant,
        now_system_time: SystemTime,
    ) -> Result<()> {
        // Rollback check 1: System clock must not be earlier than anchor system time
        if now_system_time < self.system_time {
            return Err(Error::ClockDrift(
                "Clock rollback detected: system time is earlier than anchor time".to_string(),
            ));
        }

        // Monotonic check: now_instant must not be earlier than anchor instant
        if now_instant < self.instant {
            return Err(Error::ClockDrift(
                "Monotonic clock anomaly: now_instant is earlier than anchor instant".to_string(),
            ));
        }

        let mono_elapsed = now_instant.duration_since(self.instant);
        let sys_elapsed = now_system_time
            .duration_since(self.system_time)
            .map_err(|_| {
                Error::ClockDrift(
                    "Clock rollback detected: system time is earlier than anchor time".to_string(),
                )
            })?;

        // Rollback check 2: Monotonic clock elapsed significantly exceeds system clock elapsed
        // (Allows up to 15s of small NTP backward jitter/slewing, but rejects deliberate rollback)
        if mono_elapsed > sys_elapsed + Duration::from_secs(15) {
            return Err(Error::ClockDrift(
                "Clock rollback detected: system clock drifted backward relative to monotonic clock"
                    .to_string(),
            ));
        }

        // Forward jump check: System clock jumped > 3600s (1 hour) beyond monotonic drift
        if sys_elapsed > mono_elapsed && (sys_elapsed - mono_elapsed) > Duration::from_secs(3600) {
            let forward_drift = sys_elapsed - mono_elapsed;
            return Err(Error::ClockDrift(format!(
                "Clock forward jump detected: system time jumped {}s beyond monotonic drift",
                forward_drift.as_secs()
            )));
        }

        Ok(())
    }
}

/// Generates a deterministic byte payload for Ed25519 signing and verification.
pub fn canonical_lease_payload(
    user_id: &str,
    device_hw_id: &str,
    tier: UserTier,
    issued_at: i64,
    expires_at: i64,
) -> Vec<u8> {
    let tier_str = match tier {
        UserTier::Free => "free",
        UserTier::Pro => "pro",
        UserTier::Max => "max",
        UserTier::Team => "team",
    };
    format!("{user_id}|{device_hw_id}|{tier_str}|{issued_at}|{expires_at}").into_bytes()
}

/// Signs a capability lease using an Ed25519 signing key.
pub fn sign_lease(
    user_id: &str,
    device_hw_id: &str,
    tier: UserTier,
    issued_at: i64,
    expires_at: i64,
    signing_key: &SigningKey,
) -> CapabilityLease {
    let payload = canonical_lease_payload(user_id, device_hw_id, tier, issued_at, expires_at);
    let signature = signing_key.sign(&payload);
    CapabilityLease {
        user_id: user_id.to_string(),
        device_hw_id: device_hw_id.to_string(),
        tier,
        issued_at,
        expires_at,
        signature: hex::encode(signature.to_bytes()),
    }
}

/// Validates a capability lease against the current device hardware ID, monotonic clock anchor,
/// TTL bounds (<= 72h), and Ed25519 signature.
pub fn validate_lease(
    lease: &CapabilityLease,
    anchor: &TimeAnchor,
    now_instant: Instant,
    now_system_time: SystemTime,
    verifying_key_bytes: &[u8; 32],
) -> Result<()> {
    validate_lease_with_hw_id(
        lease,
        anchor,
        now_instant,
        now_system_time,
        verifying_key_bytes,
        &get_device_hardware_id(),
    )
}

/// Validates a capability lease against an explicit expected hardware ID.
pub fn validate_lease_with_hw_id(
    lease: &CapabilityLease,
    anchor: &TimeAnchor,
    now_instant: Instant,
    now_system_time: SystemTime,
    verifying_key_bytes: &[u8; 32],
    expected_hw_id: &str,
) -> Result<()> {
    // 1. Clock tampering / rollback check via TimeAnchor
    anchor.check_clock_tampering(now_instant, now_system_time)?;

    // 2. Device HW ID check
    if lease.device_hw_id != expected_hw_id {
        return Err(Error::Lease(format!(
            "Device hardware ID mismatch: expected {expected_hw_id}, got {}",
            lease.device_hw_id
        )));
    }

    // 3. TTL <= 72 hours check
    if lease.expires_at < lease.issued_at {
        return Err(Error::Lease(
            "Lease expires_at is before issued_at".to_string(),
        ));
    }
    let ttl_seconds = lease.expires_at - lease.issued_at;
    if ttl_seconds > MAX_LEASE_TTL_SECONDS {
        return Err(Error::Lease(format!(
            "Lease TTL exceeds maximum allowed 72 hours (got {ttl_seconds}s)"
        )));
    }

    // 4. Expiration and future-issued checks against now
    let now_epoch = now_system_time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::ClockDrift("System time is before Unix epoch".to_string()))?
        .as_secs() as i64;

    if now_epoch < lease.issued_at {
        return Err(Error::Lease(
            "Lease is not yet valid (issued in the future)".to_string(),
        ));
    }

    if now_epoch >= lease.expires_at {
        return Err(Error::Lease("Lease has expired".to_string()));
    }

    // 5. Ed25519 signature validity over canonical payload
    let verifying_key = VerifyingKey::from_bytes(verifying_key_bytes)
        .map_err(|e| Error::Lease(format!("Invalid Ed25519 verifying key: {e}")))?;

    let sig_bytes = hex::decode(&lease.signature)
        .map_err(|e| Error::Lease(format!("Invalid signature hex: {e}")))?;

    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| Error::Lease("Invalid signature length: expected 64 bytes".to_string()))?;

    let signature = Signature::from_bytes(&sig_array);

    let canonical_payload = canonical_lease_payload(
        &lease.user_id,
        &lease.device_hw_id,
        lease.tier,
        lease.issued_at,
        lease.expires_at,
    );

    verifying_key
        .verify(&canonical_payload, &signature)
        .map_err(|e| Error::Lease(format!("Invalid lease signature: {e}")))?;

    Ok(())
}
