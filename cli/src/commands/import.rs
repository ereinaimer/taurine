use std::path::PathBuf;

use inquire::{Password, PasswordDisplayMode};
use taurine_core::db::init;
use taurine_core::exchange::{
    ExchangeFormat, ExchangePayload, crypto, decode_plaintext_payload, deserialize_payload,
    detect_exchange_format, import_automations,
};
use tracing::info;
use zeroize::Zeroize;

pub fn execute(path: PathBuf) -> taurine_core::error::Result<()> {
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
    let conn = init::setup()?;
    let imported = import_automations(&conn, &payload)?;

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

fn prompt_import_password() -> taurine_core::error::Result<String> {
    Password::new("Decryption password:")
        .with_display_mode(PasswordDisplayMode::Masked)
        .without_confirmation()
        .prompt()
        .map_err(|e| {
            taurine_core::error::Error::Service(format!("Failed to read decryption password: {e}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::exchange::{ExchangePayload, crypto, serialize_payload};

    fn sample_payload() -> ExchangePayload {
        ExchangePayload::new(vec![])
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
}
