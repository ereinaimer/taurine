use rusqlite::Connection;
use taurine_core::db::crud::{ResolvedInvocation, increment_usage_count_by_id};
use taurine_core::engine::variables::{ArgMap, ExpansionStep};

/// Expands a matched voice trigger through the canonical expansion pipeline
/// and dispatches injection on a guarded thread. Voice has no typed text to
/// erase (`delete_count` 0) and never participates in Backspace Undo.
///
/// Returns the display text (expanded `Text` steps, else the raw output) so
/// callers can report what was injected; `None` when expansion fails.
pub fn fire_voice_trigger_with_args(
    inv: ResolvedInvocation,
    args: &ArgMap,
    spoken_transcript: &str,
    conn: &Connection,
    active_app: Option<String>,
    spinner_style: taurine_core::settings::SpinnerStyle,
) -> Option<String> {
    let action = inv.action.clone();
    let Some(expansion) = taurine_core::engine::catalog::expand_trigger_action_with_args(
        action,
        args,
        &inv.invocation,
    ) else {
        tracing::warn!(
            "Voice trigger '{}' expanded to None; skipping",
            inv.invocation
        );
        return None;
    };
    if expansion.ai_transformer_template.is_some() {
        tracing::warn!(
            "Voice trigger '{}' contains AI transformer; injecting static steps only",
            inv.invocation
        );
    }
    let display = expansion
        .steps
        .iter()
        .filter_map(|step| match step {
            ExpansionStep::Text(text) => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    let display = if display.is_empty() {
        inv.action.output.clone()
    } else {
        display
    };
    let output_chars = display.chars().count();
    let steps = expansion.steps;
    crate::injector::spawn_guarded_injection_thread("tau-voice-disp", move || {
        crate::injector::inject_expansion_for_voice(steps, spinner_style);
    });
    taurine_core::db::crud::record_voice_trigger_usage(spoken_transcript, output_chars, active_app);
    let _ = increment_usage_count_by_id(conn, &inv.trigger_id);
    Some(display)
}

/// Expands a matched voice trigger through the canonical expansion pipeline
/// and dispatches injection on a guarded thread. Voice has no typed text to
/// erase (`delete_count` 0) and never participates in Backspace Undo.
pub fn fire_voice_trigger(
    inv: ResolvedInvocation,
    conn: &Connection,
    active_app: Option<String>,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    let spoken = inv.invocation.clone();
    let _ = fire_voice_trigger_with_args(
        inv,
        &ArgMap::default(),
        &spoken,
        conn,
        active_app,
        spinner_style,
    );
}
