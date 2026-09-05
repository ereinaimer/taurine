use taurine_core::error::Result;
use taurine_core::settings::{SettingsManager, apply_setting_input};

pub struct TraySettings;

impl TraySettings {
    pub fn load_quick_settings() -> (bool, bool) {
        if let Ok(conn) = taurine_core::db::get_conn() {
            let manager = SettingsManager::new(&conn);
            let settings = manager.load_all();
            (settings.instant_expand, settings.start_on_boot)
        } else {
            (false, true)
        }
    }

    pub fn toggle_instant_expand() -> Result<bool> {
        let (current_instant, _) = Self::load_quick_settings();
        let next = !current_instant;
        Self::set_instant_expand(next)?;
        Ok(next)
    }

    pub fn set_instant_expand(enabled: bool) -> Result<()> {
        apply_setting_input(
            "instant_expand",
            Some(if enabled { "true" } else { "false" }),
        )?;
        Ok(())
    }

    pub fn toggle_start_on_boot() -> Result<bool> {
        let (_, current_boot) = Self::load_quick_settings();
        let next = !current_boot;
        Self::set_start_on_boot(next)?;
        Ok(next)
    }

    pub fn set_start_on_boot(enabled: bool) -> Result<()> {
        apply_setting_input(
            "start_on_boot",
            Some(if enabled { "true" } else { "false" }),
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toggle_instant_expand() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (initial_instant, _) = TraySettings::load_quick_settings();
        let new_val = TraySettings::toggle_instant_expand().expect("toggle instant expand");
        assert_eq!(new_val, !initial_instant);

        let restored = TraySettings::toggle_instant_expand().expect("restore instant expand");
        assert_eq!(restored, initial_instant);
    }

    #[test]
    fn test_toggle_start_on_boot() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (_, initial_boot) = TraySettings::load_quick_settings();
        let new_val = TraySettings::toggle_start_on_boot().expect("toggle start on boot");
        assert_eq!(new_val, !initial_boot);

        let restored = TraySettings::toggle_start_on_boot().expect("restore start on boot");
        assert_eq!(restored, initial_boot);
    }
}
