use super::*;
use crate::db::crud::triggers::{InvocationType, NewEntry, create_entry};
use crate::db::crud::workspaces::create_workspace;
use crate::db::init::ensure_schema;
use rusqlite::Connection;

fn make_entry(trigger: &str) -> NewEntry {
    NewEntry {
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
    ensure_schema(&conn).unwrap();

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
fn test_free_tier_allows_unlimited_snippets() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();

    for i in 1..=35 {
        let entry = make_entry(&format!(":snip{i}"));
        assert!(
            create_entry(&conn, entry).is_ok(),
            "Snippet {i} creation should succeed under Free tier"
        );
    }

    let enabled_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0 AND is_enabled = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(enabled_count, 35);
}

#[test]
fn test_free_tier_allows_multiple_workspaces() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();

    assert!(create_workspace(&conn, "Secondary").is_ok());
    assert!(create_workspace(&conn, "Tertiary").is_ok());
}

#[test]
fn test_set_user_tier_free_does_not_pause_snippets() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();

    set_user_tier(&conn, UserTier::Pro).unwrap();

    for i in 1..=35 {
        let entry = make_entry(&format!(":snip{i}"));
        create_entry(&conn, entry).unwrap();
    }

    set_user_tier(&conn, UserTier::Free).unwrap();

    let enabled_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0 AND is_enabled = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(enabled_count, 35, "Free tier must not pause any snippets");
}

#[test]
fn test_free_tier_can_enable_any_snippet() {
    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();

    set_user_tier(&conn, UserTier::Pro).unwrap();
    for i in 1..=30 {
        let entry = make_entry(&format!(":snip{i}"));
        create_entry(&conn, entry).unwrap();
    }
    let entry_31 = make_entry(":snip31");
    let (id_31, _) = create_entry(&conn, entry_31).unwrap();
    conn.execute("UPDATE triggers SET is_enabled = 0 WHERE id = ?1", [&id_31])
        .unwrap();

    set_user_tier(&conn, UserTier::Free).unwrap();
    assert!(set_trigger_enabled(&conn, &id_31, true).is_ok());

    let is_enabled: bool = conn
        .query_row(
            "SELECT is_enabled FROM triggers WHERE id = ?1",
            [&id_31],
            |r| r.get(0),
        )
        .unwrap();
    assert!(is_enabled);
}
