use serde::{Deserialize, Serialize};

pub mod manager;

pub use manager::SettingsManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub trigger_char: char,
    pub pause_hotkey: String,
    pub pause_notifications_enabled: bool,
    pub start_on_boot: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trigger_char: '>',
            pause_hotkey: "Alt + `".to_string(),
            pause_notifications_enabled: true,
            start_on_boot: true,
        }
    }
}
