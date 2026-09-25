use super::*;
use crate::db::crud::{
    InvocationType, NewEntry, TriggerType, create_entry, get_trigger, get_triggers_list,
    upsert_trigger, upsert_trigger_with_type,
};
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
use crate::exchange::AssetExport;
use crate::exchange::export::export_triggers;
use crate::testing::{init_tracing_for_tests, open_test_db};

fn insert_entry_word(
    conn: &rusqlite::Connection,
    name: &str,
    trigger: &str,
    output: &str,
    target_os: &str,
) {
    create_entry(
        conn,
        NewEntry {
            name: name.to_string(),
            description: None,
            content: output.to_string(),
            action_type: "text".to_string(),
            target_os: target_os.to_string(),
            only_apps: None,
            except_apps: None,
            tags_json: "[]".to_string(),
            auto_case: false,
            interpreter: None,
            behavior: None,
            invocations: vec![(InvocationType::Word, trigger.to_string(), false)],
        },
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
        aliases: vec![],
    }
}

fn entry_fixture(invocations: Vec<(InvocationType, &str)>) -> NewEntry {
    NewEntry {
        name: String::new(),
        description: None,
        content: "Hello!".to_string(),
        action_type: "text".to_string(),
        target_os: "all".to_string(),
        only_apps: None,
        except_apps: None,
        tags_json: "[]".to_string(),
        auto_case: false,
        interpreter: None,
        behavior: None,
        invocations: invocations
            .into_iter()
            .map(|(t, s)| (t, s.to_string(), false))
            .collect(),
    }
}

#[test]
fn export_import_roundtrip_preserves_aliases() {
    init_tracing_for_tests();
    let (_dir, conn) = open_test_db();
    let (_, _) = create_entry(
        &conn,
        entry_fixture(vec![
            (InvocationType::Word, "hi"),
            (InvocationType::Hotkey, "ctrl+h"),
            (InvocationType::Voice, "say hi"),
        ]),
    )
    .unwrap();
    let payload = export_triggers(&conn).unwrap();
    assert_eq!(payload.triggers.len(), 1);
    assert_eq!(payload.triggers[0].aliases.len(), 3);
    let (_dir2, mut conn2) = open_test_db();
    import_payload_transactionally(&mut conn2, &payload, |_, _| {
        Ok(ImportConflictAction::Overwrite)
    })
    .unwrap();
    let list = get_triggers_list(&conn2).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].invocations.len(), 3);
}

#[test]
fn legacy_single_trigger_payload_still_imports() {
    init_tracing_for_tests();
    let payload = ExchangePayload::new(vec![TriggerExport {
        name: String::new(),
        description: None,
        trigger_type: TriggerType::Word,
        trigger: "hi".into(),
        output: "Hello!".into(),
        action_type: "text".into(),
        is_enabled: true,
        target_os: "all".into(),
        tags: vec![],
        script: None,
        assets: vec![],
        aliases: vec![],
    }]);
    let (_dir, mut conn) = open_test_db();
    import_payload_transactionally(&mut conn, &payload, |_, _| {
        Ok(ImportConflictAction::Overwrite)
    })
    .unwrap();
    assert_eq!(get_triggers_list(&conn).unwrap()[0].invocations.len(), 1);
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
            "SELECT t.id, t.usage_count, t.last_used_at, t.is_deleted, t.output
                 FROM triggers t
                 JOIN trigger_aliases al ON al.trigger_id = t.id
                 WHERE al.invocation_type = 'word' AND al.invocation = ?1
                   AND t.target_os = ?2 AND t.is_deleted = 0",
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
            "SELECT al.invocation_type, al.invocation
                 FROM trigger_aliases al
                 JOIN triggers t ON t.id = al.trigger_id
                 WHERE t.is_deleted = 0",
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
            "SELECT al.invocation
                 FROM trigger_aliases al
                 JOIN triggers t ON t.id = al.trigger_id
                 WHERE al.invocation_type = 'hotkey' AND t.is_deleted = 0",
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
            "SELECT t.id, al.invocation
                 FROM triggers t
                 JOIN trigger_aliases al ON al.trigger_id = t.id
                 WHERE al.invocation_type = 'hotkey'
                   AND al.invocation = 'shift+alt+2' AND t.is_deleted = 0",
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
            "SELECT COUNT(DISTINCT al.trigger_id)
                 FROM trigger_aliases al
                 JOIN triggers t ON t.id = al.trigger_id
                 WHERE al.invocation = 'tab' AND t.is_deleted = 0",
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
        aliases: vec![],
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
        aliases: vec![],
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

    insert_entry_word(&conn, "Windows only", "gm", "win output", "win");
    let (linux_id, _) = create_entry(
        &conn,
        NewEntry {
            name: "Linux only".to_string(),
            description: None,
            content: "linux output".to_string(),
            action_type: "text".to_string(),
            target_os: "linux".to_string(),
            only_apps: None,
            except_apps: None,
            tags_json: "[]".to_string(),
            auto_case: false,
            interpreter: None,
            behavior: None,
            invocations: vec![(InvocationType::Word, "gm".to_string(), false)],
        },
    )
    .unwrap();

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

    let win_active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE target_os = 'win' AND is_deleted = 0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(win_active, 1);

    let local_linux = get_trigger(&conn, &linux_id).unwrap().unwrap();
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
            "SELECT COUNT(DISTINCT al.trigger_id)
                 FROM trigger_aliases al
                 JOIN triggers t ON t.id = al.trigger_id
                 WHERE al.invocation_type = 'hotkey'
                   AND al.invocation = 'ctrl+shift+g'
                   AND t.is_deleted = 0",
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
            output: format!("Asset here: [image(asset({}))]", old_asset_id),
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
            aliases: vec![],
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

    // Retrieve both triggers and assets (the overwritten first import is
    // tombstoned and keeps no aliases, so scope by table, not invocation).
    let mut stmt = conn.prepare("SELECT id, output FROM triggers").unwrap();
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
