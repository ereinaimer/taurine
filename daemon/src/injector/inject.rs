use crate::platform::ClipboardManager;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;
use tracing::{debug, error, trace, warn};

use arboard::Clipboard;
use taurine_core::engine::shell::ScriptBehavior;
use taurine_core::engine::variables::ExpansionStep;

use super::clipboard::{prepare_clipboard_for_expansion, restore_clipboard};
use super::gate::{INJECTION_ABORT, InjectionFlagGuard, inject_mutex};
use super::simulate::{
    MouseButton, pre_release_modifiers, simulate_key_alias, simulate_mouse_click,
    simulate_mouse_hold, simulate_mouse_move, simulate_mouse_scroll,
};

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

#[allow(clippy::needless_return)]
pub fn inject_text_segment(
    text: &str,
    original_clipboard: &Option<String>,
) -> TextSegmentInjection {
    let injected_chars = text.chars().count();
    let delay_ms = taurine_core::settings::get_cached_clipboard_restore_delay();
    let post_paste_wait = Duration::from_millis(delay_ms as u64);

    #[cfg(windows)]
    {
        let mut clip = crate::platform::windows::WindowsClipboard;
        if original_clipboard.is_none() {
            // First text segment: save the original clipboard.
            match prepare_clipboard_for_expansion(&mut clip, text) {
                Ok(orig) => {
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                    return TextSegmentInjection {
                        original_clipboard: Some(orig),
                        injected_chars,
                        success: true,
                    };
                }
                Err(e) => {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    return TextSegmentInjection::default();
                }
            }
        } else {
            // Subsequent text segments: clipboard was already saved.
            if let Err(e) = clip.set_text(text) {
                error!("Failed to set clipboard for text segment: {}", e);
                return TextSegmentInjection {
                    original_clipboard: original_clipboard.clone(),
                    injected_chars: 0,
                    success: false,
                };
            }
            thread::sleep(Duration::from_millis(25));
            simulate_paste();
            thread::sleep(post_paste_wait);
            return TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars,
                success: true,
            };
        }
    }

    #[cfg(target_os = "linux")]
    {
        use crate::platform::linux;

        let has_display =
            std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok();
        let use_typing = !has_display;

        if !use_typing {
            let mut clipboard = linux::LinuxClipboard;
            if original_clipboard.is_none() {
                match prepare_clipboard_for_expansion(&mut clipboard, text) {
                    Ok(orig) => {
                        simulate_paste();
                        thread::sleep(post_paste_wait);
                        return TextSegmentInjection {
                            original_clipboard: Some(orig),
                            injected_chars,
                            success: true,
                        };
                    }
                    Err(e) => {
                        error!(
                            "Clipboard expansion failed (verify mismatch or permission issue: {}). Falling back to direct typing.",
                            e
                        );
                    }
                }
            } else {
                if let Err(e) = clipboard.set_text(text) {
                    error!("Failed to set clipboard for text segment: {}", e);
                    return TextSegmentInjection {
                        original_clipboard: original_clipboard.clone(),
                        injected_chars: 0,
                        success: false,
                    };
                } else {
                    thread::sleep(Duration::from_millis(25));
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                }
                return TextSegmentInjection {
                    original_clipboard: original_clipboard.clone(),
                    injected_chars,
                    success: true,
                };
            }
        }

        // At this point, we must use direct typing (either display-less or fallback).
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

    #[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "macos")))]
    {
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to initialize clipboard: {}", e);
                return TextSegmentInjection::default();
            }
        };

        if original_clipboard.is_none() {
            match prepare_clipboard_for_expansion(&mut clipboard, text) {
                Ok(orig) => {
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                    TextSegmentInjection {
                        original_clipboard: Some(orig),
                        injected_chars,
                        success: true,
                    }
                }
                Err(e) => {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    TextSegmentInjection::default()
                }
            }
        } else {
            if let Err(e) = clipboard.set_text(text) {
                error!("Failed to set clipboard for text segment: {}", e);
                return TextSegmentInjection {
                    original_clipboard: original_clipboard.clone(),
                    injected_chars: 0,
                    success: false,
                };
            }
            thread::sleep(Duration::from_millis(25));
            simulate_paste();
            thread::sleep(post_paste_wait);
            TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars,
                success: true,
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut clipboard = crate::platform::macos::clipboard::MacosClipboard;

        if original_clipboard.is_none() {
            match prepare_clipboard_for_expansion(&mut clipboard, text) {
                Ok(orig) => {
                    simulate_paste();
                    thread::sleep(post_paste_wait);
                    TextSegmentInjection {
                        original_clipboard: Some(orig),
                        injected_chars,
                        success: true,
                    }
                }
                Err(e) => {
                    if e.starts_with("clipboard verify failed:") {
                        warn!("Clipboard content mismatch before paste — {}. Skipping.", e);
                    } else {
                        error!("Could not prepare clipboard before paste: {}", e);
                    }
                    TextSegmentInjection::default()
                }
            }
        } else {
            if let Err(e) = clipboard.set_text(text) {
                error!("Failed to set clipboard for text segment: {}", e);
                return TextSegmentInjection {
                    original_clipboard: original_clipboard.clone(),
                    injected_chars: 0,
                    success: false,
                };
            }
            thread::sleep(Duration::from_millis(25));
            simulate_paste();
            thread::sleep(post_paste_wait);
            TextSegmentInjection {
                original_clipboard: original_clipboard.clone(),
                injected_chars,
                success: true,
            }
        }
    }
}

pub fn inject_undo(trigger_string: String, output_length: usize) {
    let _state_guard = InjectionFlagGuard::begin();
    let _inject_guard = inject_mutex().lock().expect("inject mutex poisoned");
    pre_release_modifiers();

    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(10));

    erase_trigger(output_length);

    #[cfg(target_os = "linux")]
    thread::sleep(Duration::from_millis(20));

    let original_clipboard = inject_text_segment(&trigger_string, &None);

    if let Some(ref original) = original_clipboard.original_clipboard {
        restore_clipboard(original);
    }

    pre_release_modifiers();
}

fn erase_trigger(delete_count: usize) {
    trace!("Injecting {} backspaces", delete_count);
    for _ in 0..delete_count {
        #[cfg(target_os = "linux")]
        {
            crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = super::simulate::simulate_monitored(&rdev::EventType::KeyPress(
                rdev::Key::Backspace,
            ));
            let _ = super::simulate::simulate_monitored(&rdev::EventType::KeyRelease(
                rdev::Key::Backspace,
            ));
        }
        thread::sleep(Duration::from_millis(3));
    }
}

fn simulate_paste() {
    #[cfg(target_os = "linux")]
    {
        crate::platform::linux::uinput::simulate_key(evdev::KeyCode::KEY_LEFTCTRL, true);
        crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_V);
        crate::platform::linux::uinput::simulate_key(evdev::KeyCode::KEY_LEFTCTRL, false);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let modifier = if cfg!(target_os = "macos") {
            rdev::Key::MetaLeft
        } else {
            rdev::Key::ControlLeft
        };
        let _ = super::simulate::simulate_monitored(&rdev::EventType::KeyPress(modifier));
        let _ = super::simulate::simulate_monitored(&rdev::EventType::KeyPress(rdev::Key::KeyV));
        let _ = super::simulate::simulate_monitored(&rdev::EventType::KeyRelease(rdev::Key::KeyV));
        let _ = super::simulate::simulate_monitored(&rdev::EventType::KeyRelease(modifier));
    }
}

pub struct StreamingTextSession {
    guard: Option<std::sync::MutexGuard<'static, ()>>,
    state_guard: Option<InjectionFlagGuard>,
    original_clipboard: Option<String>,
    tracked_chars: usize,
}

impl StreamingTextSession {
    pub fn begin() -> Self {
        let guard = inject_mutex().lock().expect("inject mutex poisoned");
        let state_guard = InjectionFlagGuard::begin();
        pre_release_modifiers();

        Self {
            guard: Some(guard),
            state_guard: Some(state_guard),
            original_clipboard: None,
            tracked_chars: 0,
        }
    }

    pub fn push_text(&mut self, text: &str, track_metrics: bool) -> bool {
        if text.is_empty() || INJECTION_ABORT.load(Ordering::SeqCst) {
            return !INJECTION_ABORT.load(Ordering::SeqCst);
        }

        let injection = inject_text_segment(text, &self.original_clipboard);
        if self.original_clipboard.is_none() {
            self.original_clipboard = injection.original_clipboard;
        }
        if track_metrics {
            self.tracked_chars = self.tracked_chars.saturating_add(injection.injected_chars);
        }

        !INJECTION_ABORT.load(Ordering::SeqCst)
    }

    pub fn abort_requested(&self) -> bool {
        INJECTION_ABORT.load(Ordering::SeqCst)
    }

    pub fn finish(&mut self) -> usize {
        let tracked_chars = self.tracked_chars;
        if let Some(ref original) = self.original_clipboard {
            restore_clipboard(original);
        }

        pre_release_modifiers();
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

pub fn inject_expansion(
    steps: Vec<ExpansionStep>,
    delete_count: usize,
    spinner_style: taurine_core::settings::SpinnerStyle,
) -> InjectionReport {
    let _state_guard = InjectionFlagGuard::begin();
    let _inject_guard = inject_mutex().lock().expect("inject mutex poisoned");

    // Pre-Release: neutralize modifier state before any injection.
    pre_release_modifiers();

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
        if INJECTION_ABORT.load(Ordering::SeqCst) {
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
                let injection = inject_text_segment(text, &original_clipboard);
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
            ExpansionStep::KeyPress(alias) => {
                if !simulate_key_alias(alias) {
                    debug!("Unknown key alias '{}', skipping", alias);
                }
            }
            ExpansionStep::MouseClick => {
                simulate_mouse_click(MouseButton::Left);
            }
            ExpansionStep::MouseRClick => {
                simulate_mouse_click(MouseButton::Right);
            }
            ExpansionStep::MouseMClick => {
                simulate_mouse_click(MouseButton::Middle);
            }
            ExpansionStep::MouseMove(x, y) => {
                simulate_mouse_move(*x, *y);
            }
            ExpansionStep::MouseScroll(delta) => {
                simulate_mouse_scroll(*delta);
            }
            ExpansionStep::MouseHold => {
                simulate_mouse_hold(MouseButton::Left, true);
            }
            ExpansionStep::MouseRelease => {
                simulate_mouse_hold(MouseButton::Left, false);
            }
            ExpansionStep::Delay(ms) => {
                // Split long delays into smaller chunks so abort is responsive.
                let mut remaining = *ms;
                while remaining > 0 {
                    if INJECTION_ABORT.load(Ordering::SeqCst) {
                        report.completed = false;
                        break;
                    }
                    let chunk = remaining.min(50);
                    thread::sleep(Duration::from_millis(chunk));
                    remaining -= chunk;
                }
            }
            ExpansionStep::Script(metadata) => {
                match metadata.behavior {
                    ScriptBehavior::Inline => {
                        // Start the modern Braille spinner from core
                        let spinner_handle = taurine_core::utils::spinner::spawn_threaded(
                            spinner_style,
                            crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
                        );

                        // Execute script and block until completion (or abort/timeout)
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .expect("Failed to initialize script runtime");

                        let script_result: taurine_core::Result<String> =
                            rt.block_on(crate::platform::executor::execute_script(metadata));

                        // Stop the spinner
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
                                // Silent abort: if the user killed it, don't paste an error.
                                let err_str = e.to_string();
                                if !err_str.contains("aborted by user") {
                                    let err_msg = format!(" [Error: {}] ", e);
                                    let injection =
                                        inject_text_segment(&err_msg, &original_clipboard);
                                    if original_clipboard.is_none() {
                                        original_clipboard = injection.original_clipboard;
                                    }
                                }
                            }
                        }
                    }
                    ScriptBehavior::Silent => {
                        // Fire and forget in the background
                        let metadata_clone = metadata.clone();
                        let spawn_res = thread::Builder::new()
                            .name("tau-script-bg".to_string())
                            .spawn(move || {
                                if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                                    .enable_all()
                                    .build()
                                {
                                    let _ = rt.block_on(crate::platform::executor::execute_script(
                                        &metadata_clone,
                                    ));
                                }
                            });
                        if let Err(e) = spawn_res {
                            error!("Failed to spawn background script thread: {}", e);
                        }
                    }
                }
            }
            ExpansionStep::InlineRun(metadata, transformers) => match metadata.behavior {
                ScriptBehavior::Inline => {
                    let spinner_handle = taurine_core::utils::spinner::spawn_threaded(
                        spinner_style,
                        crate::platform::spinner_renderer::OsSpinnerRenderer::default(),
                    );

                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .expect("Failed to initialize inline run runtime");

                    let mut script_result: taurine_core::Result<String> =
                        rt.block_on(crate::platform::executor::execute_script(metadata));

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
                    let metadata_clone = metadata.clone();
                    let spawn_res = thread::Builder::new()
                        .name("tau-script-bg".to_string())
                        .spawn(move || {
                            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                            {
                                let _ = rt.block_on(crate::platform::executor::execute_script(
                                    &metadata_clone,
                                ));
                            }
                        });
                    if let Err(e) = spawn_res {
                        error!("Failed to spawn background script thread: {}", e);
                    }
                }
            },
        }
    }

    // 3. Restore the user's original clipboard (if we touched it).
    if let Some(ref original) = original_clipboard {
        restore_clipboard(original);
    }

    // Panic Release: ensure all modifiers are logically released.
    pre_release_modifiers();
    report
}
