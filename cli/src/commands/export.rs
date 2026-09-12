use std::path::PathBuf;

use taurine_core::db::init;
use taurine_core::exchange::{encode_exchange_blob, export_triggers, resolve_export_path};
use zeroize::Zeroize;

pub fn execute(path: Option<PathBuf>, yes: bool) -> taurine_core::error::Result<()> {
    let (path, password) = if !yes {
        match taurine_tui::run_export_overlay()? {
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
    let encoded = encoded?;

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
    use taurine_core::exchange::{ExchangePayload, TAU_MAGIC};

    fn sample_payload() -> ExchangePayload {
        ExchangePayload::new(vec![])
    }

    #[test]
    fn encode_exchange_blob_is_always_encrypted() {
        for password in [None, Some("hunter222")] {
            let blob = encode_exchange_blob(&sample_payload(), password).unwrap();
            assert_eq!(&blob[..4], &TAU_MAGIC);
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
        let dir = tempfile::tempdir().expect("temp dir");
        // SAFETY: Test runs under TEST_LOCK and temporary dir is cleaned up.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", dir.path()) };
        let target = dir.path().join("backup.tau");

        let result = execute(Some(target.clone()), true);

        // SAFETY: Test runs under TEST_LOCK to restore process environment safely.
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };

        result.unwrap();
        let bytes = std::fs::read(&target).unwrap();
        assert_eq!(&bytes[..4], &TAU_MAGIC);
    }
}
