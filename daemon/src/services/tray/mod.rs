use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

pub mod icons;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(windows, target_os = "macos"))]
mod native;
pub mod settings;
pub mod snooze;
#[cfg(any(windows, target_os = "macos"))]
pub(crate) use native::set_tray_pause_audio;
/// Linux tray has no local pause-fallback cues; the setter is a no-op there
/// so startup stays platform-uniform.
#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn set_tray_pause_audio(_enabled: Arc<AtomicBool>) {}

pub fn spawn(paused: Arc<AtomicBool>, system_tray_enabled: Arc<AtomicBool>) -> JoinHandle<()> {
    #[cfg(target_os = "linux")]
    {
        linux::spawn(paused, system_tray_enabled)
    }

    #[cfg(all(not(target_os = "linux"), any(windows, target_os = "macos")))]
    {
        native::spawn(paused, system_tray_enabled)
    }

    #[cfg(all(not(target_os = "linux"), not(any(windows, target_os = "macos"))))]
    {
        let _ = (paused, system_tray_enabled);
        std::thread::Builder::new()
            .name("tau-tray".to_string())
            .spawn(|| {})
            .expect("tray thread spawn")
    }
}

/// Set by daemon shutdown so the tray thread removes its icon and exits
/// instead of being killed mid-loop (which orphans a ghost icon every
/// restart). Checked once per loop iteration (~100ms).
static TRAY_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Signal the tray thread to remove its icon and exit cleanly.
pub fn request_shutdown() {
    TRAY_SHUTDOWN.store(true, Ordering::Relaxed);
}

pub(crate) fn shutdown_requested() -> bool {
    TRAY_SHUTDOWN.load(Ordering::Relaxed)
}
