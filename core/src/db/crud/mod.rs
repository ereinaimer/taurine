pub mod ai_presets;
pub mod automations;
pub mod metrics;
pub mod settings;

pub use automations::{
    AddOutcome, AutomationAction, AutomationListItem, AutomationRow, AutomationSummary,
    TriggerConflict, TriggerType, add_automation_by_trigger, add_automation_by_trigger_type,
    delete_automation, delete_automation_by_trigger, delete_automations_by_triggers,
    find_trigger_overlap_conflict, get_action_by_trigger, get_all_active_automations,
    get_all_active_hotkey_automations, get_automation, get_automations_list,
    get_syncable_automations, increment_usage_count_by_trigger, record_expansion_usage,
    search_automations, target_os_values_overlap, upsert_automation,
    upsert_automation_with_trigger_type, upsert_script, validate_trigger_not_reserved,
    validate_trigger_target_os_conflict,
};

pub use crate::metrics::AutomationMetricKind;
pub use metrics::{
    AutomationMetricEvent, MetricRow, delete_metric, get_metric, get_metric_counters,
    increment_metric, record_automation_metric, record_automation_metric_with_conn,
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
