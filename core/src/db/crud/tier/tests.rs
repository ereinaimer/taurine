use super::*;
use crate::db::crud::triggers::{InvocationType, NewEntry, create_entry, delete_trigger};
use crate::db::crud::workspaces::create_workspace;
use crate::db::init::migrate::run_migrations;
use rusqlite::Connection;

fn make_entry(trigger: &str) -> NewEntry {
    NewEntry {
        name: format!("Snippet {trigger}"),
        description: None,
        content: format!("Output for {trigger}"),
        action_type: "text".to_string(),
        target_os: "all".to_string(),
        only_apps: None,
        except_apps: None,
        tags_json: "[]".to_string(),
        auto_case: false,
        interpreter: None,
        behavior: None,
        invocations: vec![(InvocationType::Word, trigger.to_string(), false)],
    }
}

#[test]
fn test_user_tier_serde_and_default() {
    assert_eq!(UserTier::default(), UserTier::Free);
    assert!(UserTier::Free.is_free());
    assert!(!UserTier::Free.is_unlimited());
    assert!(!UserTier::Pro.is_free());
    assert!(UserTier::Pro.is_unlimited());

    let json = serde_json::to_string(&UserTier::Free).unwrap();
    assert_eq!(json, "\"free\"");
    let parsed: UserTier = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, UserTier::Free);

    let parsed_pro: UserTier = serde_json::from_str("\"pro\"").unwrap();
    assert_eq!(parsed_pro, UserTier::Pro);

    let parsed_max: UserTier = serde_json::from_str("\"max\"").unwrap();
    assert_eq!(parsed_max, UserTier::Max);

    let parsed_team: UserTier = serde_json::from_str("\"team\"").unwrap();
    assert_eq!(parsed_team, UserTier::Team);
}

#[test]
fn test_get_and_set_user_tier() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    assert_eq!(get_user_tier(&conn), UserTier::Free);

    set_user_tier(&conn, UserTier::Pro).unwrap();
    assert_eq!(get_user_tier(&conn), UserTier::Pro);

    set_user_tier(&conn, UserTier::Max).unwrap();
    assert_eq!(get_user_tier(&conn), UserTier::Max);

    set_user_tier(&conn, UserTier::Team).unwrap();
    assert_eq!(get_user_tier(&conn), UserTier::Team);

    set_user_tier(&conn, UserTier::Free).unwrap();
    assert_eq!(get_user_tier(&conn), UserTier::Free);
}

#[test]
fn test_free_tier_allows_up_to_30_snippets_rejects_31st() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    for i in 1..=30 {
        let entry = make_entry(&format!(":snip{i}"));
        assert!(
            create_entry(&conn, entry).is_ok(),
            "Snippet {i} creation should succeed under Free tier"
        );
    }

    let entry_31 = make_entry(":snip31");
    let result = create_entry(&conn, entry_31);
    match result {
        Err(crate::Error::QuotaExceeded(msg)) => {
            assert!(
                msg.contains("30 snippets"),
                "Expected message about 30 snippets, got: {msg}"
            );
        }
        other => panic!("Expected QuotaExceeded error, got: {other:?}"),
    }
}

#[test]
fn test_pro_tier_allows_31st_snippet() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    set_user_tier(&conn, UserTier::Pro).unwrap();

    for i in 1..=35 {
        let entry = make_entry(&format!(":prosnip{i}"));
        assert!(
            create_entry(&conn, entry).is_ok(),
            "Snippet {i} creation should succeed under Pro tier"
        );
    }
}

#[test]
fn test_soft_deleted_snippets_do_not_count_toward_limit() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let mut first_id = String::new();
    for i in 1..=30 {
        let entry = make_entry(&format!(":softdel{i}"));
        let (id, _) = create_entry(&conn, entry).unwrap();
        if i == 1 {
            first_id = id;
        }
    }

    // 31st snippet rejected
    let entry_31 = make_entry(":softdel31");
    assert!(matches!(
        create_entry(&conn, entry_31),
        Err(crate::Error::QuotaExceeded(_))
    ));

    // Soft delete the first snippet
    let deleted = delete_trigger(&conn, &first_id).unwrap();
    assert!(deleted);

    // Now active count is 29, so 31st snippet succeeds
    let entry_31_retry = make_entry(":softdel31");
    assert!(create_entry(&conn, entry_31_retry).is_ok());

    // 32nd snippet should be rejected again
    let entry_32 = make_entry(":softdel32");
    assert!(matches!(
        create_entry(&conn, entry_32),
        Err(crate::Error::QuotaExceeded(_))
    ));
}

#[test]
fn test_free_tier_rejects_second_workspace() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    // Default workspace is already seeded during migrations
    let result = create_workspace(&conn, "Secondary");
    match result {
        Err(crate::Error::QuotaExceeded(msg)) => {
            assert!(
                msg.contains("1 workspace"),
                "Expected message about 1 workspace, got: {msg}"
            );
        }
        other => panic!("Expected QuotaExceeded error, got: {other:?}"),
    }
}

#[test]
fn test_pro_tier_allows_second_workspace() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    set_user_tier(&conn, UserTier::Pro).unwrap();

    let ws = create_workspace(&conn, "Secondary");
    assert!(
        ws.is_ok(),
        "Pro tier should allow creating a second workspace"
    );
}

#[test]
fn test_auto_pause_excess_snippets_keeps_top_30_most_used() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    set_user_tier(&conn, UserTier::Pro).unwrap();

    let mut ids = Vec::new();
    for i in 1..=35 {
        let entry = make_entry(&format!(":autopause{i}"));
        let (id, _) = create_entry(&conn, entry).unwrap();
        // Give snippet i a usage_count of i * 10
        conn.execute(
            "UPDATE triggers SET usage_count = ?1 WHERE id = ?2",
            rusqlite::params![i * 10, id],
        )
        .unwrap();
        ids.push((id, i * 10));
    }

    // Now switch tier to Free, which triggers enforce_free_tier_snippet_cap
    set_user_tier(&conn, UserTier::Free).unwrap();

    // Verify 30 are enabled and 5 are disabled
    let enabled_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0 AND is_enabled = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(enabled_count, 30);

    let disabled_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0 AND is_enabled = 0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(disabled_count, 5);

    // The 5 disabled should be snippets 1 to 5 (the lowest usage counts)
    for (id, usage) in &ids[0..5] {
        let is_enabled: bool = conn
            .query_row("SELECT is_enabled FROM triggers WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            !is_enabled,
            "Snippet with usage {usage} should have been paused"
        );
    }

    // The top 30 (snippets 6 to 35) should remain enabled
    for (id, usage) in &ids[5..35] {
        let is_enabled: bool = conn
            .query_row("SELECT is_enabled FROM triggers WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(
            is_enabled,
            "Snippet with usage {usage} should remain enabled"
        );
    }
}

#[test]
fn test_auto_pause_one_way_valve_preserves_custom_disabled_snippets() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let mut custom_disabled_id = String::new();
    for i in 1..=25 {
        let entry = make_entry(&format!(":valve{i}"));
        let (id, _) = create_entry(&conn, entry).unwrap();
        if i == 5 {
            // Manually disable snippet 5
            conn.execute("UPDATE triggers SET is_enabled = 0 WHERE id = ?1", [&id])
                .unwrap();
            custom_disabled_id = id;
        }
    }

    // 24 enabled, 1 disabled. Total <= 30.
    let paused = enforce_free_tier_snippet_cap(&conn).unwrap();
    assert_eq!(paused, 0);

    // Verify snippet 5 is still disabled (it was NOT auto-enabled!)
    let is_enabled: bool = conn
        .query_row(
            "SELECT is_enabled FROM triggers WHERE id = ?1",
            [&custom_disabled_id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !is_enabled,
        "Auto-cap must never re-enable disabled snippets"
    );
}

#[test]
fn test_enable_guard_blocks_when_30_snippets_active() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    for i in 1..=30 {
        let entry = make_entry(&format!(":guard{i}"));
        create_entry(&conn, entry).unwrap();
    }

    // Temporarily switch to Pro to insert a 31st snippet as disabled
    set_user_tier(&conn, UserTier::Pro).unwrap();
    let entry_31 = make_entry(":guard31");
    let (id_31, _) = create_entry(&conn, entry_31).unwrap();
    conn.execute("UPDATE triggers SET is_enabled = 0 WHERE id = ?1", [&id_31])
        .unwrap();
    let disabled_id = id_31;
    set_user_tier(&conn, UserTier::Free).unwrap();

    // Attempting to enable the 31st snippet should fail because 30 are already enabled
    let result = set_trigger_enabled(&conn, &disabled_id, true);
    match result {
        Err(crate::Error::QuotaExceeded(msg)) => {
            assert!(
                msg.contains("30 active snippets"),
                "Expected 30 active snippets message, got: {msg}"
            );
        }
        other => panic!("Expected QuotaExceeded error, got: {other:?}"),
    }
}

#[test]
fn test_disable_allows_enabling_another_snippet() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let mut first_active_id = String::new();
    for i in 1..=30 {
        let entry = make_entry(&format!(":swap{i}"));
        let (id, _) = create_entry(&conn, entry).unwrap();
        if i == 1 {
            first_active_id = id;
        }
    }

    // Add a disabled 31st snippet under Pro then revert to Free
    set_user_tier(&conn, UserTier::Pro).unwrap();
    let entry_31 = make_entry(":swap31");
    let (id_31, _) = create_entry(&conn, entry_31).unwrap();
    conn.execute("UPDATE triggers SET is_enabled = 0 WHERE id = ?1", [&id_31])
        .unwrap();
    set_user_tier(&conn, UserTier::Free).unwrap();

    // Disable snippet 1
    set_trigger_enabled(&conn, &first_active_id, false).unwrap();

    // Now enabling snippet 31 must succeed because only 29 are active
    set_trigger_enabled(&conn, &id_31, true).unwrap();

    let is_31_enabled: bool = conn
        .query_row(
            "SELECT is_enabled FROM triggers WHERE id = ?1",
            [&id_31],
            |r| r.get(0),
        )
        .unwrap();
    assert!(is_31_enabled);
}
