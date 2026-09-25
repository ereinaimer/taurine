pub mod settings;
pub mod stats;
pub mod target_os;
pub mod triggers;

pub use target_os::TargetOs;
pub use triggers::{
    ActionType, AddOutcome, AppFilterPrefix, ExistingTriggerUpdate, InvocationType, NewEntry,
    NewTrigger, PreparedTrigger, ResolvedInvocation, TriggerAction, TriggerAliasRow,
    TriggerConflict, TriggerLimits, TriggerListItem, TriggerRow, TriggerSummary, TriggerType,
    add_alias, add_trigger, add_trigger_by_type, add_trigger_by_type_with_case,
    add_trigger_with_case, app_filters_overlap, audit_payload_tags,
    audit_payload_tags_with_trigger_type, audit_script_payload_tags, count_aliases,
    count_triggers_by_pattern, create_entry, create_trigger, delete_alias, delete_trigger,
    delete_trigger_by_value, delete_triggers_by_pattern, delete_triggers_by_tag,
    delete_triggers_by_values, display_alias, display_for_aliases, find_parent_by_invocation,
    find_trigger_overlap_conflict, get_action_by_trigger, get_all_active_hotkey_triggers,
    get_all_active_regex_triggers, get_all_active_triggers, get_syncable_triggers, get_trigger,
    get_triggers_list, increment_usage_count_by_id, increment_usage_count_by_trigger,
    list_active_voice_invocations, list_aliases, normalize_tags, normalize_voice_phrase,
    prepare_trigger, prepare_trigger_with_type, record_expansion_usage, search_triggers,
    target_os_values_overlap, threshold_for_phrase, tombstone_entry, update_existing_trigger,
    update_trigger_app_filters, upsert_entry_full, upsert_script, upsert_trigger,
    upsert_trigger_with_type, upsert_trigger_with_type_and_case,
    validate_trigger_target_os_conflict, validate_voice_phrase,
};

pub use crate::stats::TriggerStatKind;
pub use settings::{
    SettingRow, delete_setting, get_all_settings, get_setting, get_setting_value, upsert_setting,
};
pub use stats::{
    AppStatRow, AppStatsSortBy, StatDeltas, StatRow, TopAppStat, TriggerStatEvent, delete_stat,
    format_app_display_name, get_stat, get_stat_counters, get_top_app_stats_with_conn,
    increment_stat, record_calculation_usage, record_trigger_stat, record_trigger_stat_with_conn,
    record_voice_dictation_usage, record_voice_trigger_usage, upsert_app_stat_with_conn,
};

pub const SUPPORTED_TARGET_OS_VALUES: [&str; 6] = ["all", "win", "linux", "mac", "android", "ios"];

/// Returns the internal database identifier for the current platform's OS.
pub fn get_current_os_db_string() -> &'static str {
    TargetOs::current().to_db_str()
}

/// Normalizes CLI-friendly OS names to database identifiers.
///
/// Supported inputs: windows, linux, macos, all, android, ios.
pub fn normalize_os(os: &str) -> Option<&'static str> {
    TargetOs::parse_str(os).map(TargetOs::to_db_str)
}
