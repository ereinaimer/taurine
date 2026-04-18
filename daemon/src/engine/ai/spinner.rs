use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::oneshot;

use crate::engine::ai::InlineAiSpinnerHandle;

const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const AI_SPINNER_TICK_MS: u64 = 80;
const AI_SPINNER_SUFFIX_LEN: usize = 9;
const AI_SPINNER_TOTAL_LEN: usize = 10;

trait SpinnerRenderer {
    fn move_left(&mut self, count: usize);
    fn move_right(&mut self, count: usize);
    fn backspace(&mut self, count: usize);
    fn inject_frame(&mut self, frame: &str);
    fn finish(&mut self);
}

#[derive(Default)]
struct OsSpinnerRenderer {
    original_clipboard: Option<String>,
}

impl SpinnerRenderer for OsSpinnerRenderer {
    fn move_left(&mut self, count: usize) {
        with_hidden_spinner_input(|| simulate_left(count));
    }

    fn move_right(&mut self, count: usize) {
        with_hidden_spinner_input(|| simulate_right(count));
    }

    fn backspace(&mut self, count: usize) {
        with_hidden_spinner_input(|| simulate_backspace(count));
    }

    fn inject_frame(&mut self, frame: &str) {
        if try_inject_frame_raw(frame) {
            return;
        }

        let original = with_hidden_spinner_input(|| {
            crate::injector::inject_text_segment(frame, &self.original_clipboard)
        });
        if self.original_clipboard.is_none() {
            self.original_clipboard = original;
        }
    }

    fn finish(&mut self) {
        if let Some(original) = self.original_clipboard.take() {
            crate::injector::restore_clipboard_text(&original);
        }
    }
}

pub fn spawn(runtime: &Handle) -> InlineAiSpinnerHandle {
    let (cancel, cancel_rx) = oneshot::channel();
    let task = runtime.spawn(async move {
        run_spinner_loop(OsSpinnerRenderer::default(), cancel_rx).await;
    });

    InlineAiSpinnerHandle { cancel, task }
}

async fn run_spinner_loop<R: SpinnerRenderer>(
    mut renderer: R,
    mut cancel_rx: oneshot::Receiver<()>,
) {
    renderer.move_left(AI_SPINNER_SUFFIX_LEN);

    let mut frame_index = 1usize;
    let timer = tokio::time::sleep(Duration::from_millis(AI_SPINNER_TICK_MS));
    tokio::pin!(timer);

    loop {
        tokio::select! {
            _ = &mut cancel_rx => {
                renderer.move_right(AI_SPINNER_SUFFIX_LEN);
                renderer.backspace(AI_SPINNER_TOTAL_LEN);
                renderer.finish();
                break;
            }
            _ = &mut timer => {
                renderer.backspace(1);
                renderer.inject_frame(BRAILLE_FRAMES[frame_index % BRAILLE_FRAMES.len()]);
                frame_index += 1;
                timer
                    .as_mut()
                    .reset(tokio::time::Instant::now() + Duration::from_millis(AI_SPINNER_TICK_MS));
            }
        }
    }
}

fn with_hidden_spinner_input<T>(action: impl FnOnce() -> T) -> T {
    crate::injector::IS_INJECTING.store(true, Ordering::SeqCst);
    let result = action();
    crate::injector::IS_INJECTING.store(false, Ordering::SeqCst);
    result
}

#[cfg(not(target_os = "linux"))]
fn simulate_left(count: usize) {
    use rdev::{EventType, Key};

    for _ in 0..count {
        let _ = crate::injector::simulate_monitored(&EventType::KeyPress(Key::LeftArrow));
        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(Key::LeftArrow));
    }
}

#[cfg(target_os = "linux")]
fn simulate_left(count: usize) {
    for _ in 0..count {
        crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_LEFT);
    }
}

#[cfg(not(target_os = "linux"))]
fn simulate_right(count: usize) {
    use rdev::{EventType, Key};

    for _ in 0..count {
        let _ = crate::injector::simulate_monitored(&EventType::KeyPress(Key::RightArrow));
        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(Key::RightArrow));
    }
}

#[cfg(target_os = "linux")]
fn simulate_right(count: usize) {
    for _ in 0..count {
        crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_RIGHT);
    }
}

#[cfg(not(target_os = "linux"))]
fn simulate_backspace(count: usize) {
    use rdev::{EventType, Key};

    for _ in 0..count {
        let _ = crate::injector::simulate_monitored(&EventType::KeyPress(Key::Backspace));
        let _ = crate::injector::simulate_monitored(&EventType::KeyRelease(Key::Backspace));
    }
}

#[cfg(target_os = "linux")]
fn simulate_backspace(count: usize) {
    for _ in 0..count {
        crate::platform::linux::uinput::simulate_keypress(evdev::KeyCode::KEY_BACKSPACE);
    }
}

#[cfg(not(target_os = "linux"))]
fn try_inject_frame_raw(_frame: &str) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn try_inject_frame_raw(frame: &str) -> bool {
    let Some(c) = frame.chars().next() else {
        return false;
    };
    let Some(lookup) = crate::platform::linux::get_reverse_lookup() else {
        return false;
    };
    if frame.chars().count() == 1 && lookup.contains_key(&c) {
        crate::platform::linux::uinput::simulate_type_string(frame, lookup);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct RecordingRenderer {
        ops: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingRenderer {
        fn snapshot(&self) -> Vec<String> {
            self.ops.lock().expect("renderer poisoned").clone()
        }
    }

    impl SpinnerRenderer for RecordingRenderer {
        fn move_left(&mut self, count: usize) {
            self.ops
                .lock()
                .expect("renderer poisoned")
                .push(format!("left:{count}"));
        }

        fn move_right(&mut self, count: usize) {
            self.ops
                .lock()
                .expect("renderer poisoned")
                .push(format!("right:{count}"));
        }

        fn backspace(&mut self, count: usize) {
            self.ops
                .lock()
                .expect("renderer poisoned")
                .push(format!("backspace:{count}"));
        }

        fn inject_frame(&mut self, frame: &str) {
            self.ops
                .lock()
                .expect("renderer poisoned")
                .push(format!("frame:{frame}"));
        }

        fn finish(&mut self) {
            self.ops
                .lock()
                .expect("renderer poisoned")
                .push("finish".to_string());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn spinner_advances_frames_every_80ms() {
        let renderer = RecordingRenderer::default();
        let snapshot = renderer.clone();
        let (_cancel, cancel_rx) = oneshot::channel();

        let task = tokio::spawn(run_spinner_loop(renderer, cancel_rx));
        tokio::time::advance(Duration::from_millis(AI_SPINNER_TICK_MS)).await;
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(AI_SPINNER_TICK_MS)).await;
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_millis(AI_SPINNER_TICK_MS)).await;
        tokio::task::yield_now().await;
        task.abort();

        assert_eq!(
            snapshot.snapshot(),
            vec![
                "left:9".to_string(),
                "backspace:1".to_string(),
                "frame:⠙".to_string(),
                "backspace:1".to_string(),
                "frame:⠹".to_string()
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn spinner_cancel_cleans_up_with_ten_backspaces() {
        let renderer = RecordingRenderer::default();
        let snapshot = renderer.clone();
        let (cancel, cancel_rx) = oneshot::channel();

        let task = tokio::spawn(run_spinner_loop(renderer, cancel_rx));
        let _ = cancel.send(());
        task.await.expect("spinner task should exit cleanly");

        assert_eq!(
            snapshot.snapshot(),
            vec![
                "left:9".to_string(),
                "right:9".to_string(),
                "backspace:10".to_string(),
                "finish".to_string()
            ]
        );
    }
}
