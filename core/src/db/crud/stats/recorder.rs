use rusqlite::Connection;

use crate::db::crud::automations::increment_usage_count_by_trigger;
use crate::settings::Settings;
use crate::stats::{AutomationStatKind, calculate_expansion_stats, get_current_date_string};

use super::increment_stat;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationStatEvent {
    pub automation_trigger: Option<String>,
    pub trigger_chars: usize,
    pub success: bool,
    pub output_chars: usize,
    pub kind: AutomationStatKind,
    pub wpm: Option<u32>,
}

impl AutomationStatEvent {
    fn should_record(&self) -> bool {
        match self.kind {
            AutomationStatKind::InlineAi => self.success,
            _ => self.success || self.output_chars > 0,
        }
    }
}

pub fn record_automation_stat(event: AutomationStatEvent) {
    if cfg!(test) {
        match crate::db::get_conn() {
            Ok(mut conn) => {
                if let Err(error) = record_automation_stat_with_conn(&mut conn, &event) {
                    tracing::warn!(error = %error, ?event, "Failed to record automation stat synchronously in test");
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

    static STATS_TX: OnceLock<Sender<AutomationStatEvent>> = OnceLock::new();

    let tx = STATS_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<AutomationStatEvent>();
        thread::Builder::new()
            .name("tau-stats".to_string())
            .spawn(move || {
                while let Ok(evt) = rx.recv() {
                    match crate::db::get_conn() {
                        Ok(mut conn) => {
                            if let Err(error) = record_automation_stat_with_conn(&mut conn, &evt) {
                                tracing::warn!(error = %error, ?evt, "Failed to record automation stat in background");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, ?evt, "Could not get pooled connection for stats in background");
                        }
                    }
                }
            })
            .expect("Failed to spawn stats background thread");
        tx
    });

    if let Err(e) = tx.send(event) {
        tracing::warn!("Failed to send stat event to background channel: {}", e);
    }
}

pub fn record_automation_stat_with_conn(
    conn: &mut Connection,
    event: &AutomationStatEvent,
) -> crate::Result<()> {
    if !event.should_record() {
        return Ok(());
    }

    let date = get_current_date_string();
    let tx = conn.transaction()?;

    let (executions, ai_executions, keystrokes_saved, time_saved_ms) = match event.kind {
        AutomationStatKind::InlineAi => (0, 1, 0, 0),
        AutomationStatKind::Snippet | AutomationStatKind::Calculation => {
            let stats = calculate_expansion_stats(
                event.output_chars,
                event.trigger_chars,
                effective_wpm(&tx, event.wpm),
            );
            (1, 0, stats.keystrokes_saved, stats.time_saved_ms)
        }
        AutomationStatKind::Hotkey | AutomationStatKind::Script => (1, 0, 0, 0),
    };

    if let Some(trigger) = event.automation_trigger.as_deref() {
        increment_usage_count_by_trigger(&tx, trigger)?;
    }

    increment_stat(
        &tx,
        &date,
        executions,
        ai_executions,
        keystrokes_saved,
        time_saved_ms,
    )?;

    tx.commit()?;
    Ok(())
}

fn effective_wpm(_conn: &Connection, event_wpm: Option<u32>) -> u32 {
    event_wpm
        .map(Settings::sanitize_wpm)
        .unwrap_or_else(|| Settings::sanitize_wpm(crate::settings::get_cached_wpm()))
}
