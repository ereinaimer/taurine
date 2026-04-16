use std::path::PathBuf;

use inquire::{Password, PasswordDisplayMode};
use taurine_core::db::init;
use taurine_core::exchange::{
    ExchangePayload, crypto, encode_plaintext_payload, export_automations, serialize_payload,
};
use tracing::info;
use zeroize::Zeroize;

pub fn execute(path: PathBuf, no_encrypt: bool) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let payload = export_automations(&conn)?;
    let encoded = if no_encrypt {
        encode_plaintext_payload(&payload)?
    } else {
        let mut password = prompt_export_password()?;
        let result = encode_exchange_blob(&payload, false, Some(password.as_str()));
        password.zeroize();
        result?
    };

    std::fs::write(&path, encoded)?;

    info!(
        "Exported {} automation(s) to {}",
        payload.automations.len(),
        path.display()
    );

    Ok(())
}

fn encode_exchange_blob(
    payload: &ExchangePayload,
    no_encrypt: bool,
    password: Option<&str>,
) -> taurine_core::error::Result<Vec<u8>> {
    if no_encrypt {
        return encode_plaintext_payload(payload);
    }

    let password = password.ok_or_else(|| {
        taurine_core::error::Error::Config(
            "An encryption password is required for TAU1 exports".to_string(),
        )
    })?;

    let mut serialized = serialize_payload(payload)?;
    let result = crypto::encrypt(&serialized, password);
    serialized.zeroize();
    result
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
    use taurine_core::exchange::{ENCRYPTED_MAGIC_HEADER, PLAINTEXT_MAGIC_HEADER};

    fn sample_payload() -> ExchangePayload {
        ExchangePayload::new(vec![])
    }

    #[test]
    fn encode_exchange_blob_uses_taup_for_plaintext_exports() {
        let blob = encode_exchange_blob(&sample_payload(), true, None).unwrap();
        assert_eq!(&blob[..4], &PLAINTEXT_MAGIC_HEADER);
    }

    #[test]
    fn encode_exchange_blob_uses_tau1_for_encrypted_exports() {
        let blob = encode_exchange_blob(&sample_payload(), false, Some("hunter2")).unwrap();
        assert_eq!(&blob[..4], &ENCRYPTED_MAGIC_HEADER);
        assert!(
            !blob
                .windows(b"schema_version".len())
                .any(|window| window == b"schema_version"),
            "Encrypted export should be an opaque binary blob"
        );
    }
}
