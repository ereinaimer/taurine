use std::path::PathBuf;

use nite_core::db::init;
use nite_core::exchange::{encode_exchange_blob, export_triggers, resolve_export_path};
use zeroize::Zeroize;

pub fn execute(path: Option<PathBuf>, yes: bool) -> nite_core::error::Result<()> {
    let (path, password) = if !yes {
        match nite_tui::run_export_overlay()? {
            Some(result) => (Some(result.path), result.password),
            None => return Ok(()),
        }
    } else {
        (path, None)
    };

    let path = resolve_export_path(path)?;
    let conn = init::setup()?;
    let payload = export_triggers(&conn)?;
    let mut password = password;
    let encoded = encode_exchange_blob(&payload, password.as_deref());
    if let Some(ref mut pw) = password {
        pw.zeroize();
    }
    let encoded = encoded.map_err(|err| {
        let diag = nite_core::diagnostic::Diagnostic::problem(format!(
            "Cannot prepare export data: {err}"
        ))
        .help("Try exporting again:")
        .example("nite export ./backup.nx -y");
        nite_core::error::Error::Config(diag.render())
    })?;

    nite_core::exchange::write_export_file(&path, &encoded).map_err(|err| {
        let diag = nite_core::diagnostic::Diagnostic::problem(format!(
            "Cannot write export file to {}: {err}",
            path.display()
        ))
        .help("Check the directory exists and is writable, then try again:")
        .example("nite export ./backup.nx -y");
        nite_core::error::Error::Config(diag.render())
    })?;

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
    use nite_core::exchange::{ExchangePayload, NITE_MAGIC};

    fn sample_payload() -> ExchangePayload {
        ExchangePayload::new(vec![])
    }

    #[test]
    fn encode_exchange_blob_is_always_encrypted() {
        for password in [None, Some("hunter222")] {
            let blob = encode_exchange_blob(&sample_payload(), password).unwrap();
            assert_eq!(&blob[..4], &NITE_MAGIC);
            assert!(
                !blob
                    .windows(b"schema_version".len())
                    .any(|window| window == b"schema_version"),
                "Export should be an opaque binary blob"
            );
        }
    }

    #[test]
    fn test_export_non_interactive_passwordless_succeeds() {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        crate::commands::test_keyring::use_shared_test_keyring();
        let dir = tempfile::tempdir().expect("temp dir");
        // SAFETY: Test runs under TEST_LOCK and temporary dir is cleaned up.
        unsafe { std::env::set_var("NITE_DATA_DIR", dir.path()) };
        let target = dir.path().join("backup.nx");

        let result = execute(Some(target.clone()), true);

        // SAFETY: Test runs under TEST_LOCK to restore process environment safely.
        unsafe { std::env::remove_var("NITE_DATA_DIR") };

        result.unwrap();
        let bytes = std::fs::read(&target).unwrap();
        assert_eq!(&bytes[..4], &NITE_MAGIC);
    }

    #[test]
    fn test_export_write_failure_diagnostic() {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        crate::commands::test_keyring::use_shared_test_keyring();
        let dir = tempfile::tempdir().expect("temp dir");
        // SAFETY: Test runs under TEST_LOCK and temporary dir is cleaned up.
        unsafe { std::env::set_var("NITE_DATA_DIR", dir.path()) };
        let target = dir.path().join("no-such-dir").join("backup.nx");

        let result = execute(Some(target), true);

        // SAFETY: Test runs under TEST_LOCK to restore process environment safely.
        unsafe { std::env::remove_var("NITE_DATA_DIR") };

        let err = result.unwrap_err().to_string();
        assert!(err.contains("Cannot write export file"), "Error was: {err}");
        assert!(
            err.contains("nite export ./backup.nx -y"),
            "Error was: {err}"
        );
        assert!(!err.contains('`'), "Must not contain backticks: {err}");
        assert!(!err.contains('\''), "Must not contain single quotes: {err}");
    }
}
