use crate::args::{AddArgs, AddSubcommand};
use crate::commands::validate::format_trigger_log;
use std::fs;
use std::path::PathBuf;
use taurine_core::db::crud::{
    TriggerType, audit_script_payload_tags, prepare_trigger_with_type, upsert_script,
};
use taurine_core::db::init;
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter, compress};
use unicode_normalization::UnicodeNormalization;

pub fn execute_args(args: AddArgs, json: bool) -> taurine_core::error::Result<()> {
    let AddSubcommand::Script {
        trigger,
        hotkey,
        regex,
        content,
        file,
        lang,
        mode,
        os,
        include_apps,
        exclude_apps,
        tag,
        name,
        description,
        auto_case,
    } = args
        .sub
        .expect("add dispatch routes to script only when subcommand is present");

    let trigger_type = TriggerType::from_cli_flags(hotkey, regex);
    let os = os
        .to_db_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| taurine_core::db::get_current_os_db_string().to_string());

    execute_with_trigger_type(
        trigger,
        trigger_type,
        content,
        file,
        lang.map(Into::into),
        mode.into(),
        os,
        include_apps,
        exclude_apps,
        tag,
        name,
        description,
        auto_case,
        json,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn execute(
    trigger: String,
    use_hotkey: bool,
    content: Option<String>,
    file_path: Option<PathBuf>,
    lang: Option<ScriptInterpreter>,
    mode: ScriptBehavior,
    os: String,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
) -> taurine_core::error::Result<()> {
    let trigger_type = if use_hotkey {
        TriggerType::Hotkey
    } else {
        TriggerType::Word
    };
    execute_with_trigger_type(
        trigger,
        trigger_type,
        content,
        file_path,
        lang,
        mode,
        os,
        include_apps,
        exclude_apps,
        None,
        None,
        None,
        false,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn execute_with_trigger_type(
    trigger: String,
    trigger_type: TriggerType,
    content: Option<String>,
    file_path: Option<PathBuf>,
    lang: Option<ScriptInterpreter>,
    mode: ScriptBehavior,
    os: String,
    include_apps: Option<String>,
    exclude_apps: Option<String>,
    tags: Option<Vec<String>>,
    name: Option<String>,
    description: Option<String>,
    auto_case: bool,
    json: bool,
) -> taurine_core::error::Result<()> {
    // 1. Resolve content and source description
    let (content, source_desc) = if let Some(ref path) = file_path {
        if !path.exists() {
            return Err(taurine_core::error::Error::NotFound(format!(
                "Script file not found: {}",
                path.display()
            )));
        }
        let text = fs::read_to_string(path).map_err(|e| {
            taurine_core::error::Error::Service(format!("Failed to read script file: {}", e))
        })?;
        (text, format!("File: {}", path.display()))
    } else if let Some(text) = content {
        (text, "CLI argument".to_string())
    } else {
        // unreachable due to clap constraints (required_unless_present)
        return Err(taurine_core::error::Error::Service(
            "Neither script file nor content provided".to_string(),
        ));
    };

    let trigger = if auto_case && !matches!(trigger_type, TriggerType::Regex) {
        trigger.to_lowercase()
    } else {
        trigger
    };

    audit_script_payload_tags(&content, trigger_type)?;

    if matches!(trigger_type, TriggerType::Regex) {
        regex::Regex::new(&trigger)
            .map_err(|e| taurine_core::Error::Config(format!("Invalid regular expression: {e}")))?;
    }

    let prepared = prepare_trigger_with_type(&trigger, trigger_type, &os)?;
    let stored_trigger = prepared.stored_trigger.nfc().collect::<String>();
    let trigger_name: String = name.unwrap_or_else(|| stored_trigger.clone());
    let description: Option<String> =
        description.or_else(|| Some(format!("Shell script ({})", source_desc)));

    if trigger_name.len() > 200 {
        return Err(taurine_core::error::Error::Config(
            "Name exceeds maximum length of 200 characters".into(),
        ));
    }
    if description.as_deref().is_some_and(|d| d.len() > 1000) {
        return Err(taurine_core::error::Error::Config(
            "Description exceeds maximum length of 1000 characters".into(),
        ));
    }

    let description = description.as_deref();

    // 2. Infer interpreter if not provided
    let lang = match lang {
        Some(i) => i,
        None => infer_interpreter(file_path.as_deref(), &content).ok_or_else(|| {
            taurine_core::error::Error::Service(
                "Could not infer script language. Please specify with --lang".to_string(),
            )
        })?,
    };

    let conn = init::setup()?;
    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
    if !settings.scripts_enabled {
        tracing::warn!(
            "Warning: Global script execution is currently disabled. This script trigger will not trigger until `scripts_enabled` is set to true."
        );
    }

    // Check for an existing active trigger with the same trigger tuple and app filters.
    let existing_record: Option<(String, i64, Option<i64>)> = conn
        .query_row(
            "SELECT id, usage_count, last_used_at
          FROM triggers
          WHERE trigger_type = ?1
            AND trigger = ?2
            AND target_os = ?3
            AND COALESCE(only_apps, '') = ?4
            AND COALESCE(except_apps, '') = ?5
            AND is_deleted = 0
          ORDER BY updated_at DESC
          LIMIT 1",
            rusqlite::params![
                prepared.trigger_type.as_db_str(),
                stored_trigger.as_str(),
                os.as_str(),
                include_apps.as_deref().unwrap_or(""),
                exclude_apps.as_deref().unwrap_or(""),
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok();

    let (id, usage_count, last_used_at, is_update) = match existing_record {
        Some((existing_id, existing_usage, existing_last_used)) => {
            (existing_id, existing_usage, existing_last_used, true)
        }
        None => {
            taurine_core::db::crud::validate_trigger_target_os_conflict(
                &conn,
                prepared.trigger_type,
                &stored_trigger,
                &os,
                include_apps.as_deref(),
                exclude_apps.as_deref(),
                None,
            )?;
            (uuid::Uuid::new_v4().to_string(), 0, None, false)
        }
    };

    let action = if is_update { "Updated" } else { "Added" };
    let log_msg = format_trigger_log(
        action,
        &stored_trigger,
        Some((mode, lang)),
        &os,
        include_apps.as_deref(),
        exclude_apps.as_deref(),
    );
    tracing::info!("{}", log_msg);
    if json {
        let status = if is_update { "updated" } else { "created" };
        println!(
            "{}",
            serde_json::json!({"status": status, "trigger": stored_trigger, "action_type": "script"})
        );
    }

    // 3. Compress the script
    let compressed = compress(&content)?;

    let tags_str = if let Some(ref t) = tags {
        let raw_json =
            serde_json::to_string(t).map_err(|e| taurine_core::Error::Config(e.to_string()))?;
        taurine_core::db::crud::normalize_tags(&raw_json)?
    } else if is_update {
        conn.query_row("SELECT tags FROM triggers WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .unwrap_or_else(|_| "[]".to_string())
    } else {
        "[]".to_string()
    };

    // Case conflict check
    if auto_case {
        let conflict_exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM triggers
                    WHERE trigger_type = ?1
                      AND LOWER(trigger) = LOWER(?2)
                      AND target_os = ?3
                      AND COALESCE(only_apps, '') = ?4
                      AND COALESCE(except_apps, '') = ?5
                      AND is_deleted = 0
                      AND id != ?6
                 )",
                [
                    prepared.trigger_type.as_db_str(),
                    &stored_trigger,
                    &os,
                    include_apps.as_deref().unwrap_or(""),
                    exclude_apps.as_deref().unwrap_or(""),
                    &id,
                ],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if conflict_exists {
            return Err(taurine_core::Error::Config(format!(
                "Conflict: '{}' already exists (case-insensitive match)",
                stored_trigger
            )));
        }
    } else {
        let conflict_exists: bool = conn
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM triggers
                    WHERE trigger_type = ?1
                      AND LOWER(trigger) = LOWER(?2)
                      AND target_os = ?3
                      AND COALESCE(only_apps, '') = ?4
                      AND COALESCE(except_apps, '') = ?5
                      AND auto_case = 1
                      AND is_deleted = 0
                      AND id != ?6
                 )",
                [
                    prepared.trigger_type.as_db_str(),
                    &stored_trigger,
                    &os,
                    include_apps.as_deref().unwrap_or(""),
                    exclude_apps.as_deref().unwrap_or(""),
                    &id,
                ],
                |r| r.get(0),
            )
            .unwrap_or(false);
        if conflict_exists {
            return Err(taurine_core::Error::Config(format!(
                "Conflict: '{}' already exists (case-propagating, case-insensitive)",
                stored_trigger
            )));
        }
    }

    // 4. Upsert trigger row (type = "script")
    match prepared.trigger_type {
        TriggerType::Word => {
            taurine_core::db::crud::upsert_trigger_with_type_and_case(
                &conn,
                &id,
                &trigger_name,
                description,
                TriggerType::Word,
                &stored_trigger,
                &format!("[Script: {}]", lang_to_str(lang)),
                "script",
                &os,
                &tags_str,
                usage_count,
                last_used_at,
                auto_case,
            )?;
        }
        TriggerType::Hotkey => {
            taurine_core::db::crud::upsert_trigger_with_type_and_case(
                &conn,
                &id,
                &trigger_name,
                description,
                TriggerType::Hotkey,
                &stored_trigger,
                &format!("[Script: {}]", lang_to_str(lang)),
                "script",
                &os,
                &tags_str,
                usage_count,
                last_used_at,
                auto_case,
            )?;
        }
        TriggerType::Regex => {
            taurine_core::db::crud::upsert_trigger_with_type_and_case(
                &conn,
                &id,
                &trigger_name,
                description,
                TriggerType::Regex,
                &stored_trigger,
                &format!("[Script: {}]", lang_to_str(lang)),
                "script",
                &os,
                &tags_str,
                usage_count,
                last_used_at,
                auto_case,
            )?;
        }
    }

    // 5. Upsert script attachment
    upsert_script(&conn, &id, lang, mode, &compressed)?;

    taurine_core::db::crud::update_trigger_app_filters(&conn, &id, include_apps, exclude_apps)?;

    taurine_core::rpc::notify_daemon_reload();

    Ok(())
}

pub(crate) fn infer_interpreter(
    path: Option<&std::path::Path>,
    content: &str,
) -> Option<ScriptInterpreter> {
    taurine_core::engine::shell::infer_interpreter(path, content)
}

fn lang_to_str(i: ScriptInterpreter) -> &'static str {
    match i {
        ScriptInterpreter::Bash => "bash",
        ScriptInterpreter::PowerShell => "powershell",
        ScriptInterpreter::Python => "python",
        ScriptInterpreter::Node => "node",
        ScriptInterpreter::Cmd => "cmd",
    }
}
