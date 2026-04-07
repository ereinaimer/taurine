use taurine_core::db::init;
use tracing::info;

pub fn execute(trigger: String, output: String) -> Result<(), Box<dyn std::error::Error>> {
    use taurine_core::db::crud::add_automation_by_trigger;

    let conn = init::setup()?;
    add_automation_by_trigger(&conn, &trigger, &output)?;

    info!("Added automation: {} -> {}", trigger, output);
    Ok(())
}
