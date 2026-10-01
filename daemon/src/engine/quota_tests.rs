use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;
use taurine_core::db::crud::quota::get_or_init_quota_ledger;
use taurine_core::db::init::migrate::run_migrations;
use taurine_core::engine::ExpansionResult;
use taurine_core::engine::variables::{ArgMap, ExpansionStep};
use taurine_core::settings::SpinnerStyle;
use taurine_core::stats::TriggerStatKind;

use crate::engine::quota_guard::{ExpansionType, QuotaGuard};
use crate::hook::dispatch::dispatch_expansion_with;
use crate::voice::trigger_dispatch::fire_voice_trigger_with_args;

fn create_test_db() -> Connection {
    let conn = Connection::open_in_memory().expect("failed to open in-memory db");
    run_migrations(&conn).expect("failed to run migrations");
    conn
}

#[test]
fn test_expansion_drops_when_quota_exhausted() {
    let guard = QuotaGuard::new_with_balance(0.0);
    assert!(!guard.can_expand());
    assert!(guard.is_depleted());
    assert!(!guard.deplete(ExpansionType::Text));
    assert!(!guard.deplete(ExpansionType::Dynamic));
    assert!(!guard.deplete(ExpansionType::VoiceTrigger));
}

#[test]
fn test_quota_burn_text_expansion() {
    let guard = QuotaGuard::new_with_balance(100.0);
    assert!(guard.deplete(ExpansionType::Text));
    assert!((guard.balance() - 99.70).abs() < 1e-6);
    assert!((guard.unflushed_delta() - 0.30).abs() < 1e-6);
}

#[test]
fn test_quota_burn_dynamic_expansion() {
    let guard = QuotaGuard::new_with_balance(100.0);
    assert!(guard.deplete(ExpansionType::Dynamic));
    assert!((guard.balance() - 99.40).abs() < 1e-6);
    assert!((guard.unflushed_delta() - 0.60).abs() < 1e-6);
}

#[test]
fn test_quota_burn_voice_trigger() {
    let guard = QuotaGuard::new_with_balance(100.0);
    assert!(guard.deplete(ExpansionType::VoiceTrigger));
    assert!((guard.balance() - 99.10).abs() < 1e-6);
    assert!((guard.unflushed_delta() - 0.90).abs() < 1e-6);
}

#[test]
fn test_quota_voice_dictation_free() {
    let guard = QuotaGuard::new_with_balance(100.0);
    assert!(guard.deplete(ExpansionType::VoiceDictation));
    assert_eq!(guard.balance(), 100.0);
    assert_eq!(guard.unflushed_delta(), 0.0);

    guard.set_balance(0.0);
    assert!(guard.is_depleted());
    // Dictation is free and unlimited even when exhausted
    assert!(guard.deplete(ExpansionType::VoiceDictation));
    assert_eq!(guard.balance(), 0.0);
    assert_eq!(guard.unflushed_delta(), 0.0);
}

#[test]
fn test_warning_threshold_at_10_percent() {
    let guard = QuotaGuard::new_with_balance(100.0);
    assert!(!guard.is_warning());

    guard.set_balance(10.01);
    assert!(!guard.is_warning());

    guard.set_balance(10.00);
    assert!(guard.is_warning());

    guard.set_balance(5.00);
    assert!(guard.is_warning());

    guard.set_balance(0.00);
    assert!(guard.is_warning());
}

#[test]
fn test_deplete_fails_when_exhausted() {
    let guard = QuotaGuard::new_with_balance(0.20);
    assert!(guard.can_expand());
    assert!(!guard.is_depleted());

    // Burns remaining 0.20, balance becomes 0.0
    assert!(guard.deplete(ExpansionType::Text));
    assert_eq!(guard.balance(), 0.0);
    assert!(guard.is_depleted());
    assert!(!guard.can_expand());

    // Subsequent deplete attempts must fail
    assert!(!guard.deplete(ExpansionType::Text));
    assert!(!guard.deplete(ExpansionType::Dynamic));
    assert!(!guard.deplete(ExpansionType::VoiceTrigger));
    assert_eq!(guard.balance(), 0.0);
}

#[test]
fn test_flush_to_db() {
    let conn = create_test_db();
    let key = [42u8; 32];
    let guard = QuotaGuard::new_with_balance(100.0);

    assert!(guard.deplete(ExpansionType::Text)); // -0.30
    assert!(guard.deplete(ExpansionType::Dynamic)); // -0.60
    assert!((guard.unflushed_delta() - 0.90).abs() < 1e-6);

    guard
        .flush_to_db(&conn, &key)
        .expect("flush_to_db should succeed");
    assert_eq!(guard.unflushed_delta(), 0.0);

    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let row = get_or_init_quota_ledger(&conn, &key, now_epoch).expect("should fetch quota row");
    assert!((row.remaining_percentage - 99.10).abs() < 1e-6);

    // Flushing with 0 delta is a no-op
    guard
        .flush_to_db(&conn, &key)
        .expect("noop flush should succeed");
    assert_eq!(guard.unflushed_delta(), 0.0);
}

#[test]
fn test_dispatch_expansion_drops_when_quota_exhausted() {
    let _lock = taurine_core::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let original_balance = QuotaGuard::global().balance();
    QuotaGuard::global().set_balance(0.0);

    let expansion = ExpansionResult {
        delete_count: 2,
        steps: vec![ExpansionStep::Text("hello".to_string())],
        trigger: "hi".to_string(),
        undo_trigger: None,
        is_calculation: false,
        stat_kind: TriggerStatKind::Snippet,
        track_usage: false,
        follow_up: None,
    };

    let injected = Arc::new(AtomicBool::new(false));
    let injected_clone = Arc::clone(&injected);
    let state = Arc::new(taurine_core::engine::EngineState::new());

    dispatch_expansion_with(
        expansion,
        SpinnerStyle::default(),
        state.clone(),
        move |_steps, _delete_count, _spinner| {
            injected_clone.store(true, Ordering::SeqCst);
            crate::injector::InjectionReport {
                completed: true,
                successful_chars: 5,
            }
        },
        |_follow_up, _spinner| {},
    );

    assert!(
        !injected.load(Ordering::SeqCst),
        "Expansion must be dropped when quota is exhausted"
    );

    // Reset quota to 100.0% and try again
    QuotaGuard::global().set_balance(100.0);

    let expansion2 = ExpansionResult {
        delete_count: 2,
        steps: vec![ExpansionStep::Text("hello".to_string())],
        trigger: "hi".to_string(),
        undo_trigger: None,
        is_calculation: false,
        stat_kind: TriggerStatKind::Snippet,
        track_usage: false,
        follow_up: None,
    };

    let injected2 = Arc::new(AtomicBool::new(false));
    let injected2_clone = Arc::clone(&injected2);

    dispatch_expansion_with(
        expansion2,
        SpinnerStyle::default(),
        state,
        move |_steps, _delete_count, _spinner| {
            injected2_clone.store(true, Ordering::SeqCst);
            crate::injector::InjectionReport {
                completed: true,
                successful_chars: 5,
            }
        },
        |_follow_up, _spinner| {},
    );

    assert!(
        injected2.load(Ordering::SeqCst),
        "Expansion must succeed when quota is available"
    );

    // Restore original balance
    QuotaGuard::global().set_balance(original_balance);
}

#[test]
fn test_voice_trigger_drops_when_quota_exhausted() {
    let _lock = taurine_core::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let original_balance = QuotaGuard::global().balance();
    QuotaGuard::global().set_balance(0.0);

    let conn = create_test_db();
    let inv = taurine_core::db::crud::ResolvedInvocation {
        trigger_id: "test-id".to_string(),
        invocation: "hello".to_string(),
        invocation_type: taurine_core::db::crud::InvocationType::Voice,
        action: taurine_core::db::crud::TriggerAction {
            output: "world".to_string(),
            action_type: "text".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            parent_id: "test-id".to_string(),
            interpreter: None,
            behavior: None,
            script_binary: None,
        },
        require_confirmation: false,
        strict_threshold: 0.5,
    };

    let result = fire_voice_trigger_with_args(
        inv,
        &ArgMap::default(),
        "hello",
        &conn,
        None,
        SpinnerStyle::default(),
    );

    assert!(
        result.is_none(),
        "Voice trigger must return None when quota is exhausted"
    );

    QuotaGuard::global().set_balance(original_balance);
}

#[test]
fn test_quota_guard_set_global_and_init_global() {
    let _lock = taurine_core::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let original = QuotaGuard::global().balance();

    QuotaGuard::init_global(85.0);
    assert!((QuotaGuard::global().balance() - 85.0).abs() < 1e-6);

    let custom = QuotaGuard::new_with_balance(42.5);
    QuotaGuard::set_global(custom);
    assert!((QuotaGuard::global().balance() - 42.5).abs() < 1e-6);

    QuotaGuard::global().set_balance(original);
}
