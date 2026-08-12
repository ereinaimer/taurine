use std::path::PathBuf;

use taurine_core::db::init;
use taurine_core::exchange::{
    ExportOptions, encode_exchange_blob, export_triggers, resolve_export_path,
};
use zeroize::Zeroize;

pub fn execute(
    path: Option<PathBuf>,
    plain: bool,
    settings: bool,
    stats: bool,
    sensitive: bool,
    yes: bool,
) -> taurine_core::error::Result<()> {
    let (path, plain, settings, stats, sensitive, password) = if !yes {
        match taurine_tui::run_export_overlay()? {
            Some(result) => (
                Some(result.path),
                !result.encrypt,
                result.include_settings,
                result.include_stats,
                result.include_sensitive_settings,
                result.password,
            ),
            None => return Ok(()),
        }
    } else {
        if sensitive && plain {
            return Err(taurine_core::error::Error::Config(
                    "Cannot export sensitive settings without encryption. Remove the --plain / -p flag to securely export sensitive data.".to_string(),
                ));
        }
        (path, plain, settings, stats, sensitive, None)
    };

    let path = resolve_export_path(path)?;
    let conn = init::setup()?;
    let payload = export_triggers(
        &conn,
        ExportOptions {
            include_settings: settings,
            include_stats: stats,
            include_sensitive_settings: sensitive,
        },
    )?;
    let encoded = if plain {
        encode_exchange_blob(&payload, false, None)?
    } else if yes {
        return Err(taurine_core::error::Error::Config(
            "Encryption password is required. Use --plain for unencrypted export.".into(),
        ));
    } else {
        let mut password =
            match password {
                Some(pw) => pw,
                None => taurine_tui::prompt_password("Encryption password:", true)?.ok_or_else(
                    || taurine_core::error::Error::Config("Export cancelled.".to_string()),
                )?,
            };
        if let Err(err) = taurine_core::exchange::validate_export_password(&password) {
            password.zeroize();
            return Err(err);
        }
        let result = encode_exchange_blob(&payload, true, Some(password.as_str()));
        password.zeroize();
        result?
    };

    taurine_core::exchange::write_export_file(&path, &encoded)?;

    let mut parts = Vec::new();
    if settings {
        if sensitive {
            parts.push("sensitive settings");
        } else {
            parts.push("settings");
        }
    }
    if stats {
        parts.push("stats");
    }

    let details = if parts.is_empty() {
        "".to_string()
    } else {
        format!(" with {}", parts.join(" and "))
    };

    let trigger_word = if payload.triggers.len() == 1 {
        "trigger"
    } else {
        "triggers"
    };

    println!(
        "Exported {} {}{} to {}",
        payload.triggers.len(),
        trigger_word,
        details,
        path.display()
    );

    Ok(())
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
        let blob = encode_exchange_blob(&sample_payload(), true, Some("hunter222")).unwrap();
        assert_eq!(&blob[..4], &ENCRYPTED_MAGIC_HEADER);
        assert!(
            !blob
                .windows(b"schema_version".len())
                .any(|window| window == b"schema_version"),
            "Encrypted export should be an opaque binary blob"
        );
    }
}
