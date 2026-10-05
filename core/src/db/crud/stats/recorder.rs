use rusqlite::Connection;

use crate::db::crud::triggers::increment_usage_count_by_trigger;
use crate::settings::Settings;
use crate::stats::{TriggerStatKind, calculate_expansion_stats, get_current_date_string};

use super::{StatDeltas, increment_stat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerStatEvent {
    pub trigger_id: Option<String>,
    pub trigger: Option<String>,
    pub trigger_chars: usize,
    pub success: bool,
    pub output_chars: usize,
    pub kind: TriggerStatKind,
    pub wpm: Option<u32>,
    pub app: Option<String>,
    pub words_count: Option<usize>,
}

impl TriggerStatEvent {
    fn should_record(&self) -> bool {
        match self.kind {
            TriggerStatKind::InlineAi => self.success,
            _ => self.success || self.output_chars > 0,
        }
    }
}

pub fn record_trigger_stat(event: TriggerStatEvent) {
    // Synchronous in test harnesses: cfg(test) is false for this crate when
    // it is compiled as a dependency of other test binaries, so also probe
    // the runtime test-env marker. A detached background thread racing
    // process teardown segfaults test runners after the test reports ok.
    if cfg!(test) || crate::db::is_test_env() {
        match crate::db::get_conn() {
            Ok(mut conn) => {
                if let Err(error) = record_trigger_stat_with_conn(&mut conn, &event) {
                    tracing::warn!(error = %error, ?event, "Failed to record trigger stat synchronously in test");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, ?event, "Could not get pooled connection for stats synchronously in test");
            }
        }
        return;
    }

    use std::sync::OnceLock;
    use std::sync::mpsc::{self, Sender};
    use std::thread;

    static STATS_TX: OnceLock<Sender<TriggerStatEvent>> = OnceLock::new();

    let tx = STATS_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<TriggerStatEvent>();
        let spawn_result = thread::Builder::new()
            .name("tau-stats".to_string())
            .spawn(move || {
                while let Ok(evt) = rx.recv() {
                    match crate::db::get_conn() {
                        Ok(mut conn) => {
                            if let Err(error) = record_trigger_stat_with_conn(&mut conn, &evt) {
                                tracing::warn!(error = %error, ?evt, "Failed to record trigger stat in background");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, ?evt, "Could not get pooled connection for stats in background");
                        }
                    }
                }
            });
        if let Err(error) = spawn_result {
            tracing::error!(error = %error, "Failed to spawn stats background thread");
        }
        tx
    });

    if let Err(e) = tx.send(event) {
        tracing::warn!("Failed to send stat event to background channel: {}", e);
    }
}

/// Resolves the canonical trigger ID from explicit `trigger_id`, or by inspecting
/// the `trigger` field across aliases (voice, hotkey, word), direct ID, or trigger name.
pub fn resolve_trigger_id(
    conn: &Connection,
    event: &TriggerStatEvent,
) -> rusqlite::Result<Option<String>> {
    if let Some(trigger_id) = &event.trigger_id {
        return Ok(Some(trigger_id.clone()));
    }

    let Some(trigger) = event.trigger.as_deref() else {
        return Ok(None);
    };

    if event.kind == TriggerStatKind::VoiceTrigger
        && let Some(parent_id) = crate::db::crud::find_parent_by_invocation(
            conn,
            crate::db::crud::InvocationType::Voice,
            trigger,
        )
        .map_err(|err| match err {
            crate::Error::Database(inner) => inner,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        })?
    {
        return Ok(Some(parent_id));
    }

    if event.kind == TriggerStatKind::Hotkey
        && let Some(parent_id) = crate::db::crud::find_parent_by_invocation(
            conn,
            crate::db::crud::InvocationType::Hotkey,
            trigger,
        )
        .map_err(|err| match err {
            crate::Error::Database(inner) => inner,
            other => rusqlite::Error::ToSqlConversionFailure(Box::new(other)),
        })?
    {
        return Ok(Some(parent_id));
    }

    use rusqlite::OptionalExtension;

    let alias_match: Option<String> = conn
        .query_row(
            "SELECT trigger_id FROM trigger_aliases WHERE invocation = ?1 LIMIT 1",
            rusqlite::params![trigger],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = alias_match {
        return Ok(Some(id));
    }

    let direct_match: Option<String> = conn
        .query_row(
            "SELECT id FROM triggers WHERE id = ?1 AND is_deleted = 0 LIMIT 1",
            rusqlite::params![trigger],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = direct_match {
        return Ok(Some(id));
    }

    let name_match: Option<String> = conn
        .query_row(
            "SELECT id FROM triggers WHERE name = ?1 AND is_deleted = 0 LIMIT 1",
            rusqlite::params![trigger],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = name_match {
        return Ok(Some(id));
    }

    Ok(None)
}

pub fn record_trigger_stat_with_conn(
    conn: &mut Connection,
    event: &TriggerStatEvent,
) -> crate::Result<()> {
    if !event.should_record() {
        return Ok(());
    }

    let date = get_current_date_string();
    let tx = conn.transaction()?;

    let deltas = match event.kind {
        TriggerStatKind::InlineAi => StatDeltas {
            ai_executions: 1,
            ..Default::default()
        },
        TriggerStatKind::VoiceTrigger => {
            let stats = calculate_expansion_stats(
                event.output_chars,
                event.trigger_chars,
                effective_wpm(&tx, event.wpm),
            );
            StatDeltas {
                voice_executions: 1,
                keystrokes_saved: stats.keystrokes_saved,
                time_saved_ms: stats.time_saved_ms,
                ..Default::default()
            }
        }
        TriggerStatKind::VoiceDictation => {
            let words = event.words_count.unwrap_or(0);
            let stats =
                calculate_expansion_stats(event.output_chars, 0, effective_wpm(&tx, event.wpm));
            StatDeltas {
                voice_executions: 1,
                words_dictated: words as i64,
                keystrokes_saved: stats.keystrokes_saved,
                time_saved_ms: stats.time_saved_ms,
                ..Default::default()
            }
        }
        TriggerStatKind::Snippet | TriggerStatKind::Calculation => {
            let stats = calculate_expansion_stats(
                event.output_chars,
                event.trigger_chars,
                effective_wpm(&tx, event.wpm),
            );
            StatDeltas {
                executions: 1,
                keystrokes_saved: stats.keystrokes_saved,
                time_saved_ms: stats.time_saved_ms,
                ..Default::default()
            }
        }
        TriggerStatKind::Hotkey | TriggerStatKind::Script => StatDeltas {
            executions: 1,
            ..Default::default()
        },
    };

    let resolved_id = resolve_trigger_id(&tx, event)?;
    if let Some(id) = &resolved_id {
        let _ = crate::db::crud::increment_usage_count_by_id(&tx, id);
        let trigger_executions = match event.kind {
            TriggerStatKind::InlineAi => 1,
            TriggerStatKind::VoiceTrigger | TriggerStatKind::VoiceDictation => {
                deltas.voice_executions.max(0) as u64
            }
            _ => deltas.executions.max(0) as u64,
        };
        let app_key = event.app.as_deref().map(str::trim).unwrap_or("");
        super::upsert_trigger_daily_stat_with_conn(
            &tx,
            id,
            &date,
            app_key,
            trigger_executions,
            deltas.keystrokes_saved.max(0) as u64,
            deltas.time_saved_ms.max(0) as u64,
        )?;
    } else if let Some(trigger) = event.trigger.as_deref() {
        increment_usage_count_by_trigger(&tx, trigger)?;
    }

    increment_stat(&tx, &date, &deltas)?;

    if let Some(app_key) = event.app.as_deref().map(str::trim)
        && !app_key.is_empty()
    {
        let app_executions = match event.kind {
            TriggerStatKind::InlineAi => 1,
            TriggerStatKind::VoiceTrigger | TriggerStatKind::VoiceDictation => {
                deltas.voice_executions.max(0) as u64
            }
            _ => deltas.executions.max(0) as u64,
        };
        super::upsert_app_stat_with_conn(
            &tx,
            app_key,
            &date,
            app_executions,
            deltas.keystrokes_saved.max(0) as u64,
            deltas.time_saved_ms.max(0) as u64,
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn effective_wpm(_conn: &Connection, event_wpm: Option<u32>) -> u32 {
    event_wpm
        .map(Settings::sanitize_wpm)
        .unwrap_or_else(|| Settings::sanitize_wpm(crate::settings::get_cached_wpm()))
}

/// Records stats for a completed voice dictation session.
pub fn record_voice_dictation_usage(words_count: usize, chars_count: usize, app: Option<String>) {
    record_trigger_stat(TriggerStatEvent {
        trigger_id: None,
        trigger: None,
        trigger_chars: 0,
        success: true,
        output_chars: chars_count,
        kind: TriggerStatKind::VoiceDictation,
        wpm: None,
        app,
        words_count: Some(words_count),
    });
}

/// Records stats for a successfully triggered voice phrase expansion.
pub fn record_voice_trigger_usage(phrase: &str, output_chars: usize, app: Option<String>) {
    record_trigger_stat(TriggerStatEvent {
        trigger_id: None,
        trigger: Some(phrase.to_string()),
        trigger_chars: phrase.chars().count(),
        success: true,
        output_chars,
        kind: TriggerStatKind::VoiceTrigger,
        wpm: None,
        app,
        words_count: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recorder_persists_trigger_daily_stats() {
        let (_dir, mut conn) = crate::testing::open_test_db();

        // Insert trigger and alias
        conn.execute(
            "INSERT INTO triggers (id, name, output, created_at, updated_at) VALUES ('trig-abc', 'test_name', 'hello world', 100, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO trigger_aliases (id, trigger_id, invocation, invocation_type, created_at) VALUES ('alias-1', 'trig-abc', ':sig', 'word', 100)",
            [],
        ).unwrap();

        let event = TriggerStatEvent {
            trigger_id: None,
            trigger: Some(":sig".to_string()),
            trigger_chars: 4,
            success: true,
            output_chars: 20,
            kind: TriggerStatKind::Snippet,
            wpm: Some(60),
            app: Some("code.exe".to_string()),
            words_count: None,
        };

        record_trigger_stat_with_conn(&mut conn, &event).unwrap();

        let daily_stats =
            crate::db::crud::get_trigger_daily_stats_by_trigger(&conn, "trig-abc", 10).unwrap();
        assert_eq!(daily_stats.len(), 1);
        assert_eq!(daily_stats[0].trigger_id, "trig-abc");
        assert_eq!(daily_stats[0].app_key, "code.exe");
        assert_eq!(daily_stats[0].executions, 1);
        assert!(daily_stats[0].keystrokes_saved > 0);
    }

    #[test]
    fn test_resolve_trigger_id_explicit_precedence() {
        let (_dir, conn) = crate::testing::open_test_db();

        conn.execute(
            "INSERT INTO triggers (id, name, output, created_at, updated_at) VALUES ('trig-1', 'Name 1', 'output', 100, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO trigger_aliases (id, trigger_id, invocation, invocation_type, created_at) VALUES ('alias-1', 'trig-1', ':other', 'word', 100)",
            [],
        ).unwrap();

        let event = TriggerStatEvent {
            trigger_id: Some("trig-1".to_string()),
            trigger: Some(":nonexistent".to_string()),
            trigger_chars: 5,
            success: true,
            output_chars: 10,
            kind: TriggerStatKind::Snippet,
            wpm: None,
            app: None,
            words_count: None,
        };

        let resolved = resolve_trigger_id(&conn, &event).unwrap();
        assert_eq!(resolved.as_deref(), Some("trig-1"));
    }

    #[test]
    fn test_resolve_trigger_id_voice_invocation() {
        let (_dir, conn) = crate::testing::open_test_db();

        conn.execute(
            "INSERT INTO triggers (id, name, output, created_at, updated_at) VALUES ('trig-voice', 'Voice Trigger', 'output', 100, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO trigger_aliases (id, trigger_id, invocation, invocation_type, created_at) VALUES ('alias-v', 'trig-voice', 'launch browser', 'voice', 100)",
            [],
        ).unwrap();

        let event = TriggerStatEvent {
            trigger_id: None,
            trigger: Some("launch browser".to_string()),
            trigger_chars: 14,
            success: true,
            output_chars: 10,
            kind: TriggerStatKind::VoiceTrigger,
            wpm: None,
            app: None,
            words_count: None,
        };

        let resolved = resolve_trigger_id(&conn, &event).unwrap();
        assert_eq!(resolved.as_deref(), Some("trig-voice"));
    }

    #[test]
    fn test_resolve_trigger_id_hotkey_invocation() {
        let (_dir, conn) = crate::testing::open_test_db();

        conn.execute(
            "INSERT INTO triggers (id, name, output, created_at, updated_at) VALUES ('trig-hotkey', 'Hotkey Trigger', 'output', 100, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO trigger_aliases (id, trigger_id, invocation, invocation_type, created_at) VALUES ('alias-h', 'trig-hotkey', 'ctrl+shift+p', 'hotkey', 100)",
            [],
        ).unwrap();

        let event = TriggerStatEvent {
            trigger_id: None,
            trigger: Some("ctrl+shift+p".to_string()),
            trigger_chars: 12,
            success: true,
            output_chars: 10,
            kind: TriggerStatKind::Hotkey,
            wpm: None,
            app: None,
            words_count: None,
        };

        let resolved = resolve_trigger_id(&conn, &event).unwrap();
        assert_eq!(resolved.as_deref(), Some("trig-hotkey"));
    }

    #[test]
    fn test_resolve_trigger_id_direct_id_and_name() {
        let (_dir, conn) = crate::testing::open_test_db();

        conn.execute(
            "INSERT INTO triggers (id, name, output, created_at, updated_at) VALUES ('trig-direct', 'Unique Snippet Name', 'output', 100, 100)",
            [],
        ).unwrap();

        // Direct ID match
        let event_direct = TriggerStatEvent {
            trigger_id: None,
            trigger: Some("trig-direct".to_string()),
            trigger_chars: 11,
            success: true,
            output_chars: 10,
            kind: TriggerStatKind::Snippet,
            wpm: None,
            app: None,
            words_count: None,
        };
        assert_eq!(
            resolve_trigger_id(&conn, &event_direct).unwrap().as_deref(),
            Some("trig-direct")
        );

        // Name match
        let event_name = TriggerStatEvent {
            trigger_id: None,
            trigger: Some("Unique Snippet Name".to_string()),
            trigger_chars: 19,
            success: true,
            output_chars: 10,
            kind: TriggerStatKind::Snippet,
            wpm: None,
            app: None,
            words_count: None,
        };
        assert_eq!(
            resolve_trigger_id(&conn, &event_name).unwrap().as_deref(),
            Some("trig-direct")
        );

        // Unresolved match
        let event_unknown = TriggerStatEvent {
            trigger_id: None,
            trigger: Some("nonexistent".to_string()),
            trigger_chars: 11,
            success: true,
            output_chars: 10,
            kind: TriggerStatKind::Snippet,
            wpm: None,
            app: None,
            words_count: None,
        };
        assert_eq!(resolve_trigger_id(&conn, &event_unknown).unwrap(), None);
    }

    #[test]
    fn test_recorder_persists_trigger_daily_stats_without_app() {
        let (_dir, mut conn) = crate::testing::open_test_db();

        conn.execute(
            "INSERT INTO triggers (id, name, output, created_at, updated_at) VALUES ('trig-noapp', 'no app test', 'hello world', 100, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO trigger_aliases (id, trigger_id, invocation, invocation_type, created_at) VALUES ('alias-na', 'trig-noapp', ':na', 'word', 100)",
            [],
        ).unwrap();

        let event = TriggerStatEvent {
            trigger_id: None,
            trigger: Some(":na".to_string()),
            trigger_chars: 3,
            success: true,
            output_chars: 15,
            kind: TriggerStatKind::Snippet,
            wpm: Some(60),
            app: None,
            words_count: None,
        };

        record_trigger_stat_with_conn(&mut conn, &event).unwrap();

        let daily_stats =
            crate::db::crud::get_trigger_daily_stats_by_trigger(&conn, "trig-noapp", 10).unwrap();
        assert_eq!(daily_stats.len(), 1);
        assert_eq!(daily_stats[0].trigger_id, "trig-noapp");
        assert_eq!(daily_stats[0].app_key, "");
        assert_eq!(daily_stats[0].executions, 1);
    }

    #[test]
    fn test_recorder_daily_stats_accumulates_on_multiple_events() {
        let (_dir, mut conn) = crate::testing::open_test_db();

        conn.execute(
            "INSERT INTO triggers (id, name, output, created_at, updated_at) VALUES ('trig-accum', 'accum test', 'output', 100, 100)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO trigger_aliases (id, trigger_id, invocation, invocation_type, created_at) VALUES ('alias-acc', 'trig-accum', ':acc', 'word', 100)",
            [],
        ).unwrap();

        let event = TriggerStatEvent {
            trigger_id: None,
            trigger: Some(":acc".to_string()),
            trigger_chars: 4,
            success: true,
            output_chars: 20,
            kind: TriggerStatKind::Snippet,
            wpm: Some(60),
            app: Some("editor.exe".to_string()),
            words_count: None,
        };

        record_trigger_stat_with_conn(&mut conn, &event).unwrap();
        record_trigger_stat_with_conn(&mut conn, &event).unwrap();

        let daily_stats =
            crate::db::crud::get_trigger_daily_stats_by_trigger(&conn, "trig-accum", 10).unwrap();
        assert_eq!(daily_stats.len(), 1);
        assert_eq!(daily_stats[0].executions, 2);
        assert_eq!(daily_stats[0].keystrokes_saved, (20 - 4) * 2);
    }
}
