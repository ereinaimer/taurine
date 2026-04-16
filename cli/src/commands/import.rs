use std::path::PathBuf;

use inquire::{Password, PasswordDisplayMode, Select};
use taurine_core::db::init;
use taurine_core::exchange::{
    AutomationExport, ExchangeFormat, ExchangePayload, ExistingAutomationConflict,
    ImportConflictAction, crypto, decode_plaintext_payload, deserialize_payload,
    detect_exchange_format, import_automations,
};
use tracing::info;
use zeroize::Zeroize;

use crate::ImportConflictCli;

pub fn execute(
    path: PathBuf,
    on_conflict: Option<ImportConflictCli>,
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
    let mut conn = init::setup()?;
    let imported = import_payload_transactionally(&mut conn, &payload, on_conflict)?;

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
) -> taurine_core::error::Result<usize> {
    let mut remembered_choice = None;
    let tx = conn.transaction()?;
    let result = import_automations(&tx, payload, |incoming, existing| {
        resolve_conflict_action(incoming, existing, on_conflict, &mut remembered_choice)
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

fn decode_exchange_blob(
    bytes: &[u8],
    password: Option<&str>,
) -> taurine_core::error::Result<ExchangePayload> {
    match detect_exchange_format(bytes)? {
        ExchangeFormat::Plaintext => decode_plaintext_payload(bytes),
        ExchangeFormat::Encrypted => {
            let password = password.ok_or_else(|| {
                taurine_core::error::Error::Config(
                    "A password is required to import TAU1 exchange files".to_string(),
                )
            })?;
            let mut plaintext = crypto::decrypt(bytes, password)?;
            let payload = deserialize_payload(&plaintext);
            plaintext.zeroize();
            payload
        }
    }
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

    let prompt = format!(
        "Conflict for trigger '{}' on target_os '{}': local '{}' -> '{}' vs imported '{}' -> '{}'. How should Taurine proceed?",
        existing.trigger,
        existing.target_os,
        existing.name,
        existing.output,
        incoming.name,
        incoming.output
    );

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
            trigger: "gm".to_string(),
            output: "Imported output".to_string(),
            action_type: "text".to_string(),
            is_enabled: true,
            target_os: "all".to_string(),
            tags: vec![],
            script: None,
        }
    }

    fn sample_existing() -> ExistingAutomationConflict {
        ExistingAutomationConflict {
            id: "local-id".to_string(),
            name: "Local".to_string(),
            description: None,
            trigger: "gm".to_string(),
            output: "Local output".to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            is_enabled: true,
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
}
