use super::{AutomationExport, ExchangePayload, MetricExport};
use crate::db::crud::{
    TriggerType, increment_metric, target_os_values_overlap, upsert_automation_with_trigger_type,
    upsert_script, upsert_setting,
};
use crate::engine::shell::compress;
use rusqlite::{Connection, Transaction};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportConflictAction {
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportMetricsMode {
    #[default]
    Ignore,
    Merge,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportOptions {
    pub include_settings: bool,
    pub metrics_mode: ImportMetricsMode,
    pub include_sensitive_settings: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingAutomationConflict {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger_type: TriggerType,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub target_os: String,
    pub is_enabled: bool,
    pub usage_count: i64,
    pub last_used_at: Option<i64>,
}

pub fn import_automations<F>(
    tx: &Transaction<'_>,
    payload: &ExchangePayload,
    options: ImportOptions,
    mut resolve_conflict: F,
) -> crate::Result<usize>
where
    F: FnMut(&AutomationExport, &ExistingAutomationConflict) -> crate::Result<ImportConflictAction>,
{
    payload.validate_schema_version()?;

    let mut imported = 0usize;
    for automation in &payload.automations {
        let existing = find_conflicting_automation(
            tx,
            automation.trigger_type,
            automation.trigger.as_str(),
            automation.target_os.as_str(),
        )?;

        if let Some(existing) = existing.as_ref() {
            match resolve_conflict(automation, existing)? {
                ImportConflictAction::Overwrite => {
                    tombstone_conflicting_automations(
                        tx,
                        automation.trigger_type,
                        automation.trigger.as_str(),
                        automation.target_os.as_str(),
                    )?;
                }
                ImportConflictAction::Skip => continue,
            }
        }

        insert_imported_automation(tx, automation, existing.as_ref(), options.metrics_mode)?;
        imported += 1;
    }

    if options.include_settings {
        import_settings(tx, payload, options.include_sensitive_settings)?;
    }

    import_global_metrics(tx, payload, options.metrics_mode)?;

    Ok(imported)
}

pub fn import_payload_transactionally<F>(
    conn: &mut Connection,
    payload: &ExchangePayload,
    options: ImportOptions,
    mut resolve_conflict: F,
) -> crate::Result<usize>
where
    F: FnMut(&AutomationExport, &ExistingAutomationConflict) -> crate::Result<ImportConflictAction>,
{
    let tx = conn.transaction()?;
    let result = import_automations(&tx, payload, options, |incoming, existing| {
        resolve_conflict(incoming, existing)
    });

    match result {
        Ok(imported) => {
            tx.commit()?;
            Ok(imported)
        }
        Err(err) => {
            tx.rollback()?;
            Err(err)
        }
    }
}

fn insert_imported_automation(
    tx: &Transaction<'_>,
    automation: &AutomationExport,
    existing: Option<&ExistingAutomationConflict>,
    metrics_mode: ImportMetricsMode,
) -> crate::Result<()> {
    let id = Uuid::new_v4().to_string();
    let tags_json = serde_json::to_string(&automation.tags)?;
    let (usage_count, last_used_at) =
        resolve_automation_metrics(automation, existing, metrics_mode);

    upsert_automation_with_trigger_type(
        tx,
        &id,
        &automation.name,
        automation.description.as_deref(),
        automation.trigger_type,
        &automation.trigger,
        &automation.output,
        &automation.action_type,
        &automation.target_os,
        &tags_json,
        usage_count,
        last_used_at,
    )?;

    if !automation.is_enabled {
        tx.execute(
            "UPDATE automations
             SET is_enabled = 0
             WHERE id = ?1",
            [&id],
        )?;
    }

    if automation.action_type == "script" {
        let script = automation.script.as_ref().ok_or_else(|| {
            crate::Error::Config(format!(
                "Script automation '{}' is missing script data",
                automation.trigger
            ))
        })?;
        let compressed = compress(&script.content)?;
        upsert_script(tx, &id, script.interpreter, script.behavior, &compressed)?;
    }

    let now = crate::db::now_unix_secs();
    // To prevent asset link corruption, rewrite asset UUIDs.
    // Map of old asset ID to new asset ID
    let mut asset_id_map = std::collections::HashMap::new();

    for asset in &automation.assets {
        let new_asset_id = Uuid::new_v4().to_string();
        asset_id_map.insert(asset.id.clone(), new_asset_id.clone());

        let compressed = hex::decode(&asset.compressed_content_hex)
            .map_err(|e| crate::Error::Config(format!("Failed to decode asset hex: {}", e)))?;
        tx.execute(
            "INSERT OR REPLACE INTO assets (id, automation_id, mime_type, compressed_content, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                &new_asset_id,
                &id,
                &asset.mime_type,
                &compressed,
                now,
            ),
        )?;
    }

    // Now, rewrite any references to the old asset UUIDs in the output and script content
    if !asset_id_map.is_empty() {
        let mut final_output = automation.output.clone();
        for (old_id, new_id) in &asset_id_map {
            final_output = final_output.replace(old_id, new_id);
        }

        if final_output != automation.output {
            tx.execute(
                "UPDATE automations SET output = ?1 WHERE id = ?2",
                [&final_output, &id],
            )?;
        }

        if automation.action_type == "script" {
            let script = automation.script.as_ref().unwrap();
            let mut final_script_content = script.content.clone();
            for (old_id, new_id) in &asset_id_map {
                final_script_content = final_script_content.replace(old_id, new_id);
            }
            if final_script_content != script.content {
                let compressed = compress(&final_script_content)?;
                tx.execute(
                    "UPDATE scripts SET content = ?1 WHERE automation_id = ?2",
                    rusqlite::params![&compressed, &id],
                )?;
            }
        }
    }

    Ok(())
}

fn find_conflicting_automation(
    tx: &Transaction<'_>,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
) -> crate::Result<Option<ExistingAutomationConflict>> {
    let mut stmt = tx.prepare_cached(
        "SELECT id, name, description, trigger_type, trigger, output, action_type, target_os, is_enabled,
                usage_count, last_used_at
         FROM automations
         WHERE trigger_type = ?1
           AND trigger = ?2
           AND is_deleted = 0
         ORDER BY updated_at DESC",
    )?;

    let rows = stmt.query_map([trigger_type.as_db_str(), trigger], |row| {
        let trigger_type_raw: String = row.get(3)?;
        Ok(ExistingAutomationConflict {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            trigger_type: TriggerType::parse_db(&trigger_type_raw).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    3,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?,
            trigger: row.get(4)?,
            output: row.get(5)?,
            action_type: row.get(6)?,
            target_os: row.get(7)?,
            is_enabled: row.get(8)?,
            usage_count: row.get(9)?,
            last_used_at: row.get(10)?,
        })
    })?;

    for row in rows {
        let row = row?;
        if target_os_values_overlap(&row.target_os, target_os) {
            return Ok(Some(row));
        }
    }

    Ok(None)
}

fn tombstone_conflicting_automations(
    tx: &Transaction<'_>,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
) -> crate::Result<()> {
    let mut stmt = tx.prepare_cached(
        "SELECT id, target_os
         FROM automations
         WHERE trigger_type = ?1
           AND trigger = ?2
           AND is_deleted = 0",
    )?;
    let rows = stmt.query_map([trigger_type.as_db_str(), trigger], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let now = crate::db::now_unix_secs();
    for row in rows {
        let (id, existing_target_os) = row?;
        if target_os_values_overlap(&existing_target_os, target_os) {
            tx.execute(
                "UPDATE automations
                 SET is_deleted = 1,
                     version = version + 1,
                     updated_at = ?1
                 WHERE id = ?2",
                rusqlite::params![now, id],
            )?;
        }
    }

    Ok(())
}

fn resolve_automation_metrics(
    automation: &AutomationExport,
    existing: Option<&ExistingAutomationConflict>,
    metrics_mode: ImportMetricsMode,
) -> (i64, Option<i64>) {
    let imported_usage_count = automation.usage_count.unwrap_or(0);
    let imported_last_used_at = automation.last_used_at;

    match metrics_mode {
        ImportMetricsMode::Ignore => (0, None),
        ImportMetricsMode::Overwrite => (imported_usage_count, imported_last_used_at),
        ImportMetricsMode::Merge => {
            if let Some(existing) = existing {
                (
                    existing.usage_count + imported_usage_count,
                    max_option_i64(existing.last_used_at, imported_last_used_at),
                )
            } else {
                (imported_usage_count, imported_last_used_at)
            }
        }
    }
}

fn import_settings(
    tx: &Transaction<'_>,
    payload: &ExchangePayload,
    include_sensitive_settings: bool,
) -> crate::Result<()> {
    if let Some(settings) = payload.settings.as_ref() {
        for setting in settings {
            if !include_sensitive_settings
                && crate::exchange::export::is_sensitive_setting_key(&setting.key)
            {
                continue;
            }
            upsert_setting(tx, &setting.key, &setting.value)?;
        }
    }

    Ok(())
}

fn import_global_metrics(
    tx: &Transaction<'_>,
    payload: &ExchangePayload,
    metrics_mode: ImportMetricsMode,
) -> crate::Result<()> {
    let Some(metrics) = payload.metrics.as_ref() else {
        return Ok(());
    };

    match metrics_mode {
        ImportMetricsMode::Ignore => Ok(()),
        ImportMetricsMode::Merge => {
            for metric in metrics {
                increment_metric(
                    tx,
                    &metric.date,
                    metric.executions,
                    metric.ai_executions,
                    metric.keystrokes_saved,
                    metric.time_saved_ms,
                )?;
            }
            Ok(())
        }
        ImportMetricsMode::Overwrite => {
            for metric in metrics {
                overwrite_metric_row(tx, metric)?;
            }
            Ok(())
        }
    }
}

fn overwrite_metric_row(tx: &Transaction<'_>, metric: &MetricExport) -> crate::Result<()> {
    tx.execute(
        "INSERT INTO metrics (
             date,
             executions,
             ai_executions,
             keystrokes_saved,
             time_saved_ms,
             version,
             updated_at
         )
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
         ON CONFLICT(date) DO UPDATE SET
             executions = excluded.executions,
             ai_executions = excluded.ai_executions,
             keystrokes_saved = excluded.keystrokes_saved,
             time_saved_ms = excluded.time_saved_ms,
             version = version + 1,
             updated_at = excluded.updated_at",
        (
            &metric.date,
            metric.executions,
            metric.ai_executions,
            metric.keystrokes_saved,
            metric.time_saved_ms,
            crate::db::now_unix_secs(),
        ),
    )?;

    Ok(())
}

fn max_option_i64(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{
        TriggerType, get_automation, upsert_automation, upsert_automation_with_trigger_type,
    };
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
    use crate::exchange::AssetExport;
    use crate::testing::{init_tracing_for_tests, open_test_db};

    fn insert_raw_word_automation(
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
            "INSERT INTO automations (
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
    ) -> AutomationExport {
        AutomationExport {
            name: format!("Imported {trigger}"),
            description: Some("Imported automation".to_string()),
            trigger_type,
            trigger: trigger.to_string(),
            output: output.to_string(),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: target_os.to_string(),
            tags: vec!["imported".to_string()],
            usage_count: None,
            last_used_at: None,
            script: None,
            assets: Vec::new(),
        }
    }

    #[test]
    fn skip_conflict_preserves_existing_local_row() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation(
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
        let imported = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Skip)
        })
        .unwrap();
        tx.commit().unwrap();

        assert_eq!(imported, 0);

        let row = get_automation(&conn, "local-id").unwrap().unwrap();
        assert_eq!(row.output, "Local output");
        assert_eq!(row.usage_count, 27);
        assert!(!row.is_deleted);
    }

    #[test]
    fn overwrite_conflict_replaces_existing_row_with_fresh_import() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation(
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
        let imported = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap();
        tx.commit().unwrap();

        assert_eq!(imported, 1);

        let local_row = get_automation(&conn, "local-id").unwrap().unwrap();
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
                 FROM automations
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
        let imported = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap();
        tx.commit().unwrap();

        assert_eq!(imported, 1);

        let row = conn
            .query_row(
                "SELECT trigger_type, trigger
                 FROM automations
                 WHERE is_deleted = 0",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .unwrap();
        assert_eq!(row.0, "hotkey");
        assert_eq!(row.1, "ctrl+shift+g");
    }

    #[test]
    fn import_conflict_identity_keeps_word_and_hotkey_triggers_independent() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation(
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
        let imported = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap();
        tx.commit().unwrap();

        assert_eq!(imported, 1);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automations WHERE trigger = 'tab' AND is_deleted = 0",
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

        let valid_script = AutomationExport {
            name: "Valid Script".to_string(),
            description: Some("script".to_string()),
            trigger_type: TriggerType::Word,
            trigger: "script_ok".to_string(),
            output: "[Script: bash]".to_string(),
            action_type: "script".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            usage_count: None,
            last_used_at: None,
            script: Some(super::super::ScriptExport {
                interpreter: ScriptInterpreter::Bash,
                behavior: ScriptBehavior::Inline,
                content: "echo ok".to_string(),
            }),
            assets: Vec::new(),
        };
        let invalid_script = AutomationExport {
            name: "Broken Script".to_string(),
            description: Some("broken".to_string()),
            trigger_type: TriggerType::Word,
            trigger: "script_bad".to_string(),
            output: "[Script: bash]".to_string(),
            action_type: "script".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            usage_count: None,
            last_used_at: None,
            script: None,
            assets: Vec::new(),
        };

        let payload = ExchangePayload::new(vec![valid_script, invalid_script]);
        let tx = conn.transaction().unwrap();

        let err = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap_err();
        assert!(err.to_string().contains("missing script data"));
        tx.rollback().unwrap();

        let active_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automations WHERE is_deleted = 0",
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

        insert_raw_word_automation(&conn, "local-all", "All OS", "gm", "all output", "all", 1);
        insert_raw_word_automation(
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

        let imported = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap();
        tx.commit().unwrap();
        assert_eq!(imported, 1);

        let local_all = get_automation(&conn, "local-all").unwrap().unwrap();
        assert!(local_all.is_deleted);

        let local_linux = get_automation(&conn, "local-linux").unwrap().unwrap();
        assert!(local_linux.is_deleted);
    }

    #[test]
    fn non_overlapping_target_os_values_do_not_conflict_for_same_trigger_type() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation_with_trigger_type(
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
        let imported = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap();
        tx.commit().unwrap();

        assert_eq!(imported, 1);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automations
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
    fn import_rejects_reserved_inline_ai_trigger() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let payload = ExchangePayload::new(vec![text_export(
            TriggerType::Word,
            "ai",
            "all",
            "Imported output",
        )]);
        let tx = conn.transaction().unwrap();

        let err = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("reserved for Taurine Inline AI Copilot")
        );
        tx.rollback().unwrap();

        let active_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automations WHERE is_deleted = 0",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active_count, 0);
    }

    #[test]
    fn merge_metrics_combines_local_and_imported_automation_stats() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation(
            &conn,
            "local-id",
            "Local GM",
            None,
            "gm",
            "Local output",
            "text",
            "all",
            "[]",
            20,
            Some(200),
        )
        .unwrap();

        let mut imported = text_export(TriggerType::Word, "gm", "all", "Imported output");
        imported.usage_count = Some(50);
        imported.last_used_at = Some(100);

        let tx = conn.transaction().unwrap();
        import_automations(
            &tx,
            &ExchangePayload::new(vec![imported]),
            ImportOptions {
                include_settings: false,
                metrics_mode: ImportMetricsMode::Merge,
                ..Default::default()
            },
            |_, _| Ok(ImportConflictAction::Overwrite),
        )
        .unwrap();
        tx.commit().unwrap();

        let (usage_count, last_used_at): (i64, Option<i64>) = conn
            .query_row(
                "SELECT usage_count, last_used_at
                 FROM automations
                 WHERE trigger = ?1 AND target_os = ?2 AND is_deleted = 0",
                ["gm", "all"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(usage_count, 70);
        assert_eq!(last_used_at, Some(200));
    }

    #[test]
    fn overwrite_metrics_replaces_local_automation_stats() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation(
            &conn,
            "local-id",
            "Local GM",
            None,
            "gm",
            "Local output",
            "text",
            "all",
            "[]",
            20,
            Some(200),
        )
        .unwrap();

        let mut imported = text_export(TriggerType::Word, "gm", "all", "Imported output");
        imported.usage_count = Some(50);
        imported.last_used_at = Some(100);

        let tx = conn.transaction().unwrap();
        import_automations(
            &tx,
            &ExchangePayload::new(vec![imported]),
            ImportOptions {
                include_settings: false,
                metrics_mode: ImportMetricsMode::Overwrite,
                ..Default::default()
            },
            |_, _| Ok(ImportConflictAction::Overwrite),
        )
        .unwrap();
        tx.commit().unwrap();

        let (usage_count, last_used_at): (i64, Option<i64>) = conn
            .query_row(
                "SELECT usage_count, last_used_at
                 FROM automations
                 WHERE trigger = ?1 AND target_os = ?2 AND is_deleted = 0",
                ["gm", "all"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(usage_count, 50);
        assert_eq!(last_used_at, Some(100));
    }

    #[test]
    fn skip_conflict_also_skips_imported_metrics() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation(
            &conn,
            "local-id",
            "Local GM",
            None,
            "gm",
            "Local output",
            "text",
            "all",
            "[]",
            20,
            Some(200),
        )
        .unwrap();

        let mut imported = text_export(TriggerType::Word, "gm", "all", "Imported output");
        imported.usage_count = Some(50);
        imported.last_used_at = Some(500);

        let tx = conn.transaction().unwrap();
        import_automations(
            &tx,
            &ExchangePayload::new(vec![imported]),
            ImportOptions {
                include_settings: false,
                metrics_mode: ImportMetricsMode::Merge,
                ..Default::default()
            },
            |_, _| Ok(ImportConflictAction::Skip),
        )
        .unwrap();
        tx.commit().unwrap();

        let row = get_automation(&conn, "local-id").unwrap().unwrap();
        assert_eq!(row.usage_count, 20);
        assert_eq!(row.last_used_at, Some(200));
        assert!(!row.is_deleted);
    }

    #[test]
    fn include_settings_overwrites_local_setting_values() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let payload = ExchangePayload {
            schema_version: super::super::EXCHANGE_SCHEMA_VERSION,
            automations: vec![],
            settings: Some(vec![super::super::SettingExport {
                key: "trigger_char".to_string(),
                value: r#"">""#.to_string(),
            }]),
            metrics: None,
        };

        let tx = conn.transaction().unwrap();
        import_automations(
            &tx,
            &payload,
            ImportOptions {
                include_settings: true,
                metrics_mode: ImportMetricsMode::Ignore,
                ..Default::default()
            },
            |_, _| Ok(ImportConflictAction::Overwrite),
        )
        .unwrap();
        tx.commit().unwrap();

        let value: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'trigger_char'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(value, r#"">""#);
    }

    #[test]
    fn merge_global_metrics_sums_rows_by_date() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        conn.execute(
            "INSERT INTO metrics (
                date, executions, ai_executions, keystrokes_saved, time_saved_ms, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                "2026-04-01",
                20_i64,
                4_i64,
                200_i64,
                20_000_i64,
                1_700_000_000_i64,
            ),
        )
        .unwrap();

        let payload = ExchangePayload {
            schema_version: super::super::EXCHANGE_SCHEMA_VERSION,
            automations: vec![],
            settings: None,
            metrics: Some(vec![MetricExport {
                date: "2026-04-01".to_string(),
                executions: 50,
                ai_executions: 5,
                keystrokes_saved: 500,
                time_saved_ms: 50_000,
            }]),
        };

        let tx = conn.transaction().unwrap();
        import_automations(
            &tx,
            &payload,
            ImportOptions {
                include_settings: false,
                metrics_mode: ImportMetricsMode::Merge,
                ..Default::default()
            },
            |_, _| Ok(ImportConflictAction::Overwrite),
        )
        .unwrap();
        tx.commit().unwrap();

        let (executions, ai_executions, saved, time_saved_ms): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT executions, ai_executions, keystrokes_saved, time_saved_ms
                 FROM metrics
                 WHERE date = ?1",
                ["2026-04-01"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(executions, 70);
        assert_eq!(ai_executions, 9);
        assert_eq!(saved, 700);
        assert_eq!(time_saved_ms, 70_000);
    }

    #[test]
    fn overwrite_global_metrics_replaces_rows_by_date() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        conn.execute(
            "INSERT INTO metrics (
                date, executions, ai_executions, keystrokes_saved, time_saved_ms, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            (
                "2026-04-01",
                20_i64,
                4_i64,
                200_i64,
                20_000_i64,
                1_700_000_000_i64,
            ),
        )
        .unwrap();

        let payload = ExchangePayload {
            schema_version: super::super::EXCHANGE_SCHEMA_VERSION,
            automations: vec![],
            settings: None,
            metrics: Some(vec![MetricExport {
                date: "2026-04-01".to_string(),
                executions: 50,
                ai_executions: 5,
                keystrokes_saved: 500,
                time_saved_ms: 50_000,
            }]),
        };

        let tx = conn.transaction().unwrap();
        import_automations(
            &tx,
            &payload,
            ImportOptions {
                include_settings: false,
                metrics_mode: ImportMetricsMode::Overwrite,
                ..Default::default()
            },
            |_, _| Ok(ImportConflictAction::Overwrite),
        )
        .unwrap();
        tx.commit().unwrap();

        let (executions, ai_executions, saved, time_saved_ms): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT executions, ai_executions, keystrokes_saved, time_saved_ms
                 FROM metrics
                 WHERE date = ?1",
                ["2026-04-01"],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(executions, 50);
        assert_eq!(ai_executions, 5);
        assert_eq!(saved, 500);
        assert_eq!(time_saved_ms, 50_000);
    }

    #[test]
    fn test_import_rewrites_asset_uuids() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let old_asset_id = "11111111-2222-3333-4444-555555555555".to_string();
        let payload = ExchangePayload {
            schema_version: super::super::EXCHANGE_SCHEMA_VERSION,
            automations: vec![AutomationExport {
                name: "Test Asset".to_string(),
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "test_asset".to_string(),
                output: format!("Asset here: [asset:{}]", old_asset_id),
                action_type: "text".to_string(),
                is_enabled: true,
                target_os: "all".to_string(),
                tags: vec![],
                usage_count: None,
                last_used_at: None,
                script: None,
                assets: vec![AssetExport {
                    id: old_asset_id.clone(),
                    mime_type: "image/png".to_string(),
                    compressed_content_hex: "89504e470d0a1a0a".to_string(),
                }],
            }],
            settings: None,
            metrics: None,
        };

        // First import
        let tx1 = conn.transaction().unwrap();
        import_automations(&tx1, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap();
        tx1.commit().unwrap();

        // Second import (duplicate trigger, but let's import it with overwrite)
        let tx2 = conn.transaction().unwrap();
        import_automations(&tx2, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        })
        .unwrap();
        tx2.commit().unwrap();

        // Retrieve both automations and assets
        let mut stmt = conn
            .prepare("SELECT id, output FROM automations WHERE trigger = 'test_asset'")
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
                "SELECT COUNT(*) FROM assets WHERE automation_id = ?1",
                [auto1_id],
                |row| row.get(0),
            )
            .unwrap();
        let auto2_asset_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM assets WHERE automation_id = ?1",
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
            schema_version: super::super::EXCHANGE_SCHEMA_VERSION - 1,
            automations: vec![],
            settings: None,
            metrics: None,
        };

        let tx = conn.transaction().unwrap();
        let res = import_automations(&tx, &payload, ImportOptions::default(), |_, _| {
            Ok(ImportConflictAction::Overwrite)
        });
        assert!(res.is_ok());
    }
}
