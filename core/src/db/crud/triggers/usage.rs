use crate::Result;
use crate::db::now_unix_secs;
use rusqlite::Connection;

pub fn increment_usage_count_by_trigger(conn: &Connection, trigger: &str) -> Result<()> {
    conn.execute(
        "UPDATE triggers
         SET usage_count = usage_count + 1,
             last_used_at = ?1
         WHERE trigger = ?2 AND is_deleted = 0",
        rusqlite::params![now_unix_secs(), trigger],
    )?;
    Ok(())
}

pub fn record_expansion_usage(
    trigger: &str,
    output_len: usize,
    _delete_count: usize,
    _left_arrow_count: usize,
) {
    crate::db::crud::record_trigger_stat(crate::db::crud::TriggerStatEvent {
        trigger: Some(trigger.to_string()),
        trigger_chars: trigger.chars().count(),
        success: output_len > 0,
        output_chars: output_len,
        kind: crate::db::crud::TriggerStatKind::Snippet,
        wpm: None,
    });
}
