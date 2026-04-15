use crate::injector::INJECTION_ABORT;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

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

pub fn start() -> SpinnerHandle {
    let abort = Arc::new(AtomicBool::new(false));
    let abort_clone = abort.clone();

    let thread = thread::spawn(move || {
        let mut idx = 0;

        while !abort_clone.load(Ordering::SeqCst) && !INJECTION_ABORT.load(Ordering::SeqCst) {
            let frame = FRAMES[idx % FRAMES.len()];

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

            thread::sleep(Duration::from_millis(80));

            // Backspace to clear frame
            #[cfg(not(target_os = "linux"))]
            {
                use rdev::{EventType, Key, simulate};
                let _ = simulate(&EventType::KeyPress(Key::Backspace));
                let _ = simulate(&EventType::KeyRelease(Key::Backspace));
            }
            #[cfg(target_os = "linux")]
            {
                crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
            }

            idx += 1;
        }
    });

    SpinnerHandle {
        abort,
        thread: Some(thread),
    }
}
