pub mod outbox;
pub mod quota;
pub mod settings;
pub mod stats;
pub mod target_os;
pub mod tier;
pub mod triggers;
pub mod workspaces;

pub use outbox::*;
pub use tier::{UserTier, get_user_tier, set_user_tier};

pub use quota::{
    QuotaLedgerRow, calculate_week_start_epoch, compute_quota_hmac, get_or_init_quota_ledger,
    record_quota_depletion, set_quota_balance_from_cloud, verify_quota_hmac,
};
pub use target_os::TargetOs;
pub use triggers::{
    ActionType, AddOutcome, AppFilterPrefix, CloudSnippetPayload, ExistingTriggerUpdate,
    InvocationType, MAX_TRIGGER_LENGTH, NewEntry, NewTrigger, PreparedTrigger, ReconcileOutcome,
    ResolvedInvocation, TriggerAction, TriggerAliasRow, TriggerConflict, TriggerLimits,
    TriggerListItem, TriggerRow, TriggerSummary, TriggerType, add_alias, add_trigger,
    add_trigger_by_type, add_trigger_by_type_with_case, add_trigger_with_case, app_filters_overlap,
    audit_payload_tags, audit_payload_tags_with_trigger_type, audit_script_payload_tags,
    count_aliases, count_triggers_by_pattern, create_entry, create_trigger, create_trigger_entry,
    delete_alias, delete_trigger, delete_trigger_by_value, delete_trigger_entry,
    delete_triggers_by_pattern, delete_triggers_by_tag, delete_triggers_by_values, display_alias,
    display_for_aliases, find_parent_by_invocation, find_trigger_overlap_conflict,
    get_action_by_trigger, get_all_active_hotkey_triggers, get_all_active_regex_triggers,
    get_all_active_triggers, get_library_triggers, get_syncable_triggers, get_trigger,
    get_triggers_list, get_unsynced_triggers, increment_usage_count_by_id,
    increment_usage_count_by_trigger, list_active_voice_invocations, list_aliases,
    list_distinct_tags, mark_trigger_synced, normalize_tags, normalize_voice_phrase,
    prepare_trigger, prepare_trigger_with_type, purge_deleted_tombstones, reconcile_cloud_snippet,
    record_expansion_usage, rename_alias_invocation, search_triggers,
    serialize_trigger_to_cloud_snippet, set_alias_invocation_type, set_alias_require_confirmation,
    set_script_behavior, set_script_interpreter, set_trigger_action_type, set_trigger_auto_case,
    set_trigger_content, set_trigger_description, set_trigger_enabled, set_trigger_except_apps,
    set_trigger_only_apps, set_trigger_tags, set_trigger_target_os, split_app_filters,
    target_os_values_overlap, threshold_for_phrase, tombstone_entry, update_existing_trigger,
    update_trigger_app_filters, update_trigger_entry, upsert_entry_full, upsert_script,
    upsert_trigger, upsert_trigger_with_type, upsert_trigger_with_type_and_case,
    validate_trigger_target_os_conflict, validate_voice_phrase,
};
pub use workspaces::{
    WorkspaceRow, create_workspace, delete_workspace, get_default_workspace, get_workspace_by_id,
    get_workspaces,
};

pub use crate::stats::TriggerStatKind;
pub use settings::{
    CloudSettingPayload, SYNCABLE_SETTINGS, SettingReconcileOutcome, SettingRow, delete_setting,
    get_all_settings, get_setting, get_setting_value, get_unsynced_settings, is_setting_syncable,
    mark_settings_synced, reconcile_cloud_setting, seed_default_setting, set_setting,
    upsert_setting,
};
pub use stats::{
    ActivityCategory, ActivityEventRow, AppStatRow, AppStatsSortBy, CloudActivityEventPayload,
    CloudTriggerDailyStatPayload, CloudUserAppStatPayload, CloudUserStatPayload, NewActivityEvent,
    StatDeltas, StatRow, StreakInfo, TopAppStat, TopTriggerDailyStat, TriggerDailyStatRow,
    TriggerStatEvent, calculate_streaks, delete_stat, format_app_display_name, get_activity_events,
    get_recent_activity_events, get_stat, get_stat_counters, get_top_app_stats_with_conn,
    get_top_triggers_daily_with_conn, get_trigger_daily_stats_by_date,
    get_trigger_daily_stats_by_trigger, get_unsynced_activity_events, get_unsynced_app_stats,
    get_unsynced_stats, get_unsynced_trigger_daily_stats, increment_stat, insert_activity_event,
    mark_activity_events_synced, mark_app_stats_synced, mark_stats_synced,
    mark_trigger_daily_stats_synced, reconcile_cloud_activity_events, reconcile_cloud_app_stats,
    reconcile_cloud_trigger_daily_stats, reconcile_cloud_user_stats, record_calculation_usage,
    record_trigger_stat, record_trigger_stat_with_conn, record_voice_dictation_usage,
    record_voice_trigger_usage, record_voice_trigger_usage_with_conn, resolve_trigger_id,
    upsert_app_stat_with_conn, upsert_trigger_daily_stat_with_conn,
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
