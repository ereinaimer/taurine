use crate::db::crud::triggers::validate::validate_trigger_type;

use super::aliases::{InvocationType, validate_voice_phrase};
use super::trigger_set::*;
use super::{TriggerConflict, TriggerType};
use crate::Result;
use crate::keys::hotkey_strings_overlap;
use rusqlite::Connection;

pub fn target_os_values_overlap(left: &str, right: &str) -> bool {
    left == right || left == "all" || right == "all"
}

pub fn app_filters_overlap(
    only_a: Option<&str>,
    except_a: Option<&str>,
    only_b: Option<&str>,
    except_b: Option<&str>,
) -> bool {
    let clean_list = |s: &str| -> Vec<String> {
        split_app_filters(s)
            .into_iter()
            .map(|x| x.to_lowercase())
            .collect()
    };

    let o_a = only_a.map(clean_list);
    let e_a = except_a.map(clean_list);
    let o_b = only_b.map(clean_list);
    let e_b = except_b.map(clean_list);

    if o_a.is_none() && e_a.is_none() && o_b.is_none() && e_b.is_none() {
        return true;
    }

    match (o_a, e_a, o_b, e_b) {
        (Some(only_a), None, Some(only_b), None) => only_a.iter().any(|a| only_b.contains(a)),
        (Some(only_a), None, None, Some(except_b)) => only_a.iter().any(|a| !except_b.contains(a)),
        (None, Some(except_a), Some(only_b), None) => only_b.iter().any(|b| !except_a.contains(b)),
        _ => true,
    }
}

pub fn find_trigger_overlap_conflict(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    exclude_id: Option<&str>,
) -> Result<Option<TriggerConflict>> {
    let normalized = if matches!(trigger_type, TriggerType::Hotkey) {
        crate::keys::parse_hotkey(trigger)
            .map_err(|error| {
                crate::Error::Config(format!(
                    "Invalid hotkey '{}' during overlap validation: {}",
                    trigger, error
                ))
            })?
            .canonical_string()
    } else {
        trigger.to_string()
    };

    find_overlap_conflict(
        conn,
        trigger_type.as_db_str(),
        &normalized,
        matches!(trigger_type, TriggerType::Hotkey),
        target_os,
        only_apps,
        except_apps,
        exclude_id,
    )
}

pub fn find_voice_overlap_conflict(
    conn: &Connection,
    trigger: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    exclude_id: Option<&str>,
) -> Result<Option<TriggerConflict>> {
    let normalized = validate_voice_phrase(trigger)?;
    find_overlap_conflict(
        conn,
        InvocationType::Voice.as_db_str(),
        &normalized,
        false,
        target_os,
        only_apps,
        except_apps,
        exclude_id,
    )
}

#[allow(clippy::too_many_arguments)]
fn find_overlap_conflict(
    conn: &Connection,
    invocation_type: &str,
    trigger: &str,
    is_hotkey: bool,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    exclude_id: Option<&str>,
) -> Result<Option<TriggerConflict>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.id, al.invocation_type, al.invocation, t.target_os, t.only_apps, t.except_apps
         FROM trigger_aliases al JOIN triggers t ON t.id = al.trigger_id
         WHERE al.invocation_type = ?1
           AND t.is_deleted = 0
         ORDER BY t.updated_at DESC",
    )?;

    let rows = stmt.query_map([invocation_type], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    for row in rows {
        let (
            id,
            trigger_type_raw,
            existing_trigger,
            existing_target_os,
            existing_only,
            existing_except,
        ) = row?;
        if exclude_id.is_some_and(|excluded| excluded == id) {
            continue;
        }

        let overlaps = if is_hotkey {
            hotkey_strings_overlap(trigger, &existing_trigger).map_err(|error| {
                crate::Error::Config(format!(
                    "Invalid stored hotkey '{}' during overlap validation: {}",
                    existing_trigger, error
                ))
            })?
        } else {
            trigger == existing_trigger
        };

        if overlaps
            && target_os_values_overlap(&existing_target_os, target_os)
            && app_filters_overlap(
                only_apps,
                except_apps,
                existing_only.as_deref(),
                existing_except.as_deref(),
            )
        {
            return Ok(Some(TriggerConflict {
                id,
                trigger_type: InvocationType::parse_db(&trigger_type_raw)?,
                trigger: existing_trigger,
                target_os: existing_target_os,
            }));
        }
    }

    Ok(None)
}

pub fn validate_trigger_target_os_conflict(
    conn: &Connection,
    trigger_type: TriggerType,
    trigger: &str,
    target_os: &str,
    only_apps: Option<&str>,
    except_apps: Option<&str>,
    exclude_id: Option<&str>,
) -> Result<()> {
    validate_trigger_type(trigger_type, target_os)?;

    if let Some(conflict) = find_trigger_overlap_conflict(
        conn,
        trigger_type,
        trigger,
        target_os,
        only_apps,
        except_apps,
        exclude_id,
    )? {
        return Err(crate::Error::Config(format!(
            "{} '{}' conflicts with existing trigger on target_os '{}' (app filters overlap)",
            conflict.trigger_type.as_db_str(),
            trigger,
            conflict.target_os
        )));
    }

    Ok(())
}
