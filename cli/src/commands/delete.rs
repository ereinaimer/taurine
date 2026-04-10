use taurine_core::db::init;
use tracing::{info, warn};

pub fn execute(trigger: String) -> taurine_core::error::Result<()> {
    use taurine_core::db::crud::delete_automation_by_trigger;

    let conn = init::setup()?;
    let removed_count = delete_automation_by_trigger(&conn, &trigger)?;

    if removed_count == 0 {
        warn!("No active automation found for trigger: {}", trigger);
    } else {
        info!(
            "Removed {} automation(s) for trigger: {}",
            removed_count, trigger
        );
        taurine_core::rpc::notify_daemon_reload();
    }

    Ok(())
}
