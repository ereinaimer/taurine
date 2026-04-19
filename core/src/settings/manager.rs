use super::Settings;
use crate::db::crud::{get_setting_value, upsert_setting};
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

        if let Ok(Some(val)) = get_setting_value(self.conn, "trigger_char")
            && let Ok(v) = serde_json::from_str::<String>(&val)
            && let Some(c) = v.chars().next()
        {
            settings.trigger_char = c;
        }

        if let Ok(Some(val)) = get_setting_value(self.conn, "pause_hotkey")
            && let Ok(v) = serde_json::from_str::<String>(&val)
        {
            settings.pause_hotkey = v;
        }

        if let Ok(Some(val)) = get_setting_value(self.conn, "pause_notifications_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(&val)
        {
            settings.pause_notifications_enabled = v;
        }

        if let Ok(Some(val)) = get_setting_value(self.conn, "start_on_boot")
            && let Ok(v) = serde_json::from_str::<bool>(&val)
        {
            settings.start_on_boot = v;
        }

        if let Ok(Some(val)) = get_setting_value(self.conn, "spinner_style")
            && let Ok(v) = serde_json::from_str::<super::SpinnerStyle>(&val)
        {
            settings.spinner_style = v;
        }

        if let Ok(Some(val)) = get_setting_value(self.conn, "ai_provider")
            && let Ok(v) = serde_json::from_str::<Option<String>>(&val)
        {
            settings.ai_provider = v;
        }

        if let Ok(Some(val)) = get_setting_value(self.conn, "ai_model")
            && let Ok(v) = serde_json::from_str::<Option<String>>(&val)
        {
            settings.ai_model = v;
        }

        if let Ok(Some(val)) = get_setting_value(self.conn, "inline_ai_delimiter")
            && let Ok(v) = serde_json::from_str::<String>(&val)
            && let Some(c) = v.chars().next()
        {
            settings.inline_ai_delimiter = c;
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
