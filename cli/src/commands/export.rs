use std::path::PathBuf;

use taurine_core::db::init;
use taurine_core::exchange::{encode_exchange_blob, export_triggers, resolve_export_path};
use zeroize::Zeroize;

pub fn execute(path: Option<PathBuf>, plain: bool, yes: bool) -> taurine_core::error::Result<()> {
    let (path, plain, password) = if !yes {
        match taurine_tui::run_export_overlay()? {
            Some(result) => (Some(result.path), !result.encrypt, result.password),
            None => return Ok(()),
        }
    } else {
        (path, plain, None)
    };

    let path = resolve_export_path(path)?;
    let conn = init::setup()?;
    let payload = export_triggers(&conn)?;
    let encoded = if plain {
        encode_exchange_blob(&payload, false, None)?
    } else if yes {
        let diag = taurine_core::diagnostic::Diagnostic::problem(
            "Encryption password is required for non-interactive export",
        )
        .help("Use --plain to export unencrypted triggers, or run without -y for interactive password prompt:")
        .example("taurine export --plain -y")
        .example("taurine export ./backup.tau --plain -y");
        return Err(taurine_core::error::Error::Config(diag.render()));
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

    let trigger_word = if payload.triggers.len() == 1 {
        "trigger"
    } else {
        "triggers"
    };

    println!(
        "Exported {} {} to {}",
        payload.triggers.len(),
        trigger_word,
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

    #[test]
    fn test_export_non_interactive_missing_password_diagnostic() {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        let db_path =
            std::env::temp_dir().join(format!("taurine-cli-export-{}.db", uuid::Uuid::new_v4()));
        // SAFETY: Test runs under TEST_LOCK and temporary path is cleaned up.
        unsafe { std::env::set_var("TAURINE_DB_PATH", db_path.to_str().unwrap()) };

        let result = execute(None, false, true);

        // SAFETY: Test runs under TEST_LOCK to restore process environment safely.
        unsafe { std::env::remove_var("TAURINE_DB_PATH") };
        let _ = std::fs::remove_file(&db_path);

        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Encryption password is required for non-interactive export"),
            "Error was: {err}"
        );
        assert!(err.contains("--plain"), "Error was: {err}");
        assert!(!err.contains('`'), "Must not contain backticks: {err}");
        assert!(!err.contains('\''), "Must not contain single quotes: {err}");
    }
}
