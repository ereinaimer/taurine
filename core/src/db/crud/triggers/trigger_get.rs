use rusqlite::types::Type;
use rusqlite::{Connection, Result};

use super::{
    InvocationType, ResolvedInvocation, TriggerAction, TriggerListItem, TriggerRow, TriggerSummary,
    display_for_aliases, list_aliases, threshold_for_phrase,
};
use crate::db::crud::TargetOs;
use crate::db::crud::get_current_os_db_string;
use crate::engine::shell::{
    ScriptBehavior, ScriptInterpreter, compress, decompress, infer_interpreter,
};

/// Helper to parse JSON variants that might contain double-quotes from SQLite.
pub(crate) fn parse_json_variant<T: serde::de::DeserializeOwned>(s: Option<String>) -> Option<T> {
    s.and_then(|val| {
        let trimmed = val.trim_matches('"');
        serde_json::from_str::<T>(&format!("\"{}\"", trimmed)).ok()
    })
}

fn alias_err(err: crate::Error) -> rusqlite::Error {
    match err {
        crate::Error::Database(inner) => inner,
        other => rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(other)),
    }
}

/// Builds a [`TriggerAction`] for a voice invocation, carrying the parent id.
///
/// Script actions mirror the interpreter-inference logic from the deleted
/// `voice_trigger_row_to_action`: shebang → infer, else `TargetOs`-default;
/// Silent behavior; compress output. Stored script-row values win when present.
#[allow(clippy::too_many_arguments)]
fn voice_action(
    output: String,
    action_type: String,
    target_os: &str,
    only_apps: Option<String>,
    except_apps: Option<String>,
    parent_id: String,
    interpreter: Option<ScriptInterpreter>,
    behavior: Option<ScriptBehavior>,
    script_binary: Option<Vec<u8>>,
) -> TriggerAction {
    if action_type.trim().eq_ignore_ascii_case("script") {
        let probe = script_binary
            .as_deref()
            .and_then(|blob| decompress(blob).ok())
            .unwrap_or_else(|| output.clone());
        let inferred = infer_interpreter(None, &probe).unwrap_or_else(|| {
            let os = TargetOs::parse_str(target_os).unwrap_or(TargetOs::All);
            ScriptInterpreter::default_for_target_os(os)
        });
        let binary = script_binary.or_else(|| compress(&output).ok());
        return TriggerAction {
            output,
            action_type: "script".into(),
            only_apps,
            except_apps,
            auto_case: false,
            parent_id,
            interpreter: interpreter.or(Some(inferred)),
            behavior: behavior.or(Some(ScriptBehavior::Silent)),
            script_binary: binary,
        };
    }
    TriggerAction {
        output,
        action_type,
        only_apps,
        except_apps,
        auto_case: false,
        parent_id,
        interpreter: None,
        behavior: None,
        script_binary: None,
    }
}

/// Returns the full row for `id`, or `None` if it does not exist.
pub fn get_trigger(conn: &Connection, id: &str) -> Result<Option<TriggerRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
            a.id,
            a.name,
            a.description,
            a.output,
            a.action_type,
            a.target_os,
            a.only_apps,
            a.except_apps,
            a.tags,
            a.usage_count,
            a.last_used_at,
            a.created_at,
            a.updated_at,
            a.version,
            a.is_deleted,
            a.is_synced,
            a.is_enabled,
            a.auto_case,
            s.interpreter,
            s.behavior,
            s.compressed_content
         FROM triggers a
         LEFT JOIN scripts s ON a.id = s.trigger_id
         WHERE a.id = ?1",
    )?;

    let result = stmt.query_row([id], |row| {
        let interpreter = parse_json_variant(row.get(18)?);
        let behavior = parse_json_variant(row.get(19)?);

        Ok(TriggerRow {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            invocations: Vec::new(),
            display: String::new(),
            output: row.get(3)?,
            action_type: row.get(4)?,
            target_os: row.get(5)?,
            only_apps: row.get(6)?,
            except_apps: row.get(7)?,
            tags: row.get(8)?,
            usage_count: row.get(9)?,
            last_used_at: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            version: row.get(13)?,
            is_deleted: row.get(14)?,
            is_synced: row.get(15)?,
            is_enabled: row.get(16)?,
            auto_case: row.get(17)?,
            interpreter,
            behavior,
            script_binary: row.get(20)?,
        })
    });

    match result {
        Ok(mut row) => {
            row.invocations = list_aliases(conn, &row.id).map_err(alias_err)?;
            row.display = display_for_aliases(&row.name, &row.invocations);
            Ok(Some(row))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Hot-path lookup: returns just `(payload, action_type)` for an active trigger.
///
/// Uses the `idx_active_triggers` partial index by matching its predicate:
/// `WHERE is_deleted = 0 AND is_enabled = 1 AND trigger_type = 'word' AND trigger = ?`.
pub fn get_action_by_trigger(conn: &Connection, trigger: &str) -> Result<Option<TriggerAction>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT t.output, t.action_type, t.only_apps, t.except_apps, t.auto_case, t.id, s.interpreter, s.behavior, s.compressed_content
         FROM   trigger_aliases al
         JOIN   triggers t ON t.id = al.trigger_id
         LEFT JOIN scripts s ON s.trigger_id = t.id
         WHERE  al.invocation_type = 'word'
           AND  al.invocation = ?1
           AND  t.is_deleted = 0
           AND  t.is_enabled = 1
           AND  (t.target_os = 'all' OR t.target_os = ?2)
         ORDER BY (t.target_os != 'all') DESC, t.usage_count DESC
         LIMIT 1",
    )?;

    let result = stmt.query_row(rusqlite::params![trigger, os_str], |row| {
        let interpreter = parse_json_variant(row.get(6)?);
        let behavior = parse_json_variant(row.get(7)?);

        Ok(TriggerAction {
            output: row.get(0)?,
            action_type: row.get(1)?,
            only_apps: row.get(2)?,
            except_apps: row.get(3)?,
            auto_case: row.get(4)?,
            parent_id: row.get(5)?,
            interpreter,
            behavior,
            script_binary: row.get(8)?,
        })
    });

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Fetches just the trigger strings for all active triggers.
///
/// Use this at app startup to build a fast in-memory lookup cache.
pub fn get_all_active_triggers(conn: &Connection) -> Result<Vec<(String, TriggerAction)>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT al.invocation, t.output, t.action_type, t.only_apps, t.except_apps, t.auto_case, t.id, s.interpreter, s.behavior, s.compressed_content
FROM trigger_aliases al JOIN triggers t ON t.id = al.trigger_id
LEFT JOIN scripts s ON s.trigger_id = t.id
WHERE al.invocation_type = 'word' AND t.is_deleted = 0 AND t.is_enabled = 1
AND (t.target_os = 'all' OR t.target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        let interpreter = parse_json_variant(row.get(7)?);
        let behavior = parse_json_variant(row.get(8)?);

        Ok((
            row.get(0)?,
            TriggerAction {
                output: row.get(1)?,
                action_type: row.get(2)?,
                only_apps: row.get(3)?,
                except_apps: row.get(4)?,
                auto_case: row.get(5)?,
                parent_id: row.get(6)?,
                interpreter,
                behavior,
                script_binary: row.get(9)?,
            },
        ))
    })?;

    let mut actions = Vec::new();
    for action in rows {
        actions.push(action?);
    }

    Ok(actions)
}

/// Fetches all active regex triggers for the current desktop target.
pub fn get_all_active_regex_triggers(conn: &Connection) -> Result<Vec<(String, TriggerAction)>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT al.invocation, t.output, t.action_type, t.only_apps, t.except_apps, t.auto_case, t.id, s.interpreter, s.behavior, s.compressed_content
FROM trigger_aliases al JOIN triggers t ON t.id = al.trigger_id
LEFT JOIN scripts s ON s.trigger_id = t.id
WHERE al.invocation_type = 'regex' AND t.is_deleted = 0 AND t.is_enabled = 1
AND (t.target_os = 'all' OR t.target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        let interpreter = parse_json_variant(row.get(7)?);
        let behavior = parse_json_variant(row.get(8)?);

        Ok((
            row.get(0)?,
            TriggerAction {
                output: row.get(1)?,
                action_type: row.get(2)?,
                only_apps: row.get(3)?,
                except_apps: row.get(4)?,
                auto_case: row.get(5)?,
                parent_id: row.get(6)?,
                interpreter,
                behavior,
                script_binary: row.get(9)?,
            },
        ))
    })?;

    let mut actions = Vec::new();
    for action in rows {
        actions.push(action?);
    }

    Ok(actions)
}

/// Fetches all active hotkey triggers for the current desktop target.
///
/// This is a future-facing load path for daemon hotkey matching. The text
/// evaluator must continue to use `get_all_active_triggers`, which is
/// intentionally word-only.
pub fn get_all_active_hotkey_triggers(conn: &Connection) -> Result<Vec<(String, TriggerAction)>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT al.invocation, t.output, t.action_type, t.only_apps, t.except_apps, t.auto_case, t.id, s.interpreter, s.behavior, s.compressed_content
FROM trigger_aliases al JOIN triggers t ON t.id = al.trigger_id
LEFT JOIN scripts s ON s.trigger_id = t.id
WHERE al.invocation_type = 'hotkey' AND t.is_deleted = 0 AND t.is_enabled = 1
AND (t.target_os = 'all' OR t.target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        let interpreter = parse_json_variant(row.get(7)?);
        let behavior = parse_json_variant(row.get(8)?);

        Ok((
            row.get(0)?,
            TriggerAction {
                output: row.get(1)?,
                action_type: row.get(2)?,
                only_apps: row.get(3)?,
                except_apps: row.get(4)?,
                auto_case: row.get(5)?,
                parent_id: row.get(6)?,
                interpreter,
                behavior,
                script_binary: row.get(9)?,
            },
        ))
    })?;

    let mut actions = Vec::new();
    for action in rows {
        actions.push(action?);
    }

    Ok(actions)
}

/// Fetches all active voice invocations with their parent payloads.
///
/// Voice aliases JOIN parents (+ scripts join for script actions), filling
/// confirmation/threshold and the action with `parent_id`.
pub fn list_active_voice_invocations(conn: &Connection) -> Result<Vec<ResolvedInvocation>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT al.invocation, al.require_confirmation, al.strict_threshold,
                t.id, t.output, t.action_type, t.only_apps, t.except_apps, t.target_os,
                s.interpreter, s.behavior, s.compressed_content
         FROM trigger_aliases al
         JOIN triggers t ON t.id = al.trigger_id
         LEFT JOIN scripts s ON s.trigger_id = t.id
         WHERE al.invocation_type = 'voice' AND t.is_deleted = 0 AND t.is_enabled = 1
         AND (t.target_os = 'all' OR t.target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        let invocation: String = row.get(0)?;
        let require_confirmation: bool = row.get(1)?;
        let strict_threshold: Option<f32> = row.get(2)?;
        let trigger_id: String = row.get(3)?;
        let output: String = row.get(4)?;
        let action_type: String = row.get(5)?;
        let only_apps: Option<String> = row.get(6)?;
        let except_apps: Option<String> = row.get(7)?;
        let target_os: String = row.get(8)?;
        let interpreter = parse_json_variant(row.get(9)?);
        let behavior = parse_json_variant(row.get(10)?);
        let script_binary: Option<Vec<u8>> = row.get(11)?;

        let action = voice_action(
            output,
            action_type,
            &target_os,
            only_apps,
            except_apps,
            trigger_id.clone(),
            interpreter,
            behavior,
            script_binary,
        );
        Ok(ResolvedInvocation {
            strict_threshold: strict_threshold.unwrap_or_else(|| threshold_for_phrase(&invocation)),
            trigger_id,
            invocation,
            invocation_type: InvocationType::Voice,
            action,
            require_confirmation,
        })
    })?;

    let mut invocations = Vec::new();
    for row in rows {
        invocations.push(row?);
    }

    Ok(invocations)
}

/// Fetches all active triggers with enough metadata for sorting/listing in CLI.
pub fn get_triggers_list(conn: &Connection) -> Result<Vec<TriggerListItem>> {
    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT a.id, a.name, a.description, a.output, a.action_type, a.target_os,
                a.only_apps, a.except_apps, a.usage_count, a.last_used_at, a.created_at,
                a.tags, s.interpreter, s.behavior, s.compressed_content
         FROM   triggers a
         LEFT JOIN scripts s ON a.id = s.trigger_id
         WHERE  a.is_deleted = 0
           AND  a.is_enabled = 1
           AND  (a.target_os = 'all' OR a.target_os = ?1)",
    )?;

    let rows = stmt.query_map([os_str], |row| {
        let interpreter = parse_json_variant(row.get(12)?);
        let behavior = parse_json_variant(row.get(13)?);
        let script_content = row
            .get::<_, Option<Vec<u8>>>(14)?
            .map(|compressed| {
                decompress(&compressed).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(14, Type::Blob, Box::new(err))
                })
            })
            .transpose()?;

        Ok(TriggerListItem {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            invocations: Vec::new(),
            display: String::new(),
            output: row.get(3)?,
            action_type: row.get(4)?,
            target_os: row.get(5)?,
            only_apps: row.get(6)?,
            except_apps: row.get(7)?,
            usage_count: row.get(8)?,
            last_used_at: row.get(9)?,
            created_at: row.get(10)?,
            tags: row.get(11)?,
            script_content,
            interpreter,
            behavior,
        })
    })?;

    let mut list = Vec::new();
    for row in rows {
        list.push(row?);
    }
    drop(stmt);

    // honey: N+1, fine under ~1k rows; batch with a single IN query if it grows
    for item in &mut list {
        item.invocations = list_aliases(conn, &item.id).map_err(alias_err)?;
        item.display = display_for_aliases(&item.name, &item.invocations);
    }

    Ok(list)
}

/// Fuzzy-finder search over active triggers by name and trigger.
///
/// Returns a small list of summaries ordered by `usage_count` (most-used first),
/// then by most recently updated as a tie-breaker.
pub fn search_triggers(conn: &Connection, query: &str, limit: i64) -> Result<Vec<TriggerSummary>> {
    let pattern = format!("%{}%", query);

    let os_str = get_current_os_db_string();
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, description, usage_count
         FROM   triggers
         WHERE  is_deleted = 0
           AND  is_enabled = 1
           AND  (target_os = 'all' OR target_os = ?3)
           AND  (name    LIKE ?1
                 OR EXISTS (SELECT 1 FROM trigger_aliases al WHERE al.trigger_id = triggers.id AND al.invocation LIKE ?1))
         ORDER BY (target_os != 'all') DESC, usage_count DESC, updated_at DESC
        LIMIT  ?2",
    )?;

    let rows = stmt.query_map((pattern, limit, os_str), |row| {
        Ok(TriggerSummary {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            invocations: Vec::new(),
            display: String::new(),
            usage_count: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    drop(stmt);

    // honey: N+1, fine under ~1k rows; batch with a single IN query if it grows
    for item in &mut results {
        item.invocations = list_aliases(conn, &item.id).map_err(alias_err)?;
        item.display = display_for_aliases(&item.name, &item.invocations);
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shell::{ScriptBehavior, ScriptInterpreter};

    #[test]
    fn test_parse_json_variant_handles_double_quotes() {
        // Normal case
        let result: Option<ScriptInterpreter> = parse_json_variant(Some("python".to_string()));
        assert_eq!(result, Some(ScriptInterpreter::Python));

        // problematic double-quote case from SQLite
        let result: Option<ScriptInterpreter> = parse_json_variant(Some("\"python\"".to_string()));
        assert_eq!(result, Some(ScriptInterpreter::Python));

        // Behavior case
        let result: Option<ScriptBehavior> = parse_json_variant(Some("\"inline\"".to_string()));
        assert_eq!(result, Some(ScriptBehavior::Inline));

        // Mixed case (trimming matches any number of quotes at ends)
        let result: Option<ScriptInterpreter> =
            parse_json_variant(Some("\"\"bash\"\"".to_string()));
        assert_eq!(result, Some(ScriptInterpreter::Bash));
    }

    #[test]
    fn test_parse_json_variant_handles_none_and_invalid() {
        assert_eq!(parse_json_variant::<ScriptInterpreter>(None), None);
        assert_eq!(
            parse_json_variant::<ScriptInterpreter>(Some("invalid".to_string())),
            None
        );
    }
}
