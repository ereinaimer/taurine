use taurine_core::utils::spinner::SpinnerRenderer;

#[derive(Default)]
pub struct OsSpinnerRenderer {
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
