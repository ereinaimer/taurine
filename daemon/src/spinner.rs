use crate::injector::INJECTION_ABORT;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use taurine_core::settings::SpinnerStyle;

const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const ARC_FRAMES: &[&str] = &["◜", "◠", "◝", "◞", "◡", "◟"];
const CLASSIC_FRAMES: &[&str] = &["|", "/", "-", "\\"];

pub struct SpinnerHandle {
    abort: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl SpinnerHandle {
    pub fn stop(mut self) {
        self.abort.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn start(style: SpinnerStyle) -> SpinnerHandle {
    let abort = Arc::new(AtomicBool::new(false));
    let abort_clone = abort.clone();

    let frames = match style {
        SpinnerStyle::Braille => BRAILLE_FRAMES,
        SpinnerStyle::Arc => ARC_FRAMES,
        SpinnerStyle::Classic => CLASSIC_FRAMES,
    };

    let thread = thread::spawn(move || {
        let mut idx = 0;

        while !abort_clone.load(Ordering::SeqCst) && !INJECTION_ABORT.load(Ordering::SeqCst) {
            let frame = frames[idx % frames.len()];

            // To avoid flickering and clipboard mess, I'll use a direct injection
            // if we have one. For now, I'll use the existing injector::inject_text_segment
            // but I should probably add a focused version.
            #[cfg(not(target_os = "linux"))]
            {
                crate::injector::inject_text_segment(frame, &None);
            }
            #[cfg(target_os = "linux")]
            {
                // Linux fallback
            }

            thread::sleep(Duration::from_millis(60));

            // Backspace to clear frame
            #[cfg(not(target_os = "linux"))]
            {
                use rdev::EventType;
                let _ =
                    crate::injector::simulate_monitored(&EventType::KeyPress(rdev::Key::Backspace));
                let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(
                    rdev::Key::Backspace,
                ));
            }
            #[cfg(target_os = "linux")]
            {
                crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
            }

            idx += 1;
        }

        // Final cleanup: ensure the last spinner char is backspaced if we exited early
        #[cfg(not(target_os = "linux"))]
        {
            use rdev::EventType;
            let _ = crate::injector::simulate_monitored(&EventType::KeyPress(rdev::Key::Backspace));
            let _ =
                crate::injector::simulate_monitored(&EventType::KeyRelease(rdev::Key::Backspace));
        }
        #[cfg(target_os = "linux")]
        {
            crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
        }
    });

    SpinnerHandle {
        abort,
        thread: Some(thread),
    }
}
