use rusqlite::Connection;
use taurine_core::db::crud::voice_triggers::{VoiceTriggerRow, voice_trigger_row_to_action};

/// Expands a matched voice trigger through the canonical expansion pipeline
/// and dispatches injection on a guarded thread. Voice has no typed text to
/// erase (`delete_count` 0) and never participates in Backspace Undo.
pub fn fire_voice_trigger(
    row: VoiceTriggerRow,
    conn: &Connection,
    active_app: Option<String>,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    let action = voice_trigger_row_to_action(&row);
    let Some(expansion) =
        taurine_core::engine::catalog::expand_trigger_action(action, &row.spoken_phrase)
    else {
        tracing::warn!(
            "Voice trigger '{}' expanded to None; skipping",
            row.spoken_phrase
        );
        return;
    };
    if expansion.ai_transformer_template.is_some() {
        tracing::warn!(
            "Voice trigger '{}' contains AI transformer; injecting static steps only",
            row.spoken_phrase
        );
    }
    let steps = expansion.steps;
    let output_chars = row.output.chars().count();
    let phrase = row.spoken_phrase.clone();
    let id = row.id.clone();
    crate::injector::spawn_guarded_injection_thread("tau-voice-disp", move || {
        crate::injector::inject_expansion(steps, 0, spinner_style);
    });
    taurine_core::db::crud::record_voice_trigger_usage(&phrase, output_chars, active_app);
    let _ = taurine_core::db::crud::voice_triggers::increment_voice_trigger_usage(conn, &id);
}
