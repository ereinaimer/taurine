use std::time::{SystemTime, UNIX_EPOCH};

use taurine_core::stats::{calculate_saved_keystrokes, calculate_time_saved_ms};

use super::trigger::LibraryTrigger;
use crate::widgets::library::detail::{EMPTY_TOKEN, property_rows, relative_time};

/// Read-only snapshot for the trigger info popup. Cloned at open time;
/// the popup never refreshes, it just dismisses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LibraryInfoModalState {
    item: LibraryTrigger,
}

impl LibraryInfoModalState {
    pub(crate) fn from_item(item: &LibraryTrigger) -> Self {
        Self { item: item.clone() }
    }

    pub(crate) fn item(&self) -> &LibraryTrigger {
        &self.item
    }
}

/// Popup rows: base properties plus usage extras. Text and voice triggers
/// show computed savings; scripts show runs only (no keystrokes saved).
pub(crate) fn info_rows(item: &LibraryTrigger) -> Vec<(&'static str, String)> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let mut rows = property_rows(item);
    rows.push((
        "Created",
        relative_time(item.created_at()).unwrap_or_else(|| EMPTY_TOKEN.to_string()),
    ));
    rows.push(("Frequency", frequency_line(item, now)));
    if !item.is_script() {
        let per_use = calculate_saved_keystrokes(
            item.content().chars().count(),
            item.trigger().chars().count(),
        );
        rows.push((
            "Keystrokes saved",
            if item.usage_count() <= 0 {
                EMPTY_TOKEN.to_string()
            } else {
                format_int(per_use.saturating_mul(item.usage_count()))
            },
        ));
        rows.push((
            "Time saved",
            if item.usage_count() <= 0 {
                EMPTY_TOKEN.to_string()
            } else {
                format_duration(
                    calculate_time_saved_ms(per_use, 0).saturating_mul(item.usage_count()),
                )
            },
        ));
    }
    rows
}

fn frequency_line(item: &LibraryTrigger, now: i64) -> String {
    if item.usage_count() <= 0 {
        return EMPTY_TOKEN.to_string();
    }
    let days = now
        .saturating_sub(item.created_at())
        .saturating_div(86_400)
        .max(1);
    let per_day = item.usage_count() as f64 / days as f64;
    if per_day < 0.05 {
        return format!("{} total", usage_total(item.usage_count()));
    }
    let rounded = (per_day * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("≈{:.0}/day", rounded)
    } else {
        format!("≈{rounded:.1}/day")
    }
}

fn usage_total(count: i64) -> String {
    if count == 1 {
        "1 use".to_string()
    } else {
        format!("{count} uses")
    }
}

fn format_int(value: i64) -> String {
    let negative = value < 0;
    let digits: Vec<char> = value.abs().to_string().chars().collect();
    let mut out = String::new();
    for (index, ch) in digits.iter().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*ch);
    }
    if negative { format!("-{out}") } else { out }
}

fn format_duration(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    if minutes < 60 {
        let rest = secs % 60;
        return if rest == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {rest}s")
        };
    }
    let hours = minutes / 60;
    let rest = minutes % 60;
    if rest == 0 {
        format!("{hours}h")
    } else {
        format!("{hours}h {rest}m")
    }
}
