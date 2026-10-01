use crate::cloud::device::{get_device_hardware_id, get_device_name, get_device_platform};
use crate::cloud::lease::{
    CapabilityLease, TimeAnchor, canonical_lease_payload, sign_lease, validate_lease,
    validate_lease_with_hw_id,
};
use crate::db::crud::tier::UserTier;
use crate::system::error::Error;
use ed25519_dalek::SigningKey;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn test_signing_key() -> SigningKey {
    // Deterministic 32-byte seed for hermetic testing
    let seed: [u8; 32] = [42u8; 32];
    SigningKey::from_bytes(&seed)
}

#[test]
fn test_clock_rollback_rejected() {
    let anchor = TimeAnchor {
        instant: Instant::now(),
        system_time: SystemTime::now(),
    };

    // Case 1: System clock goes backward before anchor time
    let now_instant = anchor.instant + Duration::from_secs(10);
    let now_system = anchor.system_time - Duration::from_secs(30);

    let res = anchor.check_clock_tampering(now_instant, now_system);
    assert!(
        res.is_err(),
        "Expected clock rollback error when system time is before anchor"
    );
    match res.unwrap_err() {
        Error::ClockDrift(msg) => {
            assert!(
                msg.to_lowercase().contains("rollback") || msg.to_lowercase().contains("drift")
            );
        }
        other => panic!("Expected Error::ClockDrift, got: {:?}", other),
    }

    // Case 2: Monotonic clock advances by 100s, but system clock only advances by 10s (rolled back by 90s)
    let now_instant_adv = anchor.instant + Duration::from_secs(100);
    let now_system_skewed = anchor.system_time + Duration::from_secs(10);

    let res2 = anchor.check_clock_tampering(now_instant_adv, now_system_skewed);
    assert!(
        res2.is_err(),
        "Expected clock rollback error when monotonic clock outpaces system time significantly"
    );
    match res2.unwrap_err() {
        Error::ClockDrift(msg) => {
            assert!(
                msg.to_lowercase().contains("rollback") || msg.to_lowercase().contains("drift")
            );
        }
        other => panic!("Expected Error::ClockDrift, got: {:?}", other),
    }
}

#[test]
fn test_clock_forward_jump_rejected() {
    let anchor = TimeAnchor {
        instant: Instant::now(),
        system_time: SystemTime::now(),
    };

    // Monotonic clock elapsed: 10 seconds.
    // System clock jumped by 4000 seconds (> 3600 seconds limit).
    let now_instant = anchor.instant + Duration::from_secs(10);
    let now_system = anchor.system_time + Duration::from_secs(4000);

    let res = anchor.check_clock_tampering(now_instant, now_system);
    assert!(
        res.is_err(),
        "Expected clock jump error when system clock jumps > 3600s beyond monotonic drift"
    );
    match res.unwrap_err() {
        Error::ClockDrift(msg) => {
            assert!(msg.to_lowercase().contains("jump") || msg.to_lowercase().contains("drift"));
        }
        other => panic!("Expected Error::ClockDrift, got: {:?}", other),
    }
}

#[test]
fn test_clock_normal_advance_accepted() {
    let anchor = TimeAnchor {
        instant: Instant::now(),
        system_time: SystemTime::now(),
    };

    // Normal forward advance of 60 seconds
    let now_instant = anchor.instant + Duration::from_secs(60);
    let now_system = anchor.system_time + Duration::from_secs(60);

    let res = anchor.check_clock_tampering(now_instant, now_system);
    assert!(res.is_ok(), "Expected normal clock advance to succeed");
}

#[test]
fn test_canonical_lease_payload_deterministic() {
    let p1 = canonical_lease_payload("user-1", "hw-abc", UserTier::Pro, 1000, 2000);
    let p2 = canonical_lease_payload("user-1", "hw-abc", UserTier::Pro, 1000, 2000);
    assert_eq!(p1, p2);

    let p3 = canonical_lease_payload("user-2", "hw-abc", UserTier::Pro, 1000, 2000);
    assert_ne!(p1, p3);

    let p4 = canonical_lease_payload("user-1", "hw-abc", UserTier::Max, 1000, 2000);
    assert_ne!(p1, p4);
}

#[test]
fn test_valid_lease_verification() {
    let signing_key = test_signing_key();
    let verifying_key_bytes = signing_key.verifying_key().to_bytes();

    let anchor = TimeAnchor::new();
    let now_instant = anchor.instant;
    let now_system = anchor.system_time;
    let now_epoch = now_system
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_secs() as i64;

    let hw_id = get_device_hardware_id();
    let issued_at = now_epoch - 60; // 1 minute ago
    let expires_at = now_epoch + 3600; // 1 hour from now (TTL 3660s <= 72h)

    let lease: CapabilityLease = sign_lease(
        "usr_123",
        &hw_id,
        UserTier::Pro,
        issued_at,
        expires_at,
        &signing_key,
    );

    let res = validate_lease(
        &lease,
        &anchor,
        now_instant,
        now_system,
        &verifying_key_bytes,
    );
    assert!(
        res.is_ok(),
        "Expected valid lease to pass validation: {:?}",
        res
    );
}

#[test]
fn test_lease_expired_rejected() {
    let signing_key = test_signing_key();
    let verifying_key_bytes = signing_key.verifying_key().to_bytes();

    let anchor = TimeAnchor::new();
    let now_instant = anchor.instant;
    let now_system = anchor.system_time;
    let now_epoch = now_system
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_secs() as i64;

    let hw_id = get_device_hardware_id();
    let issued_at = now_epoch - 3600;
    let expires_at = now_epoch - 60; // Expired 1 minute ago

    let lease = sign_lease(
        "usr_123",
        &hw_id,
        UserTier::Pro,
        issued_at,
        expires_at,
        &signing_key,
    );

    let res = validate_lease(
        &lease,
        &anchor,
        now_instant,
        now_system,
        &verifying_key_bytes,
    );
    assert!(res.is_err(), "Expected expired lease to fail");
    match res.unwrap_err() {
        Error::Lease(msg) => {
            assert!(msg.to_lowercase().contains("expired"));
        }
        other => panic!("Expected Error::Lease, got {:?}", other),
    }
}

#[test]
fn test_lease_future_issued_rejected() {
    let signing_key = test_signing_key();
    let verifying_key_bytes = signing_key.verifying_key().to_bytes();

    let anchor = TimeAnchor::new();
    let now_instant = anchor.instant;
    let now_system = anchor.system_time;
    let now_epoch = now_system
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_secs() as i64;

    let hw_id = get_device_hardware_id();
    let issued_at = now_epoch + 300; // Issued 5 minutes in future
    let expires_at = now_epoch + 7200;

    let lease = sign_lease(
        "usr_123",
        &hw_id,
        UserTier::Pro,
        issued_at,
        expires_at,
        &signing_key,
    );

    let res = validate_lease(
        &lease,
        &anchor,
        now_instant,
        now_system,
        &verifying_key_bytes,
    );
    assert!(res.is_err(), "Expected future-issued lease to fail");
    match res.unwrap_err() {
        Error::Lease(msg) => {
            assert!(
                msg.to_lowercase().contains("future")
                    || msg.to_lowercase().contains("not yet valid")
            );
        }
        other => panic!("Expected Error::Lease, got {:?}", other),
    }
}

#[test]
fn test_lease_ttl_exceeds_72h_rejected() {
    let signing_key = test_signing_key();
    let verifying_key_bytes = signing_key.verifying_key().to_bytes();

    let anchor = TimeAnchor::new();
    let now_instant = anchor.instant;
    let now_system = anchor.system_time;
    let now_epoch = now_system
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_secs() as i64;

    let hw_id = get_device_hardware_id();
    let issued_at = now_epoch;
    let expires_at = now_epoch + (73 * 3600); // 73 hours (> 72h max)

    let lease = sign_lease(
        "usr_123",
        &hw_id,
        UserTier::Pro,
        issued_at,
        expires_at,
        &signing_key,
    );

    let res = validate_lease(
        &lease,
        &anchor,
        now_instant,
        now_system,
        &verifying_key_bytes,
    );
    assert!(res.is_err(), "Expected lease with TTL > 72h to fail");
    match res.unwrap_err() {
        Error::Lease(msg) => {
            assert!(msg.to_lowercase().contains("ttl") || msg.to_lowercase().contains("72"));
        }
        other => panic!("Expected Error::Lease, got {:?}", other),
    }
}

#[test]
fn test_lease_device_hw_id_mismatch_rejected() {
    let signing_key = test_signing_key();
    let verifying_key_bytes = signing_key.verifying_key().to_bytes();

    let anchor = TimeAnchor::new();
    let now_instant = anchor.instant;
    let now_system = anchor.system_time;
    let now_epoch = now_system
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_secs() as i64;

    // Create lease for different hardware ID
    let lease = sign_lease(
        "usr_123",
        "different-hw-guid-999",
        UserTier::Pro,
        now_epoch - 60,
        now_epoch + 3600,
        &signing_key,
    );

    // Using validate_lease_with_hw_id with expected "my-device-guid"
    let res = validate_lease_with_hw_id(
        &lease,
        &anchor,
        now_instant,
        now_system,
        &verifying_key_bytes,
        "my-device-guid",
    );
    assert!(res.is_err(), "Expected device hw_id mismatch to fail");
    match res.unwrap_err() {
        Error::Lease(msg) => {
            assert!(
                msg.to_lowercase().contains("hardware") || msg.to_lowercase().contains("mismatch")
            );
        }
        other => panic!("Expected Error::Lease, got {:?}", other),
    }
}

#[test]
fn test_lease_tampered_payload_rejected() {
    let signing_key = test_signing_key();
    let verifying_key_bytes = signing_key.verifying_key().to_bytes();

    let anchor = TimeAnchor::new();
    let now_instant = anchor.instant;
    let now_system = anchor.system_time;
    let now_epoch = now_system
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_secs() as i64;

    let hw_id = get_device_hardware_id();
    let mut lease = sign_lease(
        "usr_123",
        &hw_id,
        UserTier::Free,
        now_epoch - 60,
        now_epoch + 3600,
        &signing_key,
    );

    // Attacker modifies tier to Pro without re-signing
    lease.tier = UserTier::Pro;

    let res = validate_lease(
        &lease,
        &anchor,
        now_instant,
        now_system,
        &verifying_key_bytes,
    );
    assert!(
        res.is_err(),
        "Expected tampered lease payload to fail signature verification"
    );
    match res.unwrap_err() {
        Error::Lease(msg) => {
            assert!(msg.to_lowercase().contains("signature"));
        }
        other => panic!("Expected Error::Lease, got {:?}", other),
    }
}

#[test]
fn test_lease_invalid_signature_hex_rejected() {
    let signing_key = test_signing_key();
    let verifying_key_bytes = signing_key.verifying_key().to_bytes();

    let anchor = TimeAnchor::new();
    let now_instant = anchor.instant;
    let now_system = anchor.system_time;
    let now_epoch = now_system
        .duration_since(UNIX_EPOCH)
        .expect("time after epoch")
        .as_secs() as i64;

    let hw_id = get_device_hardware_id();
    let mut lease = sign_lease(
        "usr_123",
        &hw_id,
        UserTier::Pro,
        now_epoch - 60,
        now_epoch + 3600,
        &signing_key,
    );

    lease.signature = "invalid-hex-non-hex-characters!".to_string();

    let res = validate_lease(
        &lease,
        &anchor,
        now_instant,
        now_system,
        &verifying_key_bytes,
    );
    assert!(res.is_err(), "Expected malformed signature hex to fail");
}

#[test]
fn test_device_fingerprinting_outputs() {
    let hw_id = get_device_hardware_id();
    assert!(!hw_id.trim().is_empty(), "Hardware ID must not be empty");

    let name = get_device_name();
    assert!(!name.trim().is_empty(), "Device name must not be empty");

    let platform = get_device_platform();
    assert!(
        platform == "windows"
            || platform == "macos"
            || platform == "linux"
            || platform == "unknown",
        "Unexpected platform: {}",
        platform
    );
}
