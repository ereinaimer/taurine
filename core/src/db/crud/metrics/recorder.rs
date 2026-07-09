use rusqlite::Connection;

use crate::db::crud::automations::increment_usage_count_by_trigger;
use crate::metrics::{AutomationMetricKind, calculate_expansion_metrics, get_current_date_string};
use crate::settings::Settings;

use super::increment_metric;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationMetricEvent {
    pub automation_trigger: Option<String>,
    pub trigger_chars: usize,
    pub success: bool,
    pub output_chars: usize,
    pub kind: AutomationMetricKind,
    pub wpm: Option<u32>,
}

impl AutomationMetricEvent {
    fn should_record(&self) -> bool {
        match self.kind {
            AutomationMetricKind::InlineAi => self.success,
            _ => self.success || self.output_chars > 0,
        }
    }
}

pub fn record_automation_metric(event: AutomationMetricEvent) {
    if cfg!(test) {
        match crate::db::get_conn() {
            Ok(mut conn) => {
                if let Err(error) = record_automation_metric_with_conn(&mut conn, &event) {
                    tracing::warn!(error = %error, ?event, "Failed to record automation metric synchronously in test");
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, ?event, "Could not get pooled connection for metrics synchronously in test");
            }
        }
        return;
    }

    use std::sync::OnceLock;
    use std::sync::mpsc::{self, Sender};
    use std::thread;

    static METRICS_TX: OnceLock<Sender<AutomationMetricEvent>> = OnceLock::new();

    let tx = METRICS_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<AutomationMetricEvent>();
        thread::Builder::new()
            .name("taurine-metrics".to_string())
            .spawn(move || {
                while let Ok(evt) = rx.recv() {
                    match crate::db::get_conn() {
                        Ok(mut conn) => {
                            if let Err(error) = record_automation_metric_with_conn(&mut conn, &evt) {
                                tracing::warn!(error = %error, ?evt, "Failed to record automation metric in background");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(error = %error, ?evt, "Could not get pooled connection for metrics in background");
                        }
                    }
                }
            })
            .expect("Failed to spawn metrics background thread");
        tx
    });

    if let Err(e) = tx.send(event) {
        tracing::warn!("Failed to send metric event to background channel: {}", e);
    }
}

pub fn record_automation_metric_with_conn(
    conn: &mut Connection,
    event: &AutomationMetricEvent,
) -> crate::Result<()> {
    if !event.should_record() {
        return Ok(());
    }

    let date = get_current_date_string();
    let tx = conn.transaction()?;

    let (executions, ai_executions, keystrokes_saved, time_saved_ms) = match event.kind {
        AutomationMetricKind::InlineAi => (0, 1, 0, 0),
        AutomationMetricKind::Snippet | AutomationMetricKind::Calculation => {
            let metrics = calculate_expansion_metrics(
                event.output_chars,
                event.trigger_chars,
                effective_wpm(&tx, event.wpm),
            );
            (1, 0, metrics.keystrokes_saved, metrics.time_saved_ms)
        }
        AutomationMetricKind::Hotkey | AutomationMetricKind::Script => {
            let metrics =
                calculate_expansion_metrics(event.output_chars, 0, effective_wpm(&tx, event.wpm));
            (1, 0, metrics.keystrokes_saved, metrics.time_saved_ms)
        }
    };

    if let Some(trigger) = event.automation_trigger.as_deref() {
        increment_usage_count_by_trigger(&tx, trigger)?;
    }

    increment_metric(
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
