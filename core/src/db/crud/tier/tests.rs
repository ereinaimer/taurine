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
