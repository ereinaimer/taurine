use super::Settings;
use crate::db::crud::{get_all_settings, upsert_setting};
use crate::error::Result;
use rusqlite::Connection;
use serde::Serialize;

pub struct SettingsManager<'a> {
    conn: &'a Connection,
}

impl<'a> SettingsManager<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Loads all settings from the database, falling back to defaults for missing keys.
    pub fn load_all(&self) -> Settings {
        let mut settings = Settings::default();
        let map = get_all_settings(self.conn).unwrap_or_default();

        if let Some(val) = map.get("pause_hotkey")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.pause_hotkey = v;
        }

        if let Some(val) = map.get("audio_theme")
            && let Ok(v) = serde_json::from_str::<super::AudioTheme>(val)
        {
            settings.audio_theme = v;
        }

        if let Some(val) = map.get("audio_volume")
            && let Ok(v) = serde_json::from_str::<u32>(val)
        {
            settings.audio_volume = Settings::sanitize_audio_volume(v);
        }

        if let Some(val) = map.get("start_on_boot")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.start_on_boot = v;
        }

        if let Some(val) = map.get("auto_update")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.auto_update = v;
        }

        if let Some(val) = map.get("instant_expand")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.instant_expand = v;
        }

        if let Some(val) = map.get("ignore_fullscreen")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.ignore_fullscreen = v;
        }

        if let Some(val) = map.get("system_tray_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.system_tray_enabled = v;
        }

        if let Some(val) = map
            .get("inline_date_format")
            .or_else(|| map.get("inline_datetime_date_format"))
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_date_format = v;
        }

        if let Some(val) = map
            .get("inline_time_format")
            .or_else(|| map.get("inline_datetime_time_format"))
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_time_format = v;
        }

        if let Some(val) = map
            .get("inline_datetime_format")
            .or_else(|| map.get("inline_datetime_datetime_format"))
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_datetime_format = v;
        }

        if let Some(val) = map.get("ai_provider")
            && let Ok(v) = serde_json::from_str::<Option<String>>(val)
        {
            settings.ai_provider = v;
        }

        if let Some(val) = map.get("ai_model")
            && let Ok(v) = serde_json::from_str::<Option<String>>(val)
        {
            settings.ai_model = v;
        }

        if let Some(val) = map.get("ai_custom_endpoint")
            && let Ok(v) = serde_json::from_str::<Option<String>>(val)
        {
            settings.ai_custom_endpoint = v;
        }

        if let Some(val) = map.get("voice_ptt_hotkey")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.voice_ptt_hotkey = v;
        }

        if let Some(val) = map.get("voice_handsfree_hotkey")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.voice_handsfree_hotkey = v;
        }

        if let Some(val) = map.get("voice_dictionary")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.voice_dictionary = v;
        }

        if let Some(val) = map.get("voice_preferred_languages")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.voice_preferred_languages = v;
        }

        if let Some(val) = map.get("voice_input_device")
            && let Ok(v) = serde_json::from_str::<Option<String>>(val)
        {
            settings.voice_input_device = v;
        }

        if let Some(val) = map.get("pause_media_while_dictating")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.pause_media_while_dictating = v;
        }

        if let Some(val) = map.get("tui_cursor_style")
            && let Ok(v) = serde_json::from_str::<super::TuiCursorStyle>(val)
        {
            settings.tui_cursor_style = v;
        }

        settings
    }

    /// Updates or inserts a persistent setting in the database.
    /// The value is serialized to a JSON string before storage.
    pub fn update_setting<T: Serialize>(&self, key: &str, value: T) -> Result<()> {
        let json_val = serde_json::to_string(&value)?;
        upsert_setting(self.conn, key, &json_val)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::open_test_db;

    #[test]
    fn load_all_reads_inline_date_time_formats_from_db() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        manager
            .update_setting("inline_date_format", "YYYY-MM-DD")
            .unwrap();
        manager
            .update_setting("inline_time_format", "HH:mm")
            .unwrap();
        manager
            .update_setting("inline_datetime_format", "YYYY-MM-DD HH:mm")
            .unwrap();

        let settings = manager.load_all();
        assert_eq!(settings.inline_date_format, "YYYY-MM-DD");
        assert_eq!(settings.inline_time_format, "HH:mm");
        assert_eq!(settings.inline_datetime_format, "YYYY-MM-DD HH:mm");
    }

    #[test]
    fn load_all_reads_system_tray_enabled_from_db() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        manager
            .update_setting("system_tray_enabled", false)
            .unwrap();

        let settings = manager.load_all();
        assert!(!settings.system_tray_enabled);
    }

    #[test]
    fn load_all_reads_voice_settings_from_db() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        manager
            .update_setting("voice_ptt_hotkey", "ctrl+space")
            .unwrap();
        manager
            .update_setting("voice_handsfree_hotkey", "ctrl+alt+space")
            .unwrap();
        manager
            .update_setting("voice_dictionary", "Rust, Nite, Whisper")
            .unwrap();

        let settings = manager.load_all();
        assert_eq!(settings.voice_ptt_hotkey, "ctrl+space");
        assert_eq!(settings.voice_handsfree_hotkey, "ctrl+alt+space");
        assert_eq!(settings.voice_dictionary, "Rust, Nite, Whisper");
    }

    #[test]
    fn load_all_reads_voice_preferred_languages_from_db() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        manager
            .update_setting("voice_preferred_languages", "en,ml")
            .unwrap();

        let settings = manager.load_all();
        assert_eq!(settings.voice_preferred_languages, "en,ml");
    }
}
