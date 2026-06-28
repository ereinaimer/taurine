use std::path::PathBuf;

use inquire::{Confirm, Password, PasswordDisplayMode, Select};
use taurine_core::db::crud::TriggerType;
use taurine_core::db::init;
use taurine_core::exchange::{
    AutomationExport, ExchangeFormat, ExchangePayload, ExistingAutomationConflict,
    ImportConflictAction, ImportMetricsMode, ImportOptions,
    decode_exchange_blob as decode_exchange_blob_core, detect_exchange_format,
    import_payload_transactionally as import_payload_transactionally_core,
    payload_contains_run_variables as payload_contains_run_variables_core,
};
use tracing::info;
use zeroize::Zeroize;

use crate::{ImportConflictCli, ImportMetricsCli};

pub fn execute(
    path: PathBuf,
    on_conflict: Option<ImportConflictCli>,
    include_settings: bool,
    include_metrics: Option<ImportMetricsCli>,
) -> taurine_core::error::Result<()> {
    let bytes = std::fs::read(&path)?;
    let format = detect_exchange_format(&bytes)?;
    let payload = match format {
        ExchangeFormat::Plaintext => decode_exchange_blob(&bytes, None)?,
        ExchangeFormat::Encrypted => {
            let mut password = prompt_import_password()?;
            let result = decode_exchange_blob(&bytes, Some(password.as_str()));
            password.zeroize();
            result?
        }
    };
    confirm_run_variable_import(&payload)?;
    let mut conn = init::setup()?;
    let imported = import_payload_transactionally(
        &mut conn,
        &payload,
        on_conflict,
        ImportOptions {
            include_settings,
            metrics_mode: map_import_metrics_mode(include_metrics),
        },
    )?;

    if imported > 0 {
        taurine_core::rpc::notify_daemon_reload();
    }

    info!(
        "Imported {} automation(s) from {}",
        imported,
        path.display()
    );
    Ok(())
}

fn import_payload_transactionally(
    conn: &mut rusqlite::Connection,
    payload: &ExchangePayload,
    on_conflict: Option<ImportConflictCli>,
    options: ImportOptions,
) -> taurine_core::error::Result<usize> {
    let mut remembered_choice = None;
    import_payload_transactionally_core(conn, payload, options, |incoming, existing| {
        resolve_conflict_action(incoming, existing, on_conflict, &mut remembered_choice)
    })
}

fn map_import_metrics_mode(include_metrics: Option<ImportMetricsCli>) -> ImportMetricsMode {
    match include_metrics.unwrap_or(ImportMetricsCli::Ignore) {
        ImportMetricsCli::Ignore => ImportMetricsMode::Ignore,
        ImportMetricsCli::Merge => ImportMetricsMode::Merge,
        ImportMetricsCli::Overwrite => ImportMetricsMode::Overwrite,
    }
}

fn decode_exchange_blob(
    bytes: &[u8],
    password: Option<&str>,
) -> taurine_core::error::Result<ExchangePayload> {
    decode_exchange_blob_core(bytes, password)
}

fn confirm_run_variable_import(payload: &ExchangePayload) -> taurine_core::error::Result<()> {
    if !payload_contains_run_variables(payload) {
        return Ok(());
    }

    let confirmed = Confirm::new(
        "**CAUTION**: This import contains [execute] variables that execute shell commands.\n\
Untrusted scripts can damage your system. Continue? [y/N]",
    )
    .with_default(false)
    .prompt()
    .map_err(|e| {
        taurine_core::error::Error::Service(format!(
            "Failed to read execute variable import confirmation: {e}"
        ))
    })?;

    if confirmed {
        Ok(())
    } else {
        Err(taurine_core::error::Error::Config(
            "Import cancelled because it contains [execute] variables".to_string(),
        ))
    }
}

fn payload_contains_run_variables(payload: &ExchangePayload) -> bool {
    payload_contains_run_variables_core(payload)
}

fn resolve_conflict_action(
    incoming: &AutomationExport,
    existing: &ExistingAutomationConflict,
    on_conflict: Option<ImportConflictCli>,
    remembered_choice: &mut Option<RememberedConflictChoice>,
) -> taurine_core::error::Result<ImportConflictAction> {
    match on_conflict {
        Some(ImportConflictCli::Skip) => Ok(ImportConflictAction::Skip),
        Some(ImportConflictCli::Overwrite) => Ok(ImportConflictAction::Overwrite),
        Some(ImportConflictCli::Prompt) | None => {
            prompt_conflict_action(incoming, existing, remembered_choice)
        }
    }
}

fn prompt_conflict_action(
    incoming: &AutomationExport,
    existing: &ExistingAutomationConflict,
    remembered_choice: &mut Option<RememberedConflictChoice>,
) -> taurine_core::error::Result<ImportConflictAction> {
    if let Some(choice) = remembered_choice {
        return Ok(choice.to_action());
    }

    let prompt = format_conflict_prompt(incoming, existing);

    let selection = Select::new(
        &prompt,
        vec![
            ConflictPromptOption::Yes,
            ConflictPromptOption::No,
            ConflictPromptOption::All,
            ConflictPromptOption::SkipAll,
        ],
    )
    .prompt()
    .map_err(|e| {
        taurine_core::error::Error::Service(format!(
            "Failed to resolve import conflict interactively: {e}"
        ))
    })?;

    match selection {
        ConflictPromptOption::Yes => Ok(ImportConflictAction::Overwrite),
        ConflictPromptOption::No => Ok(ImportConflictAction::Skip),
        ConflictPromptOption::All => {
            *remembered_choice = Some(RememberedConflictChoice::OverwriteAll);
            Ok(ImportConflictAction::Overwrite)
        }
        ConflictPromptOption::SkipAll => {
            *remembered_choice = Some(RememberedConflictChoice::SkipAll);
            Ok(ImportConflictAction::Skip)
        }
    }
}

fn format_conflict_prompt(
    incoming: &AutomationExport,
    existing: &ExistingAutomationConflict,
) -> String {
    format!(
        "Conflict for {} trigger '{}' on target_os '{}': local '{}' -> '{}' vs imported '{}' -> '{}'. How should Taurine proceed?",
        format_trigger_kind(existing.trigger_type),
        existing.trigger,
        existing.target_os,
        existing.name,
        existing.output,
        incoming.name,
        incoming.output
    )
}

fn format_trigger_kind(trigger_type: TriggerType) -> &'static str {
    match trigger_type {
        TriggerType::Word => "word",
        TriggerType::Hotkey => "hotkey",
    }
}

fn prompt_import_password() -> taurine_core::error::Result<String> {
    Password::new("Decryption password:")
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|e| {
            taurine_core::error::Error::Service(format!("Failed to read decryption password: {e}"))
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RememberedConflictChoice {
    OverwriteAll,
    SkipAll,
}

impl RememberedConflictChoice {
    fn to_action(self) -> ImportConflictAction {
        match self {
            Self::OverwriteAll => ImportConflictAction::Overwrite,
            Self::SkipAll => ImportConflictAction::Skip,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConflictPromptOption {
    Yes,
    No,
    All,
    SkipAll,
}

impl std::fmt::Display for ConflictPromptOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Yes => "[y]es - overwrite this local automation",
            Self::No => "[n]o - skip this imported automation",
            Self::All => "[A]ll - overwrite all remaining conflicts",
            Self::SkipAll => "[S]kip all - keep all remaining local conflicts",
        };
        write!(f, "{label}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::exchange::{ExchangePayload, crypto, serialize_payload};

    fn sample_payload() -> ExchangePayload {
        ExchangePayload::new(vec![])
    }

    fn sample_automation() -> AutomationExport {
        AutomationExport {
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
        }
    }

    fn sample_existing() -> ExistingAutomationConflict {
        ExistingAutomationConflict {
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
        let blob = crypto::encrypt(&json, "hunter2").unwrap();

        let err = decode_exchange_blob(&blob, None).unwrap_err();
        assert!(err.to_string().contains("password is required"));
    }

    #[test]
    fn decode_exchange_blob_decrypts_tau1_when_password_is_provided() {
        let json = serialize_payload(&sample_payload()).unwrap();
        let blob = crypto::encrypt(&json, "hunter2").unwrap();

        let payload = decode_exchange_blob(&blob, Some("hunter2")).unwrap();
        assert_eq!(payload, sample_payload());
    }

    #[test]
    fn resolve_conflict_action_honors_skip_policy_without_prompting() {
        let action = resolve_conflict_action(
            &sample_automation(),
            &sample_existing(),
            Some(ImportConflictCli::Skip),
            &mut None,
        )
        .unwrap();

        assert_eq!(action, ImportConflictAction::Skip);
    }

    #[test]
    fn remembered_conflict_choice_applies_without_prompting() {
        let mut remembered = Some(RememberedConflictChoice::OverwriteAll);
        let action =
            prompt_conflict_action(&sample_automation(), &sample_existing(), &mut remembered)
                .unwrap();

        assert_eq!(action, ImportConflictAction::Overwrite);
    }

    #[test]
    fn map_import_metrics_mode_defaults_to_ignore() {
        assert_eq!(map_import_metrics_mode(None), ImportMetricsMode::Ignore);
        assert_eq!(
            map_import_metrics_mode(Some(ImportMetricsCli::Merge)),
            ImportMetricsMode::Merge
        );
    }

    #[test]
    fn detects_run_variables_case_insensitively() {
        let mut payload = sample_payload();
        payload.automations.push(AutomationExport {
            trigger_type: TriggerType::Word,
            output: "before [EXECUTE.bash(echo hi)] after".to_string(),
            ..sample_automation()
        });

        assert!(payload_contains_run_variables(&payload));
    }

    #[test]
    fn detects_run_variables_in_script_content() {
        let mut payload = sample_payload();
        let mut automation = sample_automation();
        automation.script = Some(taurine_core::exchange::ScriptExport {
            interpreter: taurine_core::engine::shell::ScriptInterpreter::Bash,
            behavior: taurine_core::engine::shell::ScriptBehavior::Inline,
            content: "echo [execute.bash(echo nested)]".to_string(),
        });
        payload.automations.push(automation);

        assert!(payload_contains_run_variables(&payload));
    }

    #[test]
    fn ignores_content_without_run_variables() {
        let mut payload = sample_payload();
        payload.automations.push(sample_automation());

        assert!(!payload_contains_run_variables(&payload));
    }

    #[test]
    fn conflict_prompt_mentions_trigger_type_trigger_and_target_os() {
        let mut incoming = sample_automation();
        incoming.trigger_type = TriggerType::Hotkey;
        incoming.trigger = "ctrl+shift+g".to_string();
        incoming.output = "git push".to_string();

        let mut existing = sample_existing();
        existing.trigger_type = TriggerType::Hotkey;
        existing.trigger = "ctrl+shift+g".to_string();
        existing.target_os = "all".to_string();
        existing.output = "git status".to_string();

        let prompt = format_conflict_prompt(&incoming, &existing);

        assert!(prompt.contains("hotkey trigger 'ctrl+shift+g'"));
        assert!(prompt.contains("target_os 'all'"));
    }
}
