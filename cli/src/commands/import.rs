use std::path::PathBuf;

use taurine_core::db::init;
use taurine_core::exchange::{decode_plaintext_payload, import_automations};
use tracing::info;

pub fn execute(path: PathBuf) -> taurine_core::error::Result<()> {
    let bytes = std::fs::read(&path)?;
    let payload = decode_plaintext_payload(&bytes)?;
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
