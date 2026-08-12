mod app_filter;
mod assets;
mod overlap;
mod trigger_delete;
mod trigger_get;
mod trigger_set;
mod trigger_sync;
mod trigger_types;
mod usage;
mod validate;

pub use app_filter::AppFilterPrefix;

pub use trigger_delete::{
    count_triggers_by_pattern, delete_trigger, delete_trigger_by_value, delete_triggers_by_pattern,
    delete_triggers_by_tag, delete_triggers_by_values,
};

pub use trigger_get::{
    get_action_by_trigger, get_all_active_hotkey_triggers, get_all_active_regex_triggers,
    get_all_active_triggers, get_trigger, get_triggers_list, search_triggers,
};
pub use validate::{
    audit_payload_tags, audit_payload_tags_with_trigger_type, audit_script_payload_tags,
    normalize_tags,
};

pub use overlap::{
    find_trigger_overlap_conflict, target_os_values_overlap, validate_trigger_target_os_conflict,
};

pub use usage::{increment_usage_count_by_trigger, record_expansion_usage};

pub use trigger_set::{
    AddOutcome, ExistingTriggerUpdate, NewTrigger, PreparedTrigger, add_trigger,
    add_trigger_by_type, add_trigger_by_type_with_case, add_trigger_with_case, create_trigger,
    prepare_trigger, prepare_trigger_with_type, update_existing_trigger,
    update_trigger_app_filters, upsert_script, upsert_trigger, upsert_trigger_with_type,
    upsert_trigger_with_type_and_case,
};
pub use trigger_sync::get_syncable_triggers;
pub use trigger_types::{
    ActionType, TriggerAction, TriggerConflict, TriggerLimits, TriggerListItem, TriggerRow,
    TriggerSummary, TriggerType,
};

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;

#[cfg(test)]
mod trigger_set_tests;
