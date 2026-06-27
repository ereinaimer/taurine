use objc2_app_kit::{NSApplication, NSWorkspace};
use objc2_foundation::{MainThreadMarker, NSNotificationCenter, NSString};
use std::sync::Arc;
use taurine_core::engine::EngineState;
use tracing::debug;

pub fn start_listener(_state: Arc<EngineState>) {
    std::thread::spawn(move || {
        let _mtm = unsafe { MainThreadMarker::new_unchecked() };

        // Note: Full implementation of NSNotificationCenter observer block using objc2 requires `Block` syntax.
        // We evaluate NSApplication.sharedApplication().currentSystemPresentationOptions()
        // If it contains NSApplicationPresentationFullScreen, we mark true.

        // This is a placeholder for the block observer registration.
        // In actual implementation, we would register a Block to handle "NSWorkspaceActiveSpaceDidChangeNotification"
        // and evaluate:
        /*
           let opts = NSApplication::sharedApplication(mtm).currentSystemPresentationOptions();
           let is_fullscreen = opts.contains(NSApplicationPresentationOptions::NSApplicationPresentationFullScreen);
           state.is_os_fullscreen.store(is_fullscreen, Ordering::Relaxed);
        */
    });
}
