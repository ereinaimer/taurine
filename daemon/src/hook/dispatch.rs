use std::sync::{Arc, Mutex};
use tokio::runtime::Handle;
use tracing::{info, trace};

use crate::injector;
use crate::platform::spinner_renderer::OsSpinnerRenderer;
use taurine_core::engine::Evaluator;

#[cfg(not(target_os = "linux"))]
pub(super) fn clear_undo_state(evaluator: &Arc<Mutex<Evaluator>>) {
    let _ = super::listener::with_evaluator_lock(evaluator, "clear_undo_state", |lock| {
        lock.state.clear_undo_state();
    });
}

#[cfg(not(target_os = "linux"))]
pub(super) fn take_active_undo_state(evaluator: &Arc<Mutex<Evaluator>>) -> Option<(String, usize)> {
    super::listener::with_evaluator_lock(evaluator, "take_active_undo_state", |lock| {
        lock.state
            .take_active_undo_state()
            .map(|undo| (undo.trigger_string, undo.output_length))
    })
    .flatten()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn spawn_undo_dispatch(trigger_string: String, output_length: usize) {
    injector::spawn_guarded_injection_thread("taurine-undo-dispatch", move || {
        injector::inject_undo(trigger_string, output_length);
    });
}

pub(crate) fn spawn_expansion_dispatch(
    expansion: taurine_core::engine::ExpansionResult,
    spinner_style: taurine_core::settings::SpinnerStyle,
    runtime_handle: Handle,
    state: Arc<taurine_core::engine::EngineState>,
) {
    injector::spawn_guarded_injection_thread("taurine-expansion-dispatch", move || {
        dispatch_expansion_with(
            expansion,
            spinner_style,
            runtime_handle,
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
    injector::spawn_guarded_injection_thread("taurine-completion-rewrite", move || {
        dispatch_completion_rewrite_with(rewrite, spinner_style, crate::injector::inject_expansion);
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
    runtime_handle: Handle,
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
        Handle,
    ),
{
    let taurine_core::engine::ExpansionResult {
        delete_count,
        steps,
        trigger,
        undo_trigger,
        is_calculation,
        metric_kind,
        track_usage,
        follow_up,
    } = expansion;
    let step_count = steps.len();
    let has_follow_up = follow_up.is_some();

    state.clear_undo_state();
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
    if track_usage && delete_count > 0 && (injection.completed || injection.successful_chars > 0) {
        state.record_word_trigger_usage(&trigger);
    }
    if follow_up.is_none()
        && injection.successful_chars > 0
        && let Some(undo_trigger) = undo_trigger
    {
        state.set_undo_state(undo_trigger, injection.successful_chars);
    }
    launch_follow_up_fn(follow_up, spinner_style, runtime_handle);

    if track_usage {
        taurine_core::db::crud::record_automation_metric(
            taurine_core::db::crud::AutomationMetricEvent {
                automation_trigger: Some(trigger.clone()),
                trigger_chars: trigger.chars().count(),
                success: injection.completed,
                output_chars: injection.successful_chars,
                kind: if is_calculation {
                    taurine_core::db::crud::AutomationMetricKind::Calculation
                } else {
                    metric_kind
                },
                wpm: None,
            },
        );
    }
}

pub(super) fn launch_follow_up(
    follow_up: Option<taurine_core::engine::ExpansionFollowUp>,
    spinner_style: taurine_core::settings::SpinnerStyle,
    runtime_handle: Handle,
) {
    if let Some(taurine_core::engine::ExpansionFollowUp::InlineAi {
        prompt,
        system_prompt_override,
    }) = follow_up
    {
        info!("Starting inline AI follow-up dispatch");
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
            info!("Finished inline AI follow-up dispatch");
        });
        return;
    }

    if let Some(taurine_core::engine::ExpansionFollowUp::AiTransformer {
        template_with_markers,
    }) = follow_up
    {
        info!("Starting AI transformer follow-up dispatch");
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
            info!("Finished AI transformer follow-up dispatch");
        });
    }
}
