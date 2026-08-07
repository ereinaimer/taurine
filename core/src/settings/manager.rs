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

        if let Some(val) = map.get("inline_ai_trigger_mode")
            && let Ok(v) = serde_json::from_str::<super::InlineAiTriggerMode>(val)
        {
            settings.inline_ai_trigger_mode = v;
        }

        if let Some(val) = map.get("inline_ai_trigger")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_ai_trigger = v;
        }

        if let Some(val) = map.get("inline_ai_trigger_open")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_ai_trigger_open = v;
        }

        if let Some(val) = map.get("inline_ai_trigger_close")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_ai_trigger_close = v;
        }

        if let Some(val) = map.get("clipboard_restore_delay_ms")
            && let Ok(v) = serde_json::from_str::<u32>(val)
        {
            settings.clipboard_restore_delay_ms = Settings::sanitize_clipboard_restore_delay_ms(v);
        }

        if let Some(val) = map.get("action_key")
            && let Ok(v) = serde_json::from_str::<super::ActionKey>(val)
        {
            settings.action_key = v;
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

        if let Some(val) = map.get("inline_emoji_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.inline_emoji_enabled = v;
        }

        if let Some(val) = map.get("inline_emoji_trigger_char")
            && let Ok(v) = serde_json::from_str::<char>(val)
        {
            settings.inline_emoji_trigger_char = v;
        }

        if let Some(val) = map.get("scripts_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.scripts_enabled = v;
        }

        if let Some(val) = map.get("system_tray_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.system_tray_enabled = v;
        }

        if let Some(val) = map.get("inline_datetime_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.inline_datetime_enabled = v;
        }
        if let Some(val) = map.get("inline_currency_to_words_enabled")
            && let Ok(v) = serde_json::from_str::<bool>(val)
        {
            settings.inline_currency_to_words_enabled = v;
        }
        if let Some(val) = map.get("inline_datetime_date_format")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_datetime_date_format = v;
        }
        if let Some(val) = map.get("inline_datetime_time_format")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_datetime_time_format = v;
        }
        if let Some(val) = map.get("inline_datetime_datetime_format")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_datetime_datetime_format = v;
        }
        if let Some(val) = map.get("inline_datetime_dialect")
            && let Ok(v) = serde_json::from_str::<String>(val)
        {
            settings.inline_datetime_dialect = v;
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

        let settings = manager.load_all();
        assert!(!settings.inline_tab_completion_enabled);
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
}
