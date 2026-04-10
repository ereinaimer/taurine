use taurine_core::db::crud::AddOutcome;
use taurine_core::db::init;
use tracing::info;

pub fn execute(trigger: String, output: String) -> taurine_core::error::Result<()> {
    use taurine_core::db::crud::add_automation_by_trigger;

    let conn = init::setup()?;
    let outcome = add_automation_by_trigger(&conn, &trigger, &output)?;

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
