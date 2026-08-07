pub mod settings;
pub mod stats;
pub mod triggers;

pub use triggers::{
    AddOutcome, ExistingTriggerUpdate, NewTrigger, PreparedTrigger, TriggerAction, TriggerConflict,
    TriggerListItem, TriggerRow, TriggerSummary, TriggerType, add_trigger, add_trigger_by_type,
    add_trigger_by_type_with_case, add_trigger_with_case, audit_payload_tags,
    audit_payload_tags_with_trigger_type, audit_script_payload_tags, count_triggers_by_pattern,
    create_trigger, delete_trigger, delete_trigger_by_value, delete_triggers_by_pattern,
    delete_triggers_by_tag, delete_triggers_by_values, find_trigger_overlap_conflict,
    get_action_by_trigger, get_all_active_hotkey_triggers, get_all_active_regex_triggers,
    get_all_active_triggers, get_syncable_triggers, get_trigger, get_triggers_list,
    increment_usage_count_by_trigger, normalize_tags, prepare_trigger, prepare_trigger_with_type,
    record_expansion_usage, search_triggers, target_os_values_overlap, update_existing_trigger,
    update_trigger_app_filters, upsert_script, upsert_trigger, upsert_trigger_with_type,
    upsert_trigger_with_type_and_case, validate_trigger_target_os_conflict,
};

pub use crate::stats::TriggerStatKind;
pub use settings::{
    SettingRow, delete_setting, get_all_settings, get_setting, get_setting_value, upsert_setting,
};
pub use stats::{
    StatRow, TriggerStatEvent, delete_stat, get_stat, get_stat_counters, increment_stat,
    record_calculation_usage, record_trigger_stat, record_trigger_stat_with_conn,
};

pub const SUPPORTED_TARGET_OS_VALUES: [&str; 6] = ["all", "win", "linux", "mac", "android", "ios"];

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
