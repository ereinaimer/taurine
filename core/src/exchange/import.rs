use super::{AutomationExport, ExchangePayload};
use crate::db::crud::{upsert_automation, upsert_script};
use crate::engine::shell::compress;
use rusqlite::Transaction;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportConflictAction {
    Overwrite,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingAutomationConflict {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub trigger: String,
    pub output: String,
    pub action_type: String,
    pub target_os: String,
    pub is_enabled: bool,
}

pub fn import_automations<F>(
    tx: &Transaction<'_>,
    payload: &ExchangePayload,
    mut resolve_conflict: F,
) -> crate::Result<usize>
where
    F: FnMut(&AutomationExport, &ExistingAutomationConflict) -> crate::Result<ImportConflictAction>,
{
    payload.validate_schema_version()?;

    let mut imported = 0usize;
    for automation in &payload.automations {
        if let Some(existing) = find_conflicting_automation(
            tx,
            automation.trigger.as_str(),
            automation.target_os.as_str(),
        )? {
            match resolve_conflict(automation, &existing)? {
                ImportConflictAction::Overwrite => {
                    tombstone_conflicting_automations(
                        tx,
                        automation.trigger.as_str(),
                        automation.target_os.as_str(),
                    )?;
                }
                ImportConflictAction::Skip => continue,
            }
        }

        insert_imported_automation(tx, automation)?;
        imported += 1;
    }

    Ok(imported)
}

fn insert_imported_automation(
    tx: &Transaction<'_>,
    automation: &AutomationExport,
) -> crate::Result<()> {
    let id = Uuid::new_v4().to_string();
    let tags_json = serde_json::to_string(&automation.tags)?;

    upsert_automation(
        tx,
        &id,
        &automation.name,
        automation.description.as_deref(),
        &automation.trigger,
        &automation.output,
        &automation.action_type,
        &automation.target_os,
        &tags_json,
        0,
        None,
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

    Ok(())
}

fn find_conflicting_automation(
    tx: &Transaction<'_>,
    trigger: &str,
    target_os: &str,
) -> crate::Result<Option<ExistingAutomationConflict>> {
    let mut stmt = tx.prepare_cached(
        "SELECT id, name, description, trigger, output, action_type, target_os, is_enabled
         FROM automations
         WHERE trigger = ?1
           AND target_os = ?2
           AND is_deleted = 0
         ORDER BY updated_at DESC
         LIMIT 1",
    )?;

    let result = stmt.query_row([trigger, target_os], |row| {
        Ok(ExistingAutomationConflict {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            trigger: row.get(3)?,
            output: row.get(4)?,
            action_type: row.get(5)?,
            target_os: row.get(6)?,
            is_enabled: row.get(7)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn tombstone_conflicting_automations(
    tx: &Transaction<'_>,
    trigger: &str,
    target_os: &str,
) -> crate::Result<()> {
    tx.execute(
        "UPDATE automations
         SET is_deleted = 1,
             version = version + 1,
             updated_at = ?1
         WHERE trigger = ?2
           AND target_os = ?3
           AND is_deleted = 0",
        rusqlite::params![crate::db::now_unix_secs(), trigger, target_os],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::crud::{get_automation, upsert_automation};
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};
    use crate::testing::{init_tracing_for_tests, open_test_db};

    fn text_export(trigger: &str, target_os: &str, output: &str) -> AutomationExport {
        AutomationExport {
            name: format!("Imported {trigger}"),
            description: Some("Imported automation".to_string()),
            trigger: trigger.to_string(),
            output: output.to_string(),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: target_os.to_string(),
            tags: vec!["imported".to_string()],
            script: None,
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

        let payload = ExchangePayload::new(vec![text_export("gm", "all", "Imported output")]);
        let tx = conn.transaction().unwrap();
        let imported =
            import_automations(&tx, &payload, |_, _| Ok(ImportConflictAction::Skip)).unwrap();
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

        let payload = ExchangePayload::new(vec![text_export("gm", "all", "Imported output")]);
        let tx = conn.transaction().unwrap();
        let imported =
            import_automations(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
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
    fn failed_import_can_be_rolled_back_atomically() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        let valid_script = AutomationExport {
            name: "Valid Script".to_string(),
            description: Some("script".to_string()),
            trigger: "script_ok".to_string(),
            output: "[Script: bash]".to_string(),
            action_type: "script".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            script: Some(super::super::ScriptExport {
                interpreter: ScriptInterpreter::Bash,
                behavior: ScriptBehavior::Inline,
                content: "echo ok".to_string(),
            }),
        };
        let invalid_script = AutomationExport {
            name: "Broken Script".to_string(),
            description: Some("broken".to_string()),
            trigger: "script_bad".to_string(),
            output: "[Script: bash]".to_string(),
            action_type: "script".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            script: None,
        };

        let payload = ExchangePayload::new(vec![valid_script, invalid_script]);
        let tx = conn.transaction().unwrap();

        let err = import_automations(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite))
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
    fn overwrite_remains_exact_to_trigger_and_target_os() {
        init_tracing_for_tests();
        let (_dir, mut conn) = open_test_db();

        upsert_automation(
            &conn,
            "local-all",
            "All OS",
            None,
            "gm",
            "all output",
            "text",
            "all",
            "[]",
            1,
            None,
        )
        .unwrap();
        upsert_automation(
            &conn,
            "local-linux",
            "Linux only",
            None,
            "gm",
            "linux output",
            "text",
            "linux",
            "[]",
            2,
            None,
        )
        .unwrap();

        let payload = ExchangePayload::new(vec![text_export("gm", "linux", "Imported linux")]);
        let tx = conn.transaction().unwrap();
        import_automations(&tx, &payload, |_, _| Ok(ImportConflictAction::Overwrite)).unwrap();
        tx.commit().unwrap();

        let local_all = get_automation(&conn, "local-all").unwrap().unwrap();
        assert!(!local_all.is_deleted);

        let local_linux = get_automation(&conn, "local-linux").unwrap().unwrap();
        assert!(local_linux.is_deleted);
    }
}
