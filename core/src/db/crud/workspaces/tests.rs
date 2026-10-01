use crate::db::crud::workspaces::{
    create_workspace, get_default_workspace, get_workspace_by_id, get_workspaces,
};
use crate::db::init::migrate::run_migrations;
use rusqlite::Connection;

#[test]
fn test_workspaces_schema_and_default_seed() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    let default_ws = get_default_workspace(&conn).unwrap();
    assert_eq!(default_ws.id, "default");
    assert_eq!(default_ws.name, "Personal");
    assert!(default_ws.is_default);
    assert_eq!(default_ws.version, 1);
    assert!(!default_ws.is_deleted);
    assert!(default_ws.is_synced);
    assert!(default_ws.created_at > 0);
    assert!(default_ws.updated_at > 0);
}

#[test]
fn test_create_and_get_workspaces() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let created = create_workspace(&conn, "Work").unwrap();
    assert_eq!(created.name, "Work");
    assert!(!created.id.is_empty());
    assert!(!created.is_default);
    assert_eq!(created.version, 1);
    assert!(!created.is_deleted);
    assert!(!created.is_synced);

    let all = get_workspaces(&conn).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, "default");
    assert_eq!(all[0].name, "Personal");
    assert!(all[0].is_default);
    assert_eq!(all[1].name, "Work");
    assert!(!all[1].is_default);
}

#[test]
fn test_create_workspace_rejects_empty_name() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let err = create_workspace(&conn, "   ").unwrap_err();
    match err {
        crate::Error::Config(msg) => {
            assert!(msg.to_lowercase().contains("empty"));
        }
        other => panic!("expected Config error, got {other:?}"),
    }
}

#[test]
fn test_quota_ledger_table_exists() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='quota_ledger')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(exists, "quota_ledger table should exist after migration");

    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(quota_ledger)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();

    assert!(cols.contains(&"week_start_epoch".to_string()));
    assert!(cols.contains(&"remaining_percentage".to_string()));
    assert!(cols.contains(&"last_expansion_at".to_string()));
    assert!(cols.contains(&"hmac_signature".to_string()));
    assert!(cols.contains(&"is_synced".to_string()));
    assert!(cols.contains(&"updated_at".to_string()));
}

#[test]
fn test_triggers_schema_has_workspace_id_and_index() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(triggers)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();

    assert!(
        cols.contains(&"workspace_id".to_string()),
        "triggers must have workspace_id column"
    );

    let index_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_triggers_workspace')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(index_exists, "idx_triggers_workspace index should exist");
}

#[test]
fn test_get_workspace_by_id() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let found = get_workspace_by_id(&conn, "default").unwrap().unwrap();
    assert_eq!(found.id, "default");
    assert_eq!(found.name, "Personal");

    let not_found = get_workspace_by_id(&conn, "nonexistent").unwrap();
    assert!(not_found.is_none());
}
