use taurine_core::db::init;
use tracing::{info, warn};

pub fn execute(triggers: Vec<String>) -> taurine_core::error::Result<()> {
    use taurine_core::db::crud::delete_automations_by_triggers;

    let conn = init::setup()?;
    let removed_count = delete_automations_by_triggers(&conn, &triggers)?;

    if removed_count == 0 {
        let triggers_str = triggers.join(", ");
        warn!(
            "No active automation found for trigger(s): {}",
            triggers_str
        );
    } else {
        let triggers_str = triggers.join(", ");
        info!(
            "Removed {} automation(s) for trigger(s): {}",
            removed_count, triggers_str
        );
        taurine_core::rpc::notify_daemon_reload();
    }

    Ok(())
}
