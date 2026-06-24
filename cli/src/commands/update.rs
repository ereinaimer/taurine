use axoupdater::AxoUpdater;
use taurine_core::error::{Error, Result};
use tracing::{error, info, warn};

pub fn execute() -> Result<()> {
    info!("Checking for updates...");

    let mut updater = AxoUpdater::new_for("taurine");

    let receipt = match updater.load_receipt() {
        Ok(u) => u,
        Err(_) => {
            warn!(
                "No install receipt found. Please use the official installer scripts or manually download the latest ZIP."
            );
            return Ok(());
        }
    };

    if let Ok(false) = receipt.check_receipt_is_for_this_executable() {
        warn!(
            "This executable was not installed via the official installer, so it cannot be updated automatically."
        );
        return Ok(());
    }

    let needs_update = match receipt.is_update_needed_sync() {
        Ok(needed) => needed,
        Err(e) => {
            error!(error=%e, "Failed to check for updates");
            return Err(Error::Engine(e.to_string()));
        }
    };

    if !needs_update {
        info!("Taurine is already up to date.");
        return Ok(());
    }

    info!("Update found! Initiating update sequence...");

    // Stop the daemon before updating to release file locks on Windows
    if let Err(e) = taurine_core::service::down() {
        warn!(error=%e, "Failed to stop daemon prior to update, continuing anyway...");
    }

    match receipt.run_sync() {
        Ok(Some(_)) => {
            info!("Update installed successfully!");

            // Restart the daemon after successful update
            let start_on_boot = {
                match taurine_core::db::init::setup() {
                    Ok(conn) => {
                        let settings_manager = taurine_core::settings::SettingsManager::new(&conn);
                        settings_manager.load_all().start_on_boot
                    }
                    Err(e) => {
                        warn!(error=%e, "Failed to access database for settings, defaulting to not starting on boot");
                        false
                    }
                }
            };

            if let Err(e) = taurine_core::service::up(start_on_boot) {
                error!(error=%e, "Failed to restart daemon after update. Please run `taurine up` manually.");
            } else {
                info!("Daemon restarted successfully.");
            }
        }
        Ok(None) => {
            info!("Taurine is already up to date.");
        }
        Err(e) => {
            error!(error=%e, "Failed to install update");
            return Err(Error::Engine(e.to_string()));
        }
    }

    Ok(())
}
