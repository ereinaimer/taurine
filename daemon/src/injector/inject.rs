use crate::platform::{ClipboardManager, MouseButton};
use std::thread;
use std::time::Duration;
#[cfg(not(target_os = "linux"))]
use tracing::warn;
use tracing::{debug, error, trace};

use taurine_core::engine::shell::ScriptBehavior;
use taurine_core::engine::variables::ExpansionStep;

use super::clipboard::{prepare_clipboard_for_expansion, restore_clipboard};
use super::gate::{InjectionFlagGuard, capture_generation, inject_mutex, is_aborted};

#[cfg(target_os = "linux")]
const INTER_STEP_DELAY_MS: u64 = 15;
#[cfg(not(target_os = "linux"))]
const INTER_STEP_DELAY_MS: u64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InjectionReport {
    pub successful_chars: usize,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextSegmentInjection {
    pub original_clipboard: Option<String>,
    pub injected_chars: usize,
    pub success: bool,
}

#[cfg(target_os = "linux")]
fn linux_direct_typing(
    text: &str,
    original_clipboard: &Option<String>,
    injected_chars: usize,
) -> TextSegmentInjection {
    use crate::platform::linux;
    if let Some(lookup) = linux::get_reverse_lookup() {
        linux::uinput::simulate_type_string(text, lookup);
        TextSegmentInjection {
            original_clipboard: original_clipboard.clone(),
            injected_chars,
            success: true,
        }
    } else {
        error!("Direct typing failed: Linux XKB mapper not initialized");
        TextSegmentInjection {
            original_clipboard: original_clipboard.clone(),
            injected_chars: 0,
            success: false,
        }
    }
}

/// Sleeps `dur`, checking for abort every 10ms so the thread responds
/// promptly when the injection generation advances.
fn abortable_sleep(dur: Duration, captured_gen: u64) {
    let mut remaining = dur.as_millis() as u64;
    while remaining > 0 {
        if is_aborted(captured_gen) {
            return;
        }
        let chunk = remaining.min(10);
        thread::sleep(Duration::from_millis(chunk));
        remaining -= chunk;
    }
}

#[allow(clippy::needless_return)]
pub fn inject_text_segment(
    text: &str,
    original_clipboard: &Option<String>,
) -> TextSegmentInjection {
    inject_text_segment_with_gen(text, original_clipboard, 0)
}

#[allow(clippy::needless_return)]
fn inject_text_segment_with_gen(
    text: &str,
    original_clipboard: &Option<String>,
    captured_gen: u64,
) -> TextSegmentInjection {
    let injected_chars = text.chars().count();
    let delay_ms = taurine_core::settings::get_cached_clipboard_restore_delay();
    let post_paste_wait = Duration::from_millis(delay_ms as u64);

    // On Linux, check if we must use direct typing (no display)
    #[cfg(target_os = "linux")]
    {
        let has_display =
            std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok();
        if !has_display {
            return linux_direct_typing(text, original_clipboard, injected_chars);
        }
    }

    // Normal clipboard-based injection
    let mut clipboard = match crate::platform::get_clipboard_manager() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to initialize clipboard: {}", e);
            // On Linux, fall back to direct typing if clipboard fails
            #[cfg(target_os = "linux")]
            {
                return linux_direct_typing(text, original_clipboard, injected_chars);
            }
            #[cfg(not(target_os = "linux"))]
            {
                return TextSegmentInjection::default();
            }
        }
    };

    if original_clipboard.is_none() {
        match prepare_clipboard_for_expansion(&mut clipboard, text, captured_gen) {
            Ok(orig) => {
                crate::platform::get_injector().simulate_paste();
                if captured_gen != 0 {
                    abortable_sleep(post_paste_wait, captured_gen);
                } else {
                    thread::sleep(post_paste_wait);
                }
                return TextSegmentInjection {
                    original_clipboard: Some(orig),
                    injected_chars,
                    success: true,
                };
            }
            Err(e) => {
                #[cfg(target_os = "linux")]
                {
                    error!(
                        "Clipboard expansion failed (verify mismatch or permission issue: {}). Falling back to direct typing.",
                        e
                    );
                    return linux_direct_typing(text, original_clipboard, injected_chars);
                }
                #[cfg(not(target_os = "linux"))]
                {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    return TextSegmentInjection::default();
                }
            }
        }
    } else {
        let is_html = taurine_core::utils::html::has_html_tags(text);
        let set_res = if is_html {
            let plaintext = taurine_core::utils::html::strip_html(text);
            clipboard.set_html(text, &plaintext)
        } else {
            clipboard.set_text(text)
        };
        if let Err(e) = set_res {
            error!("Failed to set clipboard for text segment: {}", e);
            return TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars: 0,
                success: false,
            };
        }
        thread::sleep(Duration::from_millis(25));
        crate::platform::get_injector().simulate_paste();
        if captured_gen != 0 {
            abortable_sleep(post_paste_wait, captured_gen);
        } else {
            thread::sleep(post_paste_wait);
        }
        return TextSegmentInjection {
            original_clipboard: original_clipboard.clone(),
            injected_chars,
            success: true,
        };
    }
}

fn inject_image_segment_with_gen(
    bytes: &[u8],
    mime_type: &str,
    original_clipboard: &Option<String>,
    captured_gen: u64,
) -> TextSegmentInjection {
    let delay_ms = taurine_core::settings::get_cached_clipboard_restore_delay();
    let post_paste_wait = Duration::from_millis(delay_ms as u64);

    let mut clipboard = match crate::platform::get_clipboard_manager() {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to initialize clipboard for image injection: {}", e);
            return TextSegmentInjection::default();
        }
    };

    let orig = if let Some(orig) = original_clipboard {
        orig.clone()
    } else {
        clipboard.get_text().unwrap_or_default()
    };

    let ext = match mime_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        _ => "png",
    };

    let temp_path = match taurine_core::system::paths::write_temp_file("tau_img", ext, bytes) {
        Ok(path) => path,
        Err(e) => {
            error!("Failed to write temporary image file: {}", e);
            return TextSegmentInjection {
                original_clipboard: Some(orig),
                injected_chars: 0,
                success: false,
            };
        }
    };

    if let Err(e) = clipboard.set_image_file(&temp_path) {
        error!("Failed to set clipboard image file: {}", e);
        let _ = std::fs::remove_file(&temp_path);
        return TextSegmentInjection {
            original_clipboard: Some(orig),
            injected_chars: 0,
            success: false,
        };
    }

    thread::sleep(Duration::from_millis(50));
    crate::platform::get_injector().simulate_paste();
    if captured_gen != 0 {
        abortable_sleep(post_paste_wait, captured_gen);
    } else {
        thread::sleep(post_paste_wait);
    }

    TextSegmentInjection {
        original_clipboard: Some(orig),
        injected_chars: 1,
        success: true,
    }
}

pub fn inject_undo(trigger_string: String, output_length: usize) {
    let _state_guard = InjectionFlagGuard::begin();
    let _inject_guard = match inject_mutex().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("inject mutex poisoned; recovering");
            poisoned.into_inner()
        }
    };
    crate::platform::get_injector().pre_release_modifiers();

    // Fast-path atomic single-batch backspaces and direct unicode restoration
    crate::platform::get_injector().inject_atomic_undo(output_length, &trigger_string);

    crate::platform::get_injector().pre_release_modifiers();
}

fn erase_trigger(delete_count: usize) {
    trace!("Injecting {} backspaces", delete_count);
    crate::platform::get_injector().inject_atomic_backspaces(delete_count);
}

pub struct StreamingTextSession {
    guard: Option<std::sync::MutexGuard<'static, ()>>,
    state_guard: Option<InjectionFlagGuard>,
    original_clipboard: Option<String>,
    tracked_chars: usize,
    captured_gen: u64,
}

impl StreamingTextSession {
    pub fn begin() -> Self {
        let guard = match inject_mutex().lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("inject mutex poisoned; recovering");
                poisoned.into_inner()
            }
        };
        let state_guard = InjectionFlagGuard::begin();
        crate::platform::get_injector().pre_release_modifiers();

        Self {
            guard: Some(guard),
            state_guard: Some(state_guard),
            original_clipboard: None,
            tracked_chars: 0,
            captured_gen: capture_generation(),
        }
    }

    pub fn push_text(&mut self, text: &str, track_stats: bool) -> bool {
        if text.is_empty() || is_aborted(self.captured_gen) {
            return !is_aborted(self.captured_gen);
        }

        let injection =
            inject_text_segment_with_gen(text, &self.original_clipboard, self.captured_gen);
        if self.original_clipboard.is_none() {
            self.original_clipboard = injection.original_clipboard;
        }
        if track_stats {
            self.tracked_chars = self.tracked_chars.saturating_add(injection.injected_chars);
        }

        !is_aborted(self.captured_gen)
    }

    pub fn abort_requested(&self) -> bool {
        is_aborted(self.captured_gen)
    }

    pub fn finish(&mut self) -> usize {
        let tracked_chars = self.tracked_chars;
        if let Some(ref original) = self.original_clipboard {
            restore_clipboard(original, self.captured_gen);
        }

        crate::platform::get_injector().pre_release_modifiers();
        self.original_clipboard = None;
        self.tracked_chars = 0;
        self.guard.take();
        self.state_guard.take();
        tracked_chars
    }
}

impl Drop for StreamingTextSession {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

pub(super) fn expansion_requires_keystrokes(steps: &[ExpansionStep], delete_count: usize) -> bool {
    if delete_count > 0 {
        return true;
    }
    steps.iter().any(|step| match step {
        ExpansionStep::Text(text) => !text.is_empty(),
        ExpansionStep::Image(_, _)
        | ExpansionStep::KeyPress(_)
        | ExpansionStep::MouseClick
        | ExpansionStep::MouseRClick
        | ExpansionStep::MouseMClick
        | ExpansionStep::MouseMove(_, _)
        | ExpansionStep::MouseScroll(_)
        | ExpansionStep::MouseHold
        | ExpansionStep::MouseRelease => true,
        ExpansionStep::Script(metadata) => metadata.behavior == ScriptBehavior::Inline,
        ExpansionStep::InlineRun(metadata, _) => metadata.behavior == ScriptBehavior::Inline,
        ExpansionStep::Delay(_) => false,
    })
}

pub fn inject_expansion(
    steps: Vec<ExpansionStep>,
    delete_count: usize,
    spinner_style: taurine_core::settings::SpinnerStyle,
) -> InjectionReport {
    let requires_keys = expansion_requires_keystrokes(&steps, delete_count);
    let _state_guard = if requires_keys {
        Some(InjectionFlagGuard::begin())
    } else {
        None
    };
    let _inject_guard = match inject_mutex().lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("inject mutex poisoned; recovering");
            poisoned.into_inner()
        }
    };
    let captured_gen = capture_generation();

    // Pre-Release: neutralize modifier state only when simulated keypresses or backspaces will occur.
    if requires_keys {
        crate::platform::get_injector().pre_release_modifiers();
    }

    // Fast-Path: Single-segment plain text <= 1000 characters without newlines or tabs bypassing clipboard
    if let [ExpansionStep::Text(text)] = steps.as_slice()
        && text.chars().count() <= 1000
        && !text.contains('\n')
        && !text.contains('\r')
        && !text.contains('\t')
        && !taurine_core::utils::html::has_html_tags(text)
    {
        let success =
            crate::platform::get_injector().inject_atomic_text_expansion(delete_count, text);
        return InjectionReport {
            successful_chars: if success { text.chars().count() } else { 0 },
            completed: success,
        };
    }

    // On Linux, give the OS a moment to register the released modifiers before typing starts.
    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(10));

    // 1. Erase the trigger.
    erase_trigger(delete_count);

    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(20));

    // 2. Execute each step in sequence.
    let mut original_clipboard: Option<String> = None;
    let mut report = InjectionReport {
        successful_chars: 0,
        completed: true,
    };

    for (i, step) in steps.iter().enumerate() {
        // Check for user-initiated abort before each step.
        if is_aborted(captured_gen) {
            debug!("Injection aborted by physical keypress at step {}", i);
            report.completed = false;
            break;
        }

        // Implicit inter-step delay (skip before the very first step).
        if i > 0 {
            thread::sleep(Duration::from_millis(INTER_STEP_DELAY_MS));
        }

        match step {
            ExpansionStep::Text(text) => {
                let injection =
                    inject_text_segment_with_gen(text, &original_clipboard, captured_gen);
                report.successful_chars = report
                    .successful_chars
                    .saturating_add(injection.injected_chars);
                if original_clipboard.is_none() {
                    original_clipboard = injection.original_clipboard;
                }
                if !injection.success && original_clipboard.is_none() {
                    // First text segment failed entirely — abort.
                    report.completed = false;
                    break;
                }
                if !injection.success {
                    report.completed = false;
                }
            }
            ExpansionStep::Image(bytes, mime_type) => {
                let injection = inject_image_segment_with_gen(
                    bytes,
                    mime_type,
                    &original_clipboard,
                    captured_gen,
                );
                report.successful_chars = report
                    .successful_chars
                    .saturating_add(injection.injected_chars);
                if original_clipboard.is_none() {
                    original_clipboard = injection.original_clipboard;
                }
                if !injection.success && original_clipboard.is_none() {
                    report.completed = false;
                    break;
                }
                if !injection.success {
                    report.completed = false;
                }
            }
            ExpansionStep::KeyPress(alias) => {
                if !crate::platform::get_injector().simulate_key_alias(alias) {
                    debug!("Unknown key alias '{}', skipping", alias);
                }
            }
            ExpansionStep::MouseClick => {
                crate::platform::get_injector().simulate_mouse_click(MouseButton::Left);
            }
            ExpansionStep::MouseRClick => {
                crate::platform::get_injector().simulate_mouse_click(MouseButton::Right);
            }
            ExpansionStep::MouseMClick => {
                crate::platform::get_injector().simulate_mouse_click(MouseButton::Middle);
            }
            ExpansionStep::MouseMove(x, y) => {
                crate::platform::get_injector().simulate_mouse_move(*x, *y);
            }
            ExpansionStep::MouseScroll(delta) => {
                crate::platform::get_injector().simulate_mouse_scroll(*delta);
            }
            ExpansionStep::MouseHold => {
                crate::platform::get_injector().simulate_mouse_hold(MouseButton::Left, true);
            }
            ExpansionStep::MouseRelease => {
                crate::platform::get_injector().simulate_mouse_hold(MouseButton::Left, false);
            }
            ExpansionStep::Delay(ms) => {
                // Split long delays into smaller chunks so abort is responsive.
                let mut remaining = *ms;
                while remaining > 0 {
                    if is_aborted(captured_gen) {
                        report.completed = false;
                        break;
                    }
                    let chunk = remaining.min(50);
                    thread::sleep(Duration::from_millis(chunk));
                    remaining -= chunk;
                }
            }
            ExpansionStep::Script(metadata) => match metadata.behavior {
                ScriptBehavior::Inline => {
                    let spinner_handle = taurine_core::utils::spinner::spawn_threaded(
                        spinner_style,
                        crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
                    );

                    let script_result = run_script_sync(metadata);
                    spinner_handle.stop();

                    match script_result {
                        Ok(output) => {
                            let out: String = output;
                            if !out.is_empty() {
                                let injection = inject_text_segment(&out, &original_clipboard);
                                report.successful_chars = report
                                    .successful_chars
                                    .saturating_add(injection.injected_chars);
                                if original_clipboard.is_none() {
                                    original_clipboard = injection.original_clipboard;
                                }
                                if !injection.success {
                                    report.completed = false;
                                }
                            }
                        }
                        Err(e) => {
                            report.completed = false;
                            let err_str = e.to_string();
                            if !err_str.contains("aborted by user") {
                                let err_msg = format!(" [Error: {}] ", e);
                                let injection = inject_text_segment(&err_msg, &original_clipboard);
                                if original_clipboard.is_none() {
                                    original_clipboard = injection.original_clipboard;
                                }
                            }
                        }
                    }
                }
                ScriptBehavior::Silent => {
                    spawn_script_bg(metadata.clone());
                }
            },
            ExpansionStep::InlineRun(metadata, transformers) => match metadata.behavior {
                ScriptBehavior::Inline => {
                    let spinner_handle = taurine_core::utils::spinner::spawn_threaded(
                        spinner_style,
                        crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
                    );

                    let mut script_result = run_script_sync(metadata);
                    spinner_handle.stop();

                    if let Ok(ref mut output) = script_result {
                        for tr in transformers {
                            if let Some(transformed) =
                                taurine_core::engine::variables::system::transformers::apply(
                                    tr, output,
                                )
                            {
                                *output = transformed;
                            }
                        }
                    }

                    match script_result {
                        Ok(output) => {
                            if !output.is_empty() {
                                let injection = inject_text_segment(&output, &original_clipboard);
                                report.successful_chars = report
                                    .successful_chars
                                    .saturating_add(injection.injected_chars);
                                if original_clipboard.is_none() {
                                    original_clipboard = injection.original_clipboard;
                                }
                                if !injection.success {
                                    report.completed = false;
                                }
                            }
                        }
                        Err(e) => {
                            report.completed = false;
                            let err_str = e.to_string();
                            if !err_str.contains("aborted by user") {
                                let err_msg = format!("[Error: {}]", e);
                                let injection = inject_text_segment(&err_msg, &original_clipboard);
                                if original_clipboard.is_none() {
                                    original_clipboard = injection.original_clipboard;
                                }
                            }
                        }
                    }
                }
                ScriptBehavior::Silent => {
                    spawn_script_bg(metadata.clone());
                }
            },
        }
    }

    // 3. Restore the user's original clipboard (if we touched it).
    if let Some(ref original) = original_clipboard {
        restore_clipboard(original, captured_gen);
    }

    // Panic Release: ensure all modifiers are logically released if keystrokes were involved.
    if requires_keys {
        crate::platform::get_injector().pre_release_modifiers();
    }
    report
}

fn run_script_sync(
    metadata: &taurine_core::engine::shell::ScriptMetadata,
) -> taurine_core::Result<String> {
    if let Some(handle) = crate::TOKIO_HANDLE.get() {
        tokio::task::block_in_place(|| {
            handle.block_on(crate::platform::executor::execute_script(metadata))
        })
    } else {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt.block_on(crate::platform::executor::execute_script(metadata)),
            Err(e) => Err(taurine_core::Error::Service(e.to_string())),
        }
    }
}

fn spawn_script_bg(metadata: taurine_core::engine::shell::ScriptMetadata) {
    if let Some(handle) = crate::TOKIO_HANDLE.get() {
        handle.spawn(async move {
            let _ = crate::platform::executor::execute_script(&metadata).await;
        });
    } else {
        let spawn_res = thread::Builder::new()
            .name("tau-script-bg".to_string())
            .spawn(move || {
                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    let _ = rt.block_on(crate::platform::executor::execute_script(&metadata));
                }
            });
        if let Err(e) = spawn_res {
            tracing::error!("Failed to spawn background script thread: {}", e);
        }
    }
}
