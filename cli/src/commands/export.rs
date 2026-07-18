use std::path::PathBuf;

use inquire::{Password, PasswordDisplayMode};
use taurine_core::db::init;
use taurine_core::exchange::{
    ExportOptions, encode_exchange_blob, export_automations, resolve_export_path,
};
use tracing::info;
use zeroize::Zeroize;

pub fn execute(
    path: Option<PathBuf>,
    plain: bool,
    settings: bool,
    metrics: bool,
    sensitive: bool,
) -> taurine_core::error::Result<()> {
    if sensitive && plain {
        return Err(taurine_core::error::Error::Config(
            "Cannot export sensitive settings without encryption. Remove the --plain / -p flag to securely export sensitive data.".to_string(),
        ));
    }

    let path = resolve_export_path(path)?;
    let conn = init::setup()?;
    let payload = export_automations(
        &conn,
        ExportOptions {
            include_settings: settings,
            include_metrics: metrics,
            include_sensitive_settings: sensitive,
        },
    )?;
    let encoded = if plain {
        encode_exchange_blob(&payload, false, None)?
    } else {
        let mut password = prompt_export_password()?;
        let result = encode_exchange_blob(&payload, true, Some(password.as_str()));
        password.zeroize();
        result?
    };

    std::fs::write(&path, encoded)?;

    let mut parts = Vec::new();
    if settings {
        if sensitive {
            parts.push("sensitive settings");
        } else {
            parts.push("settings");
        }
    }
    if metrics {
        parts.push("metrics");
    }

    let details = if parts.is_empty() {
        "".to_string()
    } else {
        format!(" with {}", parts.join(" and "))
    };

    let automation_word = if payload.automations.len() == 1 {
        "automation"
    } else {
        "automations"
    };

    info!(
        "Exported {} {}{} to {}",
        payload.automations.len(),
        automation_word,
        details,
        path.display()
    );

    Ok(())
}

fn prompt_export_password() -> taurine_core::error::Result<String> {
    Password::new("Encryption password:")
        .with_display_mode(PasswordDisplayMode::Masked)
        .with_custom_confirmation_message("Confirm encryption password:")
        .with_custom_confirmation_error_message("Passwords do not match.")
        .prompt()
        .map_err(|e| {
            taurine_core::error::Error::Service(format!("Failed to read encryption password: {e}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::exchange::{ENCRYPTED_MAGIC_HEADER, ExchangePayload, PLAINTEXT_MAGIC_HEADER};

    fn sample_payload() -> ExchangePayload {
        ExchangePayload::new(vec![])
    }

    #[test]
    fn encode_exchange_blob_uses_taup_for_plaintext_exports() {
        let blob = encode_exchange_blob(&sample_payload(), false, None).unwrap();
        assert_eq!(&blob[..4], &PLAINTEXT_MAGIC_HEADER);
    }

    #[test]
    fn encode_exchange_blob_uses_tau1_for_encrypted_exports() {
        let blob = encode_exchange_blob(&sample_payload(), true, Some("hunter2")).unwrap();
        assert_eq!(&blob[..4], &ENCRYPTED_MAGIC_HEADER);
        assert!(
            !blob
                .windows(b"schema_version".len())
                .any(|window| window == b"schema_version"),
            "Encrypted export should be an opaque binary blob"
        );
    }
}
