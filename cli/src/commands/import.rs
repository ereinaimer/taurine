use std::path::PathBuf;

use taurine_core::db::init;
use taurine_core::exchange::{
    ExchangeFormat, ExchangePayload, ExistingTriggerConflict, ImportConflictAction, ImportOptions,
    ImportStatsMode, TriggerExport, decode_exchange_blob as decode_exchange_blob_core,
    detect_exchange_format, import_payload_transactionally as import_payload_transactionally_core,
};
use zeroize::Zeroize;

use crate::args::{ImportConflictCli, ImportStatsCli};

pub fn execute(
    path: Option<PathBuf>,
    conflict: Option<ImportConflictCli>,
    settings: bool,
    stats: Option<ImportStatsCli>,
    sensitive: bool,
    yes: bool,
) -> taurine_core::error::Result<()> {
    let (
        resolved_path,
        resolved_settings,
        resolved_sensitive,
        resolved_stats,
        resolved_conflict,
        overlay_password,
    ) = if !yes {
        let path_str = path.as_ref().map(|p| p.to_string_lossy().into_owned());
        match taurine_tui::run_import_overlay(path_str.as_deref())? {
            Some(result) => {
                let stats_cli = match result.stats_mode {
                    ImportStatsMode::Ignore => ImportStatsCli::Ignore,
                    ImportStatsMode::Merge => ImportStatsCli::Merge,
                    ImportStatsMode::Overwrite => ImportStatsCli::Overwrite,
                };
                let conflict_cli = match result.conflict_mode {
                    taurine_tui::LibraryImportConflictMode::Skip => ImportConflictCli::Skip,
                    taurine_tui::LibraryImportConflictMode::Overwrite => {
                        ImportConflictCli::Overwrite
                    }
                };
                (
                    result.path,
                    result.include_settings,
                    result.include_sensitive_settings,
                    Some(stats_cli),
                    Some(conflict_cli),
                    result.password,
                )
            }
            None => return Ok(()),
        }
    } else {
        let path = path.ok_or_else(|| {
            taurine_core::error::Error::Config(
                "a PATH is required for non-interactive import".into(),
            )
        })?;
        (path, settings, sensitive, stats, conflict, None)
    };

    let bytes = std::fs::read(&resolved_path)?;
    let format = detect_exchange_format(&bytes)?;
    let mut password = match format {
        ExchangeFormat::Encrypted => Some(match overlay_password {
            Some(pw) => pw,
            None => {
                return Err(taurine_core::error::Error::Config(
                    "File is encrypted. Enter the decryption password in the import form.".into(),
                ));
            }
        }),
        ExchangeFormat::Plaintext => overlay_password,
    };
    let payload = match format {
        ExchangeFormat::Plaintext => decode_exchange_blob(&bytes, None)?,
        ExchangeFormat::Encrypted => {
            let pw = password.as_deref().unwrap();
            decode_exchange_blob(&bytes, Some(pw))?
        }
    };
    if let Some(ref mut pw) = password {
        pw.zeroize();
    }
    drop(password);
    let mut conn = init::setup()?;
    let imported = import_payload_transactionally(
        &mut conn,
        &payload,
        resolved_conflict,
        ImportOptions {
            include_settings: resolved_settings,
            stats_mode: map_import_stats_mode(resolved_stats),
            include_sensitive_settings: resolved_sensitive,
        },
    )?;

    if imported > 0 {
        taurine_core::rpc::notify_daemon_reload();
    }

    let mut parts = Vec::new();
    if resolved_settings {
        if resolved_sensitive && payload.settings.is_some() {
            parts.push("sensitive settings");
        } else if payload.settings.is_some() {
            parts.push("settings");
        }
    }
    let stats_cli = resolved_stats;
    if stats_cli.is_some() && stats_cli != Some(ImportStatsCli::Ignore) && payload.stats.is_some() {
        parts.push("stats");
    }

    let details = if parts.is_empty() {
        "".to_string()
    } else {
        format!(" with {}", parts.join(" and "))
    };

    if imported == 0 {
        println!("No triggers were imported.");
    } else {
        let trigger_word = if imported == 1 { "trigger" } else { "triggers" };

        println!(
            "Imported {} {}{} from {}",
            imported,
            trigger_word,
            details,
            resolved_path.display()
        );
    }

    Ok(())
}

fn import_payload_transactionally(
    conn: &mut rusqlite::Connection,
    payload: &ExchangePayload,
    conflict: Option<ImportConflictCli>,
    options: ImportOptions,
) -> taurine_core::error::Result<usize> {
    let mut remembered_choice: Option<taurine_tui::RememberedConflictChoice> = None;
    import_payload_transactionally_core(conn, payload, options, |incoming, existing| {
        resolve_conflict_action(incoming, existing, conflict, &mut remembered_choice)
    })
}

fn map_import_stats_mode(include_stats: Option<ImportStatsCli>) -> ImportStatsMode {
    match include_stats.unwrap_or(ImportStatsCli::Ignore) {
        ImportStatsCli::Ignore => ImportStatsMode::Ignore,
        ImportStatsCli::Merge => ImportStatsMode::Merge,
        ImportStatsCli::Overwrite => ImportStatsMode::Overwrite,
    }
}

fn decode_exchange_blob(
    bytes: &[u8],
    password: Option<&str>,
) -> taurine_core::error::Result<ExchangePayload> {
    decode_exchange_blob_core(bytes, password)
}

fn resolve_conflict_action(
    incoming: &TriggerExport,
    existing: &ExistingTriggerConflict,
    conflict: Option<ImportConflictCli>,
    remembered_choice: &mut Option<taurine_tui::RememberedConflictChoice>,
) -> taurine_core::error::Result<ImportConflictAction> {
    match conflict {
        Some(ImportConflictCli::Skip) => Ok(ImportConflictAction::Skip),
        Some(ImportConflictCli::Overwrite) => Ok(ImportConflictAction::Overwrite),
        Some(ImportConflictCli::Prompt) | None => {
            prompt_conflict_action(incoming, existing, remembered_choice)
        }
    }
}

fn prompt_conflict_action(
    incoming: &TriggerExport,
    existing: &ExistingTriggerConflict,
    remembered_choice: &mut Option<taurine_tui::RememberedConflictChoice>,
) -> taurine_core::error::Result<ImportConflictAction> {
    taurine_tui::run_conflict_prompt(incoming, existing, remembered_choice)
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::db::crud::TriggerType;
    use taurine_core::exchange::{ExchangePayload, crypto, serialize_payload};

    fn sample_payload() -> ExchangePayload {
        ExchangePayload::new(vec![])
    }

    fn sample_trigger() -> TriggerExport {
        TriggerExport {
            name: "Imported".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "gm".to_string(),
            output: "Imported output".to_string(),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            usage_count: None,
            last_used_at: None,
            script: None,
            assets: Vec::new(),
        }
    }

    fn sample_existing() -> ExistingTriggerConflict {
        ExistingTriggerConflict {
            id: "local-id".to_string(),
            name: "Local".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "gm".to_string(),
            output: "Local output".to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            is_enabled: true,
            usage_count: 0,
            last_used_at: None,
        }
    }

    #[test]
    fn decode_exchange_blob_routes_plaintext_without_password() {
        let blob = taurine_core::exchange::encode_plaintext_payload(&sample_payload()).unwrap();
        let payload = decode_exchange_blob(&blob, None).unwrap();

        assert_eq!(payload, sample_payload());
    }

    #[test]
    fn decode_exchange_blob_requires_password_for_tau1() {
        let json = serialize_payload(&sample_payload()).unwrap();
        let blob = crypto::encrypt(&json, "hunter22").unwrap();

        let err = decode_exchange_blob(&blob, None).unwrap_err();
        assert!(err.to_string().contains("password required"));
    }

    #[test]
    fn decode_exchange_blob_decrypts_tau1_when_password_is_provided() {
        let json = serialize_payload(&sample_payload()).unwrap();
        let blob = crypto::encrypt(&json, "hunter22").unwrap();

        let payload = decode_exchange_blob(&blob, Some("hunter22")).unwrap();
        assert_eq!(payload, sample_payload());
    }

    #[test]
    fn resolve_conflict_action_honors_skip_policy_without_prompting() {
        let action = resolve_conflict_action(
            &sample_trigger(),
            &sample_existing(),
            Some(ImportConflictCli::Skip),
            &mut None,
        )
        .unwrap();

        assert_eq!(action, ImportConflictAction::Skip);
    }

    #[test]
    fn map_import_stats_mode_defaults_to_ignore() {
        assert_eq!(map_import_stats_mode(None), ImportStatsMode::Ignore);
        assert_eq!(
            map_import_stats_mode(Some(ImportStatsCli::Merge)),
            ImportStatsMode::Merge
        );
    }
}
