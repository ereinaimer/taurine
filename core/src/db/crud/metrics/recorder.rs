use rusqlite::Connection;

use crate::db::crud::automations::increment_usage_count_by_trigger;
use crate::metrics::{AutomationMetricKind, calculate_expansion_metrics, get_current_date_string};
use crate::settings::{Settings, SettingsManager};

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
    match Connection::open(crate::paths::get_db_path()) {
        Ok(mut conn) => {
            if let Err(error) = record_automation_metric_with_conn(&mut conn, &event) {
                tracing::warn!(error = %error, ?event, "Failed to record automation metric");
            }
        }
        Err(error) => {
            tracing::warn!(error = %error, ?event, "Could not open DB for automation metrics");
        }
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

    if !matches!(event.kind, AutomationMetricKind::InlineAi)
        && let Some(trigger) = event.automation_trigger.as_deref()
    {
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

fn effective_wpm(conn: &Connection, event_wpm: Option<u32>) -> u32 {
    event_wpm
        .map(Settings::sanitize_wpm)
        .unwrap_or_else(|| SettingsManager::new(conn).load_all().wpm)
}
