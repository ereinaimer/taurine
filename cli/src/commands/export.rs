use std::path::PathBuf;

use taurine_core::db::init;
use taurine_core::exchange::{encode_plaintext_payload, export_automations};
use tracing::info;

pub fn execute(path: PathBuf, _no_encrypt: bool) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let payload = export_automations(&conn)?;
    let encoded = encode_plaintext_payload(&payload)?;

    std::fs::write(&path, encoded)?;

    info!(
        "Exported {} automation(s) to {}",
        payload.automations.len(),
        path.display()
    );

    Ok(())
}
