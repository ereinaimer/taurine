use std::sync::Arc;
use tracing::{debug, trace};

use crate::injector;
use crate::platform::spinner_renderer::OsSpinnerRenderer;

#[cfg(not(target_os = "linux"))]
pub(super) fn clear_undo_state(state: &taurine_core::engine::EngineState) {
    state.clear_undo_state();
}

#[cfg(not(target_os = "linux"))]
pub(super) fn take_active_undo_state(
    state: &taurine_core::engine::EngineState,
) -> Option<(String, usize)> {
    state
        .take_active_undo_state()
        .map(|undo| (undo.trigger_string, undo.output_length))
}

#[cfg(not(target_os = "linux"))]
pub(super) fn spawn_undo_dispatch(trigger_string: String, output_length: usize) {
    injector::spawn_guarded_injection_thread("tau-undo-disp", move || {
        injector::inject_undo(trigger_string, output_length);
    });
}

pub(crate) fn spawn_expansion_dispatch(
    expansion: taurine_core::engine::ExpansionResult,
    spinner_style: taurine_core::settings::SpinnerStyle,
    state: Arc<taurine_core::engine::EngineState>,
) {
    injector::spawn_guarded_injection_thread("tau-exp-disp", move || {
        dispatch_expansion_with(
            expansion,
            spinner_style,
            state,
            crate::injector::inject_expansion,
            launch_follow_up,
        );
    });
}

pub(crate) fn spawn_completion_rewrite_dispatch(
    rewrite: taurine_core::engine::CompletionRewrite,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    injector::spawn_guarded_injection_thread("tau-comp-rw", move || {
        dispatch_completion_rewrite_with(rewrite, spinner_style, crate::injector::inject_expansion);
    });
}

pub(crate) fn spawn_placeholder_injection_dispatch(
    placeholder: String,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    injector::spawn_guarded_injection_thread("tau-placeholder", move || {
        crate::injector::inject_expansion(
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                placeholder,
            )],
            0,
            spinner_style,
        );
    });
}

pub(super) fn dispatch_completion_rewrite_with<I>(
    rewrite: taurine_core::engine::CompletionRewrite,
    spinner_style: taurine_core::settings::SpinnerStyle,
    inject: I,
) where
    I: FnOnce(
        Vec<taurine_core::engine::variables::ExpansionStep>,
        usize,
        taurine_core::settings::SpinnerStyle,
    ) -> crate::injector::InjectionReport,
{
    let taurine_core::engine::CompletionRewrite {
        delete_count,
        replacement,
    } = rewrite;
    trace!(
        delete_count,
        replacement_chars = replacement.chars().count(),
        "Starting completion rewrite dispatch"
    );
    let injection = inject(
        vec![taurine_core::engine::variables::ExpansionStep::Text(
            replacement,
        )],
        delete_count,
        spinner_style,
    );
    trace!(
        delete_count,
        completed = injection.completed,
        output_chars = injection.successful_chars,
        "Finished completion rewrite dispatch"
    );
}

pub(super) fn dispatch_expansion_with<I, L>(
    expansion: taurine_core::engine::ExpansionResult,
    spinner_style: taurine_core::settings::SpinnerStyle,
    state: Arc<taurine_core::engine::EngineState>,
    inject_expansion: I,
    launch_follow_up_fn: L,
) where
    I: FnOnce(
        Vec<taurine_core::engine::variables::ExpansionStep>,
        usize,
        taurine_core::settings::SpinnerStyle,
    ) -> crate::injector::InjectionReport,
    L: FnOnce(
        Option<taurine_core::engine::ExpansionFollowUp>,
        taurine_core::settings::SpinnerStyle,
    ),
{
    let taurine_core::engine::ExpansionResult {
        delete_count,
        steps,
        trigger,
        undo_trigger,
        is_calculation,
        stat_kind,
        track_usage,
        follow_up,
    } = expansion;
    let step_count = steps.len();
    let has_follow_up = follow_up.is_some();

    state.clear_undo_state();

    if !has_follow_up && let Some(undo_trig) = &undo_trigger {
        let expected_output_chars: usize = steps
            .iter()
            .map(|s| {
                if let taurine_core::engine::variables::ExpansionStep::Text(t) = s {
                    t.chars().count()
                } else {
                    0
                }
            })
            .sum();
        if expected_output_chars > 0 {
            state.set_undo_state(undo_trig.clone(), expected_output_chars);
            let expanded_text = steps
                .iter()
                .filter_map(|s| {
                    if let taurine_core::engine::variables::ExpansionStep::Text(t) = s {
                        Some(t.clone())
                    } else {
                        None
                    }
                })
                .collect::<String>();
            state.register_case_cycle(expanded_text);
        }
    }

    trace!(
        delete_count,
        step_count, has_follow_up, track_usage, "Starting expansion dispatch"
    );
    let injection = inject_expansion(steps, delete_count, spinner_style);
    trace!(
        delete_count,
        step_count,
        completed = injection.completed,
        output_chars = injection.successful_chars,
        has_follow_up,
        "Finished expansion dispatch"
    );
    if follow_up.is_none()
        && injection.successful_chars > 0
        && let Some(undo_trigger) = undo_trigger
    {
        state.refresh_undo_state(&undo_trigger, injection.successful_chars);
    }
    launch_follow_up_fn(follow_up, spinner_style);

    if track_usage && stat_kind != taurine_core::db::crud::TriggerStatKind::InlineAi {
        let app = crate::platform::capture_active_app();
        taurine_core::db::crud::record_trigger_stat(taurine_core::db::crud::TriggerStatEvent {
            trigger: Some(trigger.clone()),
            trigger_chars: trigger.chars().count(),
            success: injection.completed,
            output_chars: injection.successful_chars,
            kind: if is_calculation {
                taurine_core::db::crud::TriggerStatKind::Calculation
            } else {
                stat_kind
            },
            wpm: None,
            app,
        });
    }
}

pub(super) fn launch_follow_up(
    follow_up: Option<taurine_core::engine::ExpansionFollowUp>,
    spinner_style: taurine_core::settings::SpinnerStyle,
) {
    let Some(runtime_handle) = crate::TOKIO_HANDLE.get().cloned() else {
        tracing::error!("Tokio runtime not initialized; skipping follow-up dispatch");
        return;
    };
    if let Some(taurine_core::engine::ExpansionFollowUp::InlineAi {
        prompt,
        system_prompt_override,
    }) = follow_up
    {
        debug!("Starting inline AI follow-up dispatch");
        let injection_guard = crate::injector::InjectionFlagGuard::begin();

        let spinner_handle = taurine_core::utils::spinner::spawn_async(
            spinner_style,
            OsSpinnerRenderer::default(),
            &runtime_handle,
        );
        runtime_handle.spawn(async move {
            let _guard = injection_guard;
            crate::engine::ai::stream::run_inline_ai_stream(
                prompt,
                system_prompt_override,
                spinner_handle,
            )
            .await;
            debug!("Finished inline AI follow-up dispatch");
        });
        return;
    }

    if let Some(taurine_core::engine::ExpansionFollowUp::AiTransformer {
        template_with_markers,
    }) = follow_up
    {
        debug!("Starting AI transformer follow-up dispatch");
        let injection_guard = crate::injector::InjectionFlagGuard::begin();

        let spinner_handle = taurine_core::utils::spinner::spawn_async(
            spinner_style,
            OsSpinnerRenderer::default(),
            &runtime_handle,
        );
        runtime_handle.spawn(async move {
            let _guard = injection_guard;
            crate::engine::ai::stream::run_ai_transformer_stream(
                template_with_markers,
                spinner_handle,
            )
            .await;
            debug!("Finished AI transformer follow-up dispatch");
        });
        return;
    }

    if let Some(taurine_core::engine::ExpansionFollowUp::DictionaryLookup { word, lookup_type }) =
        follow_up
    {
        debug!("Starting dictionary follow-up dispatch for word: {}", word);
        let injection_guard = crate::injector::InjectionFlagGuard::begin();

        let spinner_handle = taurine_core::utils::spinner::spawn_async(
            spinner_style,
            OsSpinnerRenderer::default(),
            &runtime_handle,
        );

        runtime_handle.spawn(async move {
            let _guard = injection_guard;
            let entries = taurine_core::engine::dictionary::lookup_word(&word).await;
            let _ = spinner_handle.cancel.send(());
            let _ = spinner_handle.task.await;

            let mut output = String::new();
            if let Some(entries) = entries {
                if entries.is_empty() {
                    output.push_str("no results found\n");
                } else {
                    let entry = &entries[0];
                    match lookup_type {
                        taurine_core::engine::dictionary::types::DictionaryLookupType::Meaning => {
                            output.push_str(&format!("{}:\n", entry.word));
                            for meaning in &entry.meanings {
                                if let Some(def) = meaning.definitions.first() {
                                    output.push_str(&format!(
                                        "  {}: {}\n",
                                        meaning.part_of_speech, def.definition
                                    ));
                                }
                            }
                        }
                        taurine_core::engine::dictionary::types::DictionaryLookupType::Synonyms => {
                            output.push_str(&format!("synonyms of {}:\n", entry.word));
                            let mut all_syns = Vec::new();
                            for meaning in &entry.meanings {
                                all_syns.extend(meaning.synonyms.clone());
                                for def in &meaning.definitions {
                                    all_syns.extend(def.synonyms.clone());
                                }
                            }
                            if all_syns.is_empty() {
                                output.push_str("    no synonyms found\n");
                            } else {
                                all_syns.sort();
                                all_syns.dedup();
                                output.push_str(&format!("    {}\n", all_syns.join(", ")));
                            }
                        }
                        taurine_core::engine::dictionary::types::DictionaryLookupType::Antonyms => {
                            output.push_str(&format!("antonyms of {}:\n", entry.word));
                            let mut all_ants = Vec::new();
                            for meaning in &entry.meanings {
                                all_ants.extend(meaning.antonyms.clone());
                                for def in &meaning.definitions {
                                    all_ants.extend(def.antonyms.clone());
                                }
                            }
                            if all_ants.is_empty() {
                                output.push_str("    no antonyms found\n");
                            } else {
                                all_ants.sort();
                                all_ants.dedup();
                                output.push_str(&format!("    {}\n", all_ants.join(", ")));
                            }
                        }
                    }
                }
            } else {
                output.push_str("no results found\n");
            }

            // Remove the trailing newline
            let final_output = output.trim_end().to_string();

            crate::injector::inject_text_segment(&final_output, &None);
            debug!("Finished dictionary follow-up dispatch");
        });
    }
}
