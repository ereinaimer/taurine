pub mod automations;
pub mod metrics;
pub mod settings;

pub use automations::{
    AddOutcome, AutomationAction, AutomationListItem, AutomationRow, AutomationSummary,
    add_automation_by_trigger, delete_automation, delete_automation_by_trigger,
    get_action_by_trigger, get_all_active_automations, get_automation, get_automations_list,
    get_syncable_automations, increment_usage_count_by_trigger, record_expansion_usage,
    search_automations, upsert_automation, upsert_script,
};

pub use metrics::{
    MetricRow, delete_metric, get_metric, get_metric_counters, increment_metric,
    record_calculation_usage,
};
pub use settings::{SettingRow, delete_setting, get_setting, get_setting_value, upsert_setting};

/// Returns the internal database identifier for the current platform's OS.
pub fn get_current_os_db_string() -> &'static str {
    match std::env::consts::OS {
        "windows" => "win",
        "macos" => "mac",
        "linux" => "linux",
        "android" => "android",
        "ios" => "ios",
        _ => "unknown",
    }
}

/// Normalizes CLI-friendly OS names to database identifiers.
///
/// Supported inputs: windows, linux, macos, all, android, ios.
pub fn normalize_os(os: &str) -> Option<&'static str> {
    match os.to_lowercase().as_str() {
        "windows" => Some("win"),
        "macos" => Some("mac"),
        "linux" => Some("linux"),
        "android" => Some("android"),
        "ios" => Some("ios"),
        "all" => Some("all"),
        _ => None,
    }
}
