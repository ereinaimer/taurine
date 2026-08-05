use tracing::warn;

use super::apply_setting_input_with_manager;
use crate::{db::init, error::Result, settings::SettingsManager};

pub fn apply_setting_input(key: &str, value: Option<&str>) -> Result<()> {
    let conn = init::setup()?;
    let manager = SettingsManager::new(&conn);
    let outcome = apply_setting_input_with_manager(&manager, key, value)?;

    if let Some(enabled) = outcome.sync_boot
        && let Err(error) = crate::service::sync_boot(enabled)
    {
        warn!(error = %error, "Failed to synchronize OS startup hook");
    }

    crate::rpc::notify_daemon_reload();
    Ok(())
}
