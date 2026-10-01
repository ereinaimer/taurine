use super::hmac::{compute_quota_hmac, verify_quota_hmac};
use super::{
    calculate_week_start_epoch, get_or_init_quota_ledger, record_quota_depletion,
    set_quota_balance_from_cloud,
};
use crate::db::init::migrate::run_migrations;
use chrono::NaiveDate;
use rusqlite::Connection;

#[test]
fn test_calculate_week_start_epoch_boundaries() {
    // 1970-01-01 00:00:00 UTC was Thursday.
    // Monday of that week was 1969-12-29 00:00:00 UTC (-259200 seconds).
    assert_eq!(calculate_week_start_epoch(0), -259_200);

    // 1970-01-05 00:00:00 UTC was Monday (345600 seconds).
    assert_eq!(calculate_week_start_epoch(345_600), 345_600);

    // Monday 2026-09-28 00:00:00 UTC:
    let mon_date = NaiveDate::from_ymd_opt(2026, 9, 28)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let mon_epoch = mon_date.and_utc().timestamp();
    assert_eq!(calculate_week_start_epoch(mon_epoch), mon_epoch);

    // Wednesday 2026-09-30 15:30:45 UTC (2 days, 15h, 30m, 45s later):
    let wed_epoch = mon_epoch + (2 * 86_400) + (15 * 3600) + (30 * 60) + 45;
    assert_eq!(calculate_week_start_epoch(wed_epoch), mon_epoch);

    // Sunday 2026-10-04 23:59:59 UTC:
    let sun_epoch = mon_epoch + (7 * 86_400) - 1;
    assert_eq!(calculate_week_start_epoch(sun_epoch), mon_epoch);

    // Next Monday 2026-10-05 00:00:00 UTC:
    let next_mon_epoch = mon_epoch + (7 * 86_400);
    assert_eq!(calculate_week_start_epoch(next_mon_epoch), next_mon_epoch);
}

#[test]
fn test_quota_hmac_computation_and_verification() {
    let key = [42u8; 32];
    let week_epoch = 1_774_915_200;
    let remaining_pct = 100.0;

    let sig = compute_quota_hmac(&key, week_epoch, remaining_pct);
    assert_eq!(sig.len(), 64, "HMAC-SHA256 signature must be 64 hex chars");

    // Exact match must verify
    assert!(verify_quota_hmac(&key, week_epoch, remaining_pct, &sig));

    // Tampered key must fail
    let mut bad_key = key;
    bad_key[0] ^= 1;
    assert!(!verify_quota_hmac(
        &bad_key,
        week_epoch,
        remaining_pct,
        &sig
    ));

    // Tampered week epoch must fail
    assert!(!verify_quota_hmac(
        &key,
        week_epoch + 1,
        remaining_pct,
        &sig
    ));

    // Tampered remaining percentage must fail
    assert!(!verify_quota_hmac(&key, week_epoch, 99.7, &sig));

    // Tampered signature must fail
    assert!(!verify_quota_hmac(
        &key,
        week_epoch,
        remaining_pct,
        "deadbeef"
    ));
    let mut corrupt_sig = sig.clone();
    corrupt_sig.replace_range(0..1, if &sig[0..1] == "a" { "b" } else { "a" });
    assert!(!verify_quota_hmac(
        &key,
        week_epoch,
        remaining_pct,
        &corrupt_sig
    ));
}

#[test]
fn test_get_or_init_quota_ledger_seeds_initial_100() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let key = [7u8; 32];
    let now_epoch = 1_774_950_000;
    let expected_week_start = calculate_week_start_epoch(now_epoch);

    let row = get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap();
    assert_eq!(row.week_start_epoch, expected_week_start);
    assert_eq!(row.remaining_percentage, 100.0);
    assert_eq!(row.last_expansion_at, None);
    assert!(!row.is_synced);
    assert_eq!(row.updated_at, now_epoch);
    assert!(verify_quota_hmac(
        &key,
        row.week_start_epoch,
        row.remaining_percentage,
        &row.hmac_signature
    ));

    // Re-fetch within the same week does not overwrite or reseed
    let row2 = get_or_init_quota_ledger(&conn, &key, now_epoch + 3600).unwrap();
    assert_eq!(row, row2);
}

#[test]
fn test_record_quota_depletion() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let key = [9u8; 32];
    let now_epoch = 1_774_950_000;

    // First depletion initializes and subtracts 0.30%
    let dep1 = record_quota_depletion(&conn, &key, 0.30, now_epoch + 10).unwrap();
    assert!((dep1.remaining_percentage - 99.70).abs() < 1e-6);
    assert_eq!(dep1.last_expansion_at, Some(now_epoch + 10));
    assert_eq!(dep1.updated_at, now_epoch + 10);
    assert!(!dep1.is_synced);
    assert!(verify_quota_hmac(
        &key,
        dep1.week_start_epoch,
        dep1.remaining_percentage,
        &dep1.hmac_signature
    ));

    // Second depletion subtracts 0.60%
    let dep2 = record_quota_depletion(&conn, &key, 0.60, now_epoch + 20).unwrap();
    assert!((dep2.remaining_percentage - 99.10).abs() < 1e-6);
    assert_eq!(dep2.last_expansion_at, Some(now_epoch + 20));
    assert_eq!(dep2.updated_at, now_epoch + 20);
    assert!(verify_quota_hmac(
        &key,
        dep2.week_start_epoch,
        dep2.remaining_percentage,
        &dep2.hmac_signature
    ));

    // Third depletion subtracts 0.90%
    let dep3 = record_quota_depletion(&conn, &key, 0.90, now_epoch + 30).unwrap();
    assert!((dep3.remaining_percentage - 98.20).abs() < 1e-6);
    assert!(verify_quota_hmac(
        &key,
        dep3.week_start_epoch,
        dep3.remaining_percentage,
        &dep3.hmac_signature
    ));

    // Massive depletion clamps to 0.0%
    let dep4 = record_quota_depletion(&conn, &key, 150.0, now_epoch + 40).unwrap();
    assert_eq!(dep4.remaining_percentage, 0.0);
    assert!(verify_quota_hmac(
        &key,
        dep4.week_start_epoch,
        dep4.remaining_percentage,
        &dep4.hmac_signature
    ));
}

#[test]
fn test_quota_ledger_fail_closed_on_tampered_balance() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let key = [13u8; 32];
    let now_epoch = 1_774_950_000;

    let _ = get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap();

    // Directly tamper with the SQLite database row to elevate remaining percentage
    conn.execute("UPDATE quota_ledger SET remaining_percentage = 999.0", [])
        .unwrap();

    // Query must fail closed with an error
    let err = get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap_err();
    match err {
        crate::Error::Engine(msg) => {
            assert!(msg.to_lowercase().contains("hmac") || msg.to_lowercase().contains("tamper"));
        }
        other => panic!("expected Engine error on tampered balance, got {other:?}"),
    }

    // Attempting to record depletion on tampered ledger must also fail closed
    let dep_err = record_quota_depletion(&conn, &key, 0.30, now_epoch + 10).unwrap_err();
    match dep_err {
        crate::Error::Engine(msg) => {
            assert!(msg.to_lowercase().contains("hmac") || msg.to_lowercase().contains("tamper"));
        }
        other => panic!("expected Engine error on depletion with tampered ledger, got {other:?}"),
    }
}

#[test]
fn test_quota_ledger_fail_closed_on_tampered_signature() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let key = [17u8; 32];
    let now_epoch = 1_774_950_000;

    let _ = get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap();

    // Corrupt signature in SQLite
    conn.execute(
        "UPDATE quota_ledger SET hmac_signature = '0123456789abcdef'",
        [],
    )
    .unwrap();

    let err = get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap_err();
    match err {
        crate::Error::Engine(msg) => {
            assert!(msg.to_lowercase().contains("hmac") || msg.to_lowercase().contains("tamper"));
        }
        other => panic!("expected Engine error on tampered signature, got {other:?}"),
    }
}

#[test]
fn test_weekly_rollover_resets_balance() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let key = [23u8; 32];
    let week1_epoch = 1_774_950_000; // Week 1

    // Seed and deplete week 1
    let row1 = record_quota_depletion(&conn, &key, 40.0, week1_epoch).unwrap();
    assert_eq!(row1.remaining_percentage, 60.0);

    // Jump 7 days forward into Week 2
    let week2_epoch = week1_epoch + (7 * 86_400);
    let row2 = get_or_init_quota_ledger(&conn, &key, week2_epoch).unwrap();

    assert_eq!(row2.remaining_percentage, 100.0);
    assert_ne!(row1.week_start_epoch, row2.week_start_epoch);
    assert_eq!(
        row2.week_start_epoch,
        calculate_week_start_epoch(week2_epoch)
    );
    assert!(verify_quota_hmac(
        &key,
        row2.week_start_epoch,
        row2.remaining_percentage,
        &row2.hmac_signature
    ));
}

#[test]
fn test_set_quota_balance_from_cloud() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let key = [31u8; 32];
    let now_epoch = 1_774_950_000;
    let week_epoch = calculate_week_start_epoch(now_epoch);

    let synced = set_quota_balance_from_cloud(&conn, &key, week_epoch, 74.5, now_epoch).unwrap();
    assert_eq!(synced.week_start_epoch, week_epoch);
    assert_eq!(synced.remaining_percentage, 74.5);
    assert!(synced.is_synced);
    assert_eq!(synced.updated_at, now_epoch);
    assert!(verify_quota_hmac(
        &key,
        synced.week_start_epoch,
        synced.remaining_percentage,
        &synced.hmac_signature
    ));

    // Cloud update persists in DB
    let fetched = get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap();
    assert_eq!(fetched.remaining_percentage, 74.5);
    assert!(fetched.is_synced);
}

#[test]
fn test_rfc4231_hmac_sha256_vector() {
    use super::hmac::hmac_sha256;
    let key = b"Jefe";
    let data = b"what do ya want for nothing?";
    let digest = hmac_sha256(key, data);
    assert_eq!(
        hex::encode(digest),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}
