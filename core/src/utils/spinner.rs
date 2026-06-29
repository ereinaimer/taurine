use crate::settings::SpinnerStyle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::oneshot;

pub const BRAILLE_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub const ARC_FRAMES: &[&str] = &["◜", "◠", "◝", "◞", "◡", "◟"];
pub const CLASSIC_FRAMES: &[&str] = &["|", "/", "-", "\\"];

pub fn get_frames(style: SpinnerStyle) -> &'static [&'static str] {
    match style {
        SpinnerStyle::Braille => BRAILLE_FRAMES,
        SpinnerStyle::Arc => ARC_FRAMES,
        SpinnerStyle::Classic => CLASSIC_FRAMES,
    }
}

pub trait SpinnerRenderer: Send + Sync {
    fn move_left(&mut self, count: usize);
    fn move_right(&mut self, count: usize);
    fn backspace(&mut self, count: usize);
    fn inject_frame(&mut self, frame: &str);
    fn finish(&mut self);
}

// --- Async Spinner (for AI) ---

pub struct AsyncSpinnerHandle {
    pub cancel: oneshot::Sender<()>,
    pub task: tokio::task::JoinHandle<()>,
}

pub fn spawn_async<R: SpinnerRenderer + 'static>(
    style: SpinnerStyle,
    renderer: R,
    runtime: &tokio::runtime::Handle,
) -> AsyncSpinnerHandle {
    spawn_async_with_suffix(style, renderer, runtime, true)
}

pub fn spawn_async_with_suffix<R: SpinnerRenderer + 'static>(
    style: SpinnerStyle,
    renderer: R,
    runtime: &tokio::runtime::Handle,
    has_suffix: bool,
) -> AsyncSpinnerHandle {
    let (cancel, cancel_rx) = oneshot::channel();
    let task = runtime.spawn(async move {
        run_async_loop(style, renderer, cancel_rx, has_suffix).await;
    });

    AsyncSpinnerHandle { cancel, task }
}

const AI_SPINNER_TICK_MS: u64 = 80;
const AI_SPINNER_SUFFIX_LEN: usize = 12;

async fn run_async_loop<R: SpinnerRenderer>(
    style: SpinnerStyle,
    mut renderer: R,
    mut cancel_rx: oneshot::Receiver<()>,
    has_suffix: bool,
) {
    let frames = get_frames(style);
    let frame_width = frames[0].chars().count();
    let suffix_len = if has_suffix { AI_SPINNER_SUFFIX_LEN } else { 0 };
    let total_width = frame_width + suffix_len;

    if suffix_len > 0 {
        renderer.move_left(suffix_len);
    }

    let mut frame_index = 1usize;
    let mut timer = tokio::time::interval(Duration::from_millis(AI_SPINNER_TICK_MS));
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    // Initial tick happens immediately
    timer.tick().await;

    loop {
        tokio::select! {
            _ = &mut cancel_rx => {
                if suffix_len > 0 {
                    renderer.move_right(suffix_len);
                }
                renderer.backspace(total_width);
                renderer.finish();
                break;
            }
            _ = timer.tick() => {
                renderer.backspace(frame_width);
                let frame = frames[frame_index % frames.len()];
                renderer.inject_frame(frame);
                frame_index += 1;
            }
        }
    }
}

// --- Threaded Spinner (for Scripts) ---

pub struct ThreadSpinnerHandle {
    abort: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl ThreadSpinnerHandle {
    pub fn stop(mut self) {
        self.abort.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

pub fn spawn_threaded<R: SpinnerRenderer + 'static>(
    style: SpinnerStyle,
    mut renderer: R,
) -> ThreadSpinnerHandle {
    let abort = Arc::new(AtomicBool::new(false));
    let abort_clone = abort.clone();
    let frames = get_frames(style);
    let frame_width = frames[0].chars().count();

    let thread = std::thread::spawn(move || {
        let mut idx = 0;

        while !abort_clone.load(Ordering::SeqCst) {
            let frame = frames[idx % frames.len()];
            renderer.inject_frame(frame);

            std::thread::sleep(Duration::from_millis(60));

            if abort_clone.load(Ordering::SeqCst) {
                break;
            }

            renderer.backspace(frame_width);
            idx += 1;
        }

        renderer.backspace(frame_width);
        renderer.finish();
    });

    ThreadSpinnerHandle {
        abort,
        thread: Some(thread),
    }
}
