use std::sync::{Arc, Mutex};
use taurine_core::engine::EngineState;
use tracing::debug;

pub mod atspi_dbus;
pub mod kwin_dbus;
pub mod wayland_wlroots;
pub mod x11;

static ACTIVE_WINDOW: std::sync::OnceLock<Arc<Mutex<Option<String>>>> = std::sync::OnceLock::new();

pub fn get_active_window_label() -> Option<String> {
    if let Some(lock) = ACTIVE_WINDOW.get() {
        if let Ok(guard) = lock.lock() {
            if guard.is_some() {
                return guard.clone();
            }
        }
    }
    // Fallback to synchronous X11 query if the listener isn't active, fails, or hasn't updated yet
    x11::get_active_window_label_sync()
}

pub fn start_listener(state: Arc<EngineState>) {
    let active_window = Arc::new(Mutex::new(None));
    let _ = ACTIVE_WINDOW.set(active_window.clone());

    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();

        if desktop.contains("kde") {
            debug!("Starting KDE KWin D-Bus toplevel listener");
            kwin_dbus::start_listener(state.clone(), active_window.clone());
            return;
        } else if desktop.contains("gnome") {
            debug!("Starting GNOME AT-SPI2 toplevel listener");
            atspi_dbus::start_listener(state.clone(), active_window.clone());
            return;
        } else {
            debug!("Starting wlroots Wayland toplevel listener");
            wayland_wlroots::start_listener(state.clone(), active_window.clone());
            return;
        }
    }

    debug!("Starting X11 toplevel listener");
    x11::start_listener(state, active_window);
}

pub fn stop_listener() {
    x11::stop_listener();
    wayland_wlroots::stop_listener();
    kwin_dbus::stop_listener();
    atspi_dbus::stop_listener();
}
