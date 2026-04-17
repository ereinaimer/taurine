use taurine_core::db::crud::AddOutcome;
use taurine_core::db::init;
use tracing::info;

pub fn execute(trigger: String, output: String, os: String) -> taurine_core::error::Result<()> {
    use crate::commands::validate::audit_payload_tags;
    use taurine_core::db::crud::add_automation_by_trigger;
    use taurine_core::engine::variables::system::validate_output;

    audit_payload_tags(&output)?;

    // Validate the snippet output for potential issues (cursors, conflicts, etc.)
    // Warnings are printed to the console via tracing::warn!
    validate_output(&output, Some(&trigger));

    let conn = init::setup()?;
    let outcome = add_automation_by_trigger(&conn, &trigger, &output, &os)?;

    match outcome {
        AddOutcome::Created => {
            info!("Added automation: {} -> {}", trigger, output);
            taurine_core::rpc::notify_daemon_reload();
        }
        AddOutcome::AlreadyExists => {
            info!("Automation already exists: {} -> {}", trigger, output)
        }
        AddOutcome::Updated => {
            info!("Updated automation: {} -> {}", trigger, output);
            taurine_core::rpc::notify_daemon_reload();
        }
    }

    Ok(())
}
