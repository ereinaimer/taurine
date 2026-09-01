use super::*;
use crate::db::crud::{TriggerType, get_trigger, upsert_trigger, upsert_trigger_with_type};
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use crate::exchange::AssetExport;
use crate::testing::{init_tracing_for_tests, open_test_db};

fn insert_raw_word_trigger(
    conn: &rusqlite::Connection,
    id: &str,
    name: &str,
    trigger: &str,
    output: &str,
    target_os: &str,
    version: i64,
) {
    let now = crate::db::now_unix_secs();
    conn.execute(
        "INSERT INTO triggers (
                id, name, description, trigger_type, trigger, output, action_type,
                is_enabled, target_os, tags, usage_count, last_used_at,
                created_at, updated_at, version, is_deleted, is_synced
             ) VALUES (
                ?1, ?2, NULL, 'word', ?3, ?4, 'text',
                1, ?5, '[]', 0, NULL,
                ?6, ?6, ?7, 0, 1
             )",
        rusqlite::params![id, name, trigger, output, target_os, now, version],
    )
    .unwrap();
}

fn text_export(
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
    output: &str,
) -> TriggerExport {
    TriggerExport {
        name: format!("Imported {trigger}"),
        description: Some("Imported trigger".to_string()),
        trigger_type,
        trigger: trigger.to_string(),
        output: output.to_string(),
        action_type: "text".to_string(),
        is_enabled: true,
        target_os: target_os.to_string(),
        tags: vec!["imported".to_string()],
        script: None,
        assets: Vec::new(),
    }
}

#[test]
fn skip_conflict_preserves_existing_local_row() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    upsert_trigger(
        &conn,
        "local-id",
        "Local GM",
        Some("local"),
        "gm",
        "Local output",
        "text",
        "all",
        r#"["local"]"#,
        27,
        Some(1_700_000_000),
    )
    .unwrap();

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Word,
        "gm",
        "all",
        "Imported output",
    )]);
    let tx = conn.transaction().unwrap();
    let imported = import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Skip)).unwrap();
    tx.commit().unwrap();

    assert_eq!(imported, 0);

    let row = get_trigger(&conn, "local-id").unwrap().unwrap();
    assert_eq!(row.output, "Local output");
    assert_eq!(row.usage_count, 27);
    assert!(!row.is_deleted);
}

#[test]
fn overwrite_conflict_replaces_existing_row_with_fresh_import() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    upsert_trigger(
        &conn,
        "local-id",
        "Local GM",
        Some("local"),
        "gm",
        "Local output",
        "text",
        "all",
        r#"["local"]"#,
        27,
        Some(1_700_000_000),
    )
    .unwrap();

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Word,
        "gm",
        "all",
        "Imported output",
    )]);
    let tx = conn.transaction().unwrap();
    let imported =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx.commit().unwrap();

    assert_eq!(imported, 1);

    let local_row = get_trigger(&conn, "local-id").unwrap().unwrap();
    assert!(local_row.is_deleted);

    let (new_id, usage_count, last_used_at, is_deleted, output): (
        String,
        i64,
        Option<i64>,
        bool,
        String,
    ) = conn
        .query_row(
            "SELECT id, usage_count, last_used_at, is_deleted, output
                 FROM triggers
                 WHERE trigger = ?1 AND target_os = ?2 AND is_deleted = 0",
            ["gm", "all"],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();

    assert_ne!(new_id, "local-id");
    assert_eq!(usage_count, 0);
    assert_eq!(last_used_at, None);
    assert!(!is_deleted);
    assert_eq!(output, "Imported output");
}

#[test]
fn import_restores_hotkey_trigger_type() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Hotkey,
        "ctrl+shift+g",
        "win",
        "git status",
    )]);
    let tx = conn.transaction().unwrap();
    let imported =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx.commit().unwrap();

    assert_eq!(imported, 1);

    let row = conn
        .query_row(
            "SELECT trigger_type, trigger
                 FROM triggers
                 WHERE is_deleted = 0",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .unwrap();
    assert_eq!(row.0, "hotkey");
    assert_eq!(row.1, "ctrl+shift+g");
}

#[test]
fn import_canonicalizes_hotkey_trigger_order() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Hotkey,
        "alt+shift+2",
        "all",
        "echo works",
    )]);
    let tx = conn.transaction().unwrap();
    let imported =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx.commit().unwrap();

    assert_eq!(imported, 1);

    let stored_trigger: String = conn
        .query_row(
            "SELECT trigger FROM triggers WHERE is_deleted = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored_trigger, "shift+alt+2");
}

#[test]
fn import_non_canonical_hotkey_detects_conflict_with_canonical_stored() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    upsert_trigger_with_type(
        &conn,
        "local-hotkey",
        "Existing",
        None,
        TriggerType::Hotkey,
        "shift+alt+2",
        "local output",
        "text",
        "all",
        "[]",
        0,
        None,
    )
    .unwrap();

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Hotkey,
        "alt+shift+2",
        "all",
        "imported output",
    )]);
    let tx = conn.transaction().unwrap();
    let imported =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx.commit().unwrap();

    assert_eq!(imported, 1);

    let local_row = crate::db::crud::get_trigger(&conn, "local-hotkey")
        .unwrap()
        .unwrap();
    assert!(local_row.is_deleted);

    let (new_id, new_trigger): (String, String) = conn
        .query_row(
            "SELECT id, trigger FROM triggers WHERE trigger = 'shift+alt+2' AND is_deleted = 0",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_ne!(new_id, "local-hotkey");
    assert_eq!(new_trigger, "shift+alt+2");
}

#[test]
fn import_conflict_identity_keeps_word_and_hotkey_triggers_independent() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    upsert_trigger(
        &conn,
        "local-word",
        "Word",
        None,
        "tab",
        "local",
        "text",
        "all",
        "[]",
        0,
        None,
    )
    .unwrap();

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Hotkey,
        "tab",
        "all",
        "imported hotkey",
    )]);
    let tx = conn.transaction().unwrap();
    let imported =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx.commit().unwrap();

    assert_eq!(imported, 1);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE trigger = 'tab' AND is_deleted = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn failed_import_can_be_rolled_back_atomically() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    let valid_script = TriggerExport {
        name: "Valid Script".to_string(),
        description: Some("script".to_string()),
        trigger_type: TriggerType::Word,
        trigger: "script_ok".to_string(),
        output: "[Script: bash]".to_string(),
        action_type: "script".to_string(),
        is_enabled: true,
        target_os: "all".to_string(),
        tags: vec![],
        script: Some(super::ScriptExport {
            interpreter: ScriptInterpreter::Bash,
            behavior: ScriptBehavior::Inline,
            content: "echo ok".to_string(),
        }),
        assets: Vec::new(),
    };
    let invalid_script = TriggerExport {
        name: "Broken Script".to_string(),
        description: Some("broken".to_string()),
        trigger_type: TriggerType::Word,
        trigger: "script_bad".to_string(),
        output: "[Script: bash]".to_string(),
        action_type: "script".to_string(),
        is_enabled: true,
        target_os: "all".to_string(),
        tags: vec![],
        script: None,
        assets: Vec::new(),
    };

    let payload = ExchangePayload::new(vec![valid_script, invalid_script]);
    let tx = conn.transaction().unwrap();

    let err =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap_err();
    assert!(err.to_string().contains("missing script data"));
    tx.rollback().unwrap();

    let active_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(active_count, 0);
}

#[test]
fn overwrite_conflict_respects_target_os_overlap_for_same_trigger_type() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    insert_raw_word_trigger(&conn, "local-all", "All OS", "gm", "all output", "all", 1);
    insert_raw_word_trigger(
        &conn,
        "local-linux",
        "Linux only",
        "gm",
        "linux output",
        "linux",
        2,
    );

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Word,
        "gm",
        "linux",
        "Imported linux",
    )]);
    let tx = conn.transaction().unwrap();

    let imported =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx.commit().unwrap();
    assert_eq!(imported, 1);

    let local_all = get_trigger(&conn, "local-all").unwrap().unwrap();
    assert!(local_all.is_deleted);

    let local_linux = get_trigger(&conn, "local-linux").unwrap().unwrap();
    assert!(local_linux.is_deleted);
}

#[test]
fn non_overlapping_target_os_values_do_not_conflict_for_same_trigger_type() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    upsert_trigger_with_type(
        &conn,
        "local-hotkey",
        "Windows hotkey",
        None,
        TriggerType::Hotkey,
        "ctrl+shift+g",
        "local",
        "text",
        "win",
        "[]",
        0,
        None,
    )
    .unwrap();

    let payload = ExchangePayload::new(vec![text_export(
        TriggerType::Hotkey,
        "ctrl+shift+g",
        "linux",
        "imported",
    )]);
    let tx = conn.transaction().unwrap();
    let imported =
        import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx.commit().unwrap();

    assert_eq!(imported, 1);

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers
                 WHERE trigger_type = 'hotkey'
                   AND trigger = 'ctrl+shift+g'
                   AND is_deleted = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn test_import_rewrites_asset_uuids() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    let old_asset_id = "11111111-2222-3333-4444-555555555555".to_string();
    let payload = ExchangePayload {
        schema_version: super::EXCHANGE_SCHEMA_VERSION,
        triggers: vec![TriggerExport {
            name: "Test Asset".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "test_asset".to_string(),
            output: format!("Asset here: [asset:{}]", old_asset_id),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            script: None,
            assets: vec![AssetExport {
                id: old_asset_id.clone(),
                mime_type: "image/png".to_string(),
                compressed_content_hex: "89504e470d0a1a0a".to_string(),
            }],
        }],
    };

    // First import
    let tx1 = conn.transaction().unwrap();
    import_triggers(&tx1, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx1.commit().unwrap();

    // Second import (duplicate trigger, but let's import it with overwrite)
    let tx2 = conn.transaction().unwrap();
    import_triggers(&tx2, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
    tx2.commit().unwrap();

    // Retrieve both triggers and assets
    let mut stmt = conn
        .prepare("SELECT id, output FROM triggers WHERE trigger = 'test_asset'")
        .unwrap();
    let autos: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(autos.len(), 2);

    let auto1_id = &autos[0].0;
    let auto1_output = &autos[0].1;
    let auto2_id = &autos[1].0;
    let auto2_output = &autos[1].1;

    assert_ne!(auto1_id, auto2_id);
    assert_ne!(auto1_output, auto2_output);

    // Verify that the assets in the DB are mapped to these new UUIDs
    let auto1_asset_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE trigger_id = ?1",
            [auto1_id],
            |row| row.get(0),
        )
        .unwrap();
    let auto2_asset_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE trigger_id = ?1",
            [auto2_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(auto1_asset_count, 1);
    assert_eq!(auto2_asset_count, 1);
}

#[test]
fn test_import_allows_older_schema_version() {
    init_tracing_for_tests();
    let (_dir, mut conn) = open_test_db();

    let payload = ExchangePayload {
        schema_version: super::EXCHANGE_SCHEMA_VERSION - 1,
        triggers: vec![],
    };

    let tx = conn.transaction().unwrap();
    let res = import_triggers(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite));
    assert!(res.is_ok());
}

#[test]
fn test_import_conflict_action_roundtrip_and_parse_aliases() {
    for action in ImportConflictAction::ALL {
        let label = action.as_str();
        assert_eq!(ImportConflictAction::parse_str(label), Some(action));
    }

    assert_eq!(
        ImportConflictAction::parse_str("replace"),
        Some(ImportConflictAction::Overwrite)
    );
    assert_eq!(
        ImportConflictAction::parse_str("ignore"),
        Some(ImportConflictAction::Skip)
    );
    assert_eq!(ImportConflictAction::parse_str("invalid"), None);
}
