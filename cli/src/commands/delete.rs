use taurine_core::db::init;
use tracing::{info, warn};

pub fn execute(triggers: Vec<String>, tag: Option<String>) -> taurine_core::error::Result<()> {
    use taurine_core::db::crud::{delete_automations_by_tag, delete_automations_by_triggers};

    let conn = init::setup()?;
    let removed_count = if let Some(ref t) = tag {
        delete_automations_by_tag(&conn, t)?
    } else {
        delete_automations_by_triggers(&conn, &triggers)?
    };

    if removed_count == 0 {
        if let Some(ref t) = tag {
            warn!("No active automation found with tag: {}", t);
        } else {
            let triggers_str = triggers.join(", ");
            warn!(
                "No active automation found for trigger(s): {}",
                triggers_str
            );
        }
    } else {
        if let Some(ref t) = tag {
            info!("Removed {} automation(s) with tag: {}", removed_count, t);
        } else {
            let triggers_str = triggers.join(", ");
            info!(
                "Removed {} automation(s) for trigger(s): {}",
                removed_count, triggers_str
            );
        }
        taurine_core::rpc::notify_daemon_reload();
    }

    Ok(())
}
