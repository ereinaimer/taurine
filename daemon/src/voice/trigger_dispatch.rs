use rusqlite::Connection;
use taurine_core::db::crud::{ResolvedInvocation, increment_usage_count_by_id};

/// Expands a matched voice trigger through the canonical expansion pipeline
/// and dispatches injection on a guarded thread. Voice has no typed text to
/// erase (`delete_count` 0) and never participates in Backspace Undo.
pub fn fire_voice_trigger(
    inv: ResolvedInvocation,
    conn: &Connection,
    active_app: Option<String>,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    let action = inv.action.clone();
    let Some(expansion) =
        taurine_core::engine::catalog::expand_trigger_action(action, &inv.invocation)
    else {
        tracing::warn!(
            "Voice trigger '{}' expanded to None; skipping",
            inv.invocation
        );
        return;
    };
    if expansion.ai_transformer_template.is_some() {
        tracing::warn!(
            "Voice trigger '{}' contains AI transformer; injecting static steps only",
            inv.invocation
        );
    }
    let steps = expansion.steps;
    let output_chars = inv.action.output.chars().count();
    let phrase = inv.invocation.clone();
    crate::injector::spawn_guarded_injection_thread("tau-voice-disp", move || {
        crate::injector::inject_expansion(steps, 0, spinner_style);
    });
    taurine_core::db::crud::record_voice_trigger_usage(&phrase, output_chars, active_app);
    let _ = increment_usage_count_by_id(conn, &inv.trigger_id);
}
