use rusqlite::Connection;

use crate::db::crud::triggers::increment_usage_count_by_trigger;
use crate::settings::Settings;
use crate::stats::{TriggerStatKind, calculate_expansion_stats, get_current_date_string};

use super::{StatDeltas, increment_stat};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerStatEvent {
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
    if cfg!(test) {
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

    if let Some(trigger) = event.trigger.as_deref() {
        if event.kind == TriggerStatKind::VoiceTrigger {
            if let Ok(Some(parent_id)) = crate::db::crud::find_parent_by_invocation(
                &tx,
                crate::db::crud::InvocationType::Voice,
                trigger,
            ) {
                let _ = crate::db::crud::increment_usage_count_by_id(&tx, &parent_id);
            }
        } else {
            increment_usage_count_by_trigger(&tx, trigger)?;
        }
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
