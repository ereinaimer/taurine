use taurine_core::utils::spinner::SpinnerRenderer;

#[derive(Default)]
pub struct OsSpinnerRenderer {
    original_clipboard: Option<String>,
}

impl SpinnerRenderer for OsSpinnerRenderer {
    fn move_left(&mut self, count: usize) {
        with_hidden_spinner_input(|| crate::platform::get_injector().simulate_left(count));
    }

    fn move_right(&mut self, count: usize) {
        with_hidden_spinner_input(|| crate::platform::get_injector().simulate_right(count));
    }

    fn backspace(&mut self, count: usize) {
        with_hidden_spinner_input(|| crate::platform::get_injector().simulate_backspace(count));
    }

    fn inject_frame(&mut self, frame: &str) {
        if crate::platform::get_injector().try_inject_frame_raw(frame) {
            return;
        }

        let original = with_hidden_spinner_input(|| {
            crate::injector::inject_text_segment(frame, &self.original_clipboard)
        });
        if self.original_clipboard.is_none() {
            self.original_clipboard = original.original_clipboard;
        }
    }

    fn finish(&mut self) {
        if let Some(original) = self.original_clipboard.take() {
            crate::injector::restore_clipboard_text(&original);
        }
    }
}

fn with_hidden_spinner_input<T>(action: impl FnOnce() -> T) -> T {
    let _guard = crate::injector::InjectionVisibilityGuard::begin();
    action()
}
