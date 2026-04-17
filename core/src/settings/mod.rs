use serde::{Deserialize, Serialize};

pub mod manager;

pub use manager::SettingsManager;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SpinnerStyle {
    #[default]
    Braille,
    Arc,
    Classic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub trigger_char: char,
    pub pause_hotkey: String,
    pub pause_notifications_enabled: bool,
    pub start_on_boot: bool,
    pub spinner_style: SpinnerStyle,
}

impl Settings {
    pub fn resolve_key(key: &str) -> &str {
        match key {
            "trigger" => "trigger_char",
            "hotkey" => "pause_hotkey",
            "notifications" => "pause_notifications_enabled",
            "boot" => "start_on_boot",
            "spinner" => "spinner_style",
            _ => key,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            trigger_char: '>',
            pause_hotkey: "Alt + `".to_string(),
            pause_notifications_enabled: true,
            start_on_boot: true,
            spinner_style: SpinnerStyle::default(),
        }
    }
}
