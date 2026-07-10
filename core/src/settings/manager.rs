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

        if let Some(val) = map.get("trigger_char")
            && let Ok(v) = serde_json::from_str::<String>(val)
            && let Some(c) = v.chars().next()
        {
            settings.trigger_char = c;
        }

        if let Some(val) = map.get("pause_hotkey")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.pause_hotkey = v;
        }

        if let Some(val) = map.get("pause_notifications_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.pause_notifications_enabled = v;
        }

        if let Some(val) = map.get("pause_audio_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.pause_audio_enabled = v;
        }

        if let Some(val) = map.get("start_on_boot")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.start_on_boot = v;
        }

        if let Some(val) = map.get("inline_tab_completion_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.inline_tab_completion_enabled = v;
        }

        if let Some(val) = map.get("inline_history_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.inline_history_enabled = v;
        }

        if let Some(val) = map.get("wpm")
            && let Ok(v) = serde_json::from_str::<u32>(val)
        {
            settings.wpm = Settings::sanitize_wpm(v);
        }

        if let Some(val) = map.get("spinner_style")
            && let Ok(v) = serde_json::from_str::<super::SpinnerStyle>(val)
        {
            settings.spinner_style = v;
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

        if let Some(val) = map.get("ai_delimiter_mode")
            && let Ok(v) = serde_json::from_str::<super::AiDelimiterMode>(val)
        {
            settings.ai_delimiter_mode = v;
        }

        if let Some(val) = map.get("ai_symmetric_delimiter")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.ai_symmetric_delimiter = v;
        }

        if let Some(val) = map.get("ai_open_delimiter")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.ai_open_delimiter = v;
        }

        if let Some(val) = map.get("ai_close_delimiter")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.ai_close_delimiter = v;
        }

        if let Some(val) = map.get("clipboard_restore_delay_ms")
            && let Ok(v) = serde_json::from_str::<u32>(val)
        {
            settings.clipboard_restore_delay_ms = Settings::sanitize_clipboard_restore_delay_ms(v);
        }

        if let Some(val) = map.get("action_delimiter")
            && let Ok(v) = serde_json::from_str::<super::ActionDelimiter>(val)
        {
            settings.action_delimiter = v;
        }

        if let Some(val) = map.get("triggerless_mode")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.triggerless_mode = v;
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

        if let Some(val) = map.get("rpc_port")
            && let Ok(v) = serde_json::from_str::<u16>(val)
        {
            settings.rpc_port = Settings::sanitize_rpc_port(v);
        }

        if let Some(val) = map.get("script_timeout")
            && let Ok(v) = serde_json::from_str::<u32>(val)
        {
            settings.script_timeout = v;
        }

        if let Some(val) = map.get("ai_temperature")
            && let Ok(v) = serde_json::from_str::<Option<f32>>(val)
        {
            settings.ai_temperature = v;
        }

        if let Some(val) = map.get("ai_max_tokens")
            && let Ok(v) = serde_json::from_str::<Option<u32>>(val)
        {
            settings.ai_max_tokens = v;
        }

        if let Some(val) = map.get("ai_system_prompt")
            && let Ok(v) = serde_json::from_str::<Option<String>>(val)
        {
            settings.ai_system_prompt = v;
        }

        if let Some(val) = map.get("auto_update")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.auto_update = v;
        }

        if let Some(val) = map.get("clipboard_history_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.clipboard_history_enabled = v;
        }

        if let Some(val) = map.get("clipboard_history_retention_secs")
            && let Ok(v) = serde_json::from_str::<u32>(val)
        {
            settings.clipboard_history_retention_secs =
                Settings::sanitize_clipboard_history_retention_secs(v);
        }

        if let Some(val) = map.get("rpc_mode")
            && let Ok(v) = serde_json::from_str::<super::RpcMode>(val)
        {
            settings.rpc_mode = v;
        }

        if let Some(val) = map.get("rpc_host")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.rpc_host = v;
        }

        if let Some(val) = map.get("rpc_token")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.rpc_token = v;
        }

        if settings.rpc_token.is_empty() {
            let token = uuid::Uuid::new_v4().to_string();
            settings.rpc_token = token.clone();
            let _ = self.update_setting("rpc_token", token);
            tracing::warn!(
                "Generated a new secure RPC authentication token. If you configure Taurine to use TCP mode, ensure this token is kept confidential."
            );
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
    fn load_all_reads_inline_trigger_assist_settings_from_db() {
        let (_dir, conn) = open_test_db();
        let manager = SettingsManager::new(&conn);

        manager
            .update_setting("inline_tab_completion_enabled", false)
            .unwrap();
        manager
            .update_setting("inline_history_enabled", false)
            .unwrap();

        let settings = manager.load_all();
        assert!(!settings.inline_tab_completion_enabled);
        assert!(!settings.inline_history_enabled);
    }
}
