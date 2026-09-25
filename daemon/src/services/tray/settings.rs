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

    /// Reloads only when the settings version moved since `last_seen`.
    /// Returns the fresh values plus the version to store for the next call.
    /// Only the native (Windows/macOS) tray polls for external edits.
    #[cfg(any(windows, target_os = "macos"))]
    pub fn load_quick_settings_if_changed(last_seen: u64) -> Option<(bool, bool, u64)> {
        let current = taurine_core::settings::settings_version();
        if current == last_seen {
            return None;
        }
        let (instant, boot) = Self::load_quick_settings();
        Some((instant, boot, current))
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

    pub fn get_voice_input_device() -> Option<String> {
        if let Ok(conn) = taurine_core::db::get_conn() {
            let manager = SettingsManager::new(&conn);
            manager.load_all().voice_input_device
        } else {
            taurine_core::settings::get_cached_voice_input_device()
        }
    }

    pub fn set_voice_input_device(device: Option<&str>) -> Result<()> {
        apply_setting_input("voice_input_device", device)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvVarGuard(&'static str);
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            // SAFETY: Serialized under TEST_LOCK for test database isolation.
            unsafe { std::env::remove_var(self.0) };
        }
    }

    #[test]
    fn test_toggle_instant_expand() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp_dir.path()) };
        let _env_guard = EnvVarGuard("TAURINE_DATA_DIR");

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
        let temp_dir = tempfile::tempdir().unwrap();
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp_dir.path()) };
        let _env_guard = EnvVarGuard("TAURINE_DATA_DIR");

        let (_, initial_boot) = TraySettings::load_quick_settings();
        let new_val = TraySettings::toggle_start_on_boot().expect("toggle start on boot");
        assert_eq!(new_val, !initial_boot);

        let restored = TraySettings::toggle_start_on_boot().expect("restore start on boot");
        assert_eq!(restored, initial_boot);
    }

    #[test]
    fn test_voice_input_device_round_trip() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp_dir.path()) };
        let _env_guard = EnvVarGuard("TAURINE_DATA_DIR");

        assert_eq!(TraySettings::get_voice_input_device(), None);
        TraySettings::set_voice_input_device(Some("Microphone Array")).expect("set device");
        assert_eq!(
            TraySettings::get_voice_input_device().as_deref(),
            Some("Microphone Array")
        );
        TraySettings::set_voice_input_device(None).expect("clear device");
        assert_eq!(TraySettings::get_voice_input_device(), None);
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn test_load_quick_settings_if_changed_skips_read_on_same_version() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp_dir.path()) };
        let _env_guard = EnvVarGuard("TAURINE_DATA_DIR");

        taurine_core::settings::set_cached_wpm(taurine_core::settings::get_cached_wpm());
        let version = taurine_core::settings::settings_version();
        assert!(
            TraySettings::load_quick_settings_if_changed(version).is_none(),
            "same version must skip the database read"
        );

        taurine_core::settings::set_cached_wpm(taurine_core::settings::get_cached_wpm());
        let (instant, boot) = TraySettings::load_quick_settings();
        let changed = TraySettings::load_quick_settings_if_changed(version)
            .expect("bumped version must reload");
        assert_eq!((changed.0, changed.1), (instant, boot));
        assert!(changed.2 > version);
    }
}
