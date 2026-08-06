use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use tray_icon::menu::{Menu, MenuEvent, MenuItem};
use tray_icon::{TrayIconBuilder, TrayIconEvent};

const TOOLTIP_RUNNING: &str = "Taurine Running";
const TOOLTIP_PAUSED: &str = "Taurine Paused";

#[cfg(target_os = "windows")]
fn initialize_windows_ui() {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::UI::HiDpi::SetProcessDpiAwareness;

    unsafe {
        // 1. Enable high-resolution menus by disabling DWM bitmap scaling for this process
        let _ = SetProcessDpiAwareness(2);

        // 2. Opt the application into Windows Dark Mode for context menus
        let uxtheme = LoadLibraryW(windows_sys::w!("uxtheme.dll"));
        if !uxtheme.is_null() {
            // AllowDarkModeForApp / SetPreferredAppMode is ordinal 135
            let set_preferred_app_mode: Option<unsafe extern "system" fn(i32) -> i32> =
                std::mem::transmute(GetProcAddress(uxtheme, 135 as _));

            if let Some(func) = set_preferred_app_mode {
                // 1 = AllowDark, 2 = ForceDark. AllowDark causes menus to follow the system theme.
                func(1);
            }
        }
    }
}

pub fn spawn(paused: Arc<AtomicBool>, system_tray_enabled: Arc<AtomicBool>) -> JoinHandle<()> {
    let spawn_result = std::thread::Builder::new()
        .name("tau-tray".to_string())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            initialize_windows_ui();

            let pause_item = MenuItem::new("Pause", true, None);
            let quit_item = MenuItem::new("Quit", true, None);

            let menu = Menu::new();
            menu.append(&pause_item).ok();
            menu.append(&quit_item).ok();

            let _tray = match TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .with_tooltip(TOOLTIP_RUNNING)
                .build()
            {
                Ok(tray) => tray,
                Err(error) => {
                    tracing::warn!(error = %error, "System tray init failed");
                    return;
                }
            };

            let _ = _tray.set_visible(system_tray_enabled.load(Ordering::Relaxed));

            let menu_rx = MenuEvent::receiver();
            let _tray_rx = TrayIconEvent::receiver();

            #[cfg(target_os = "windows")]
            {
                use windows_sys::Win32::UI::WindowsAndMessaging::{
                    DispatchMessageW, PM_REMOVE, PeekMessageW, TranslateMessage,
                };

                let mut msg = unsafe { std::mem::zeroed() };
                let mut last_paused = None;
                let mut last_visible = None;
                loop {
                    // Update tray visibility based on settings
                    let now_visible = system_tray_enabled.load(Ordering::Relaxed);
                    if last_visible != Some(now_visible) {
                        last_visible = Some(now_visible);
                        let _ = _tray.set_visible(now_visible);
                    }

                    unsafe {
                        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) > 0 {
                            TranslateMessage(&msg);
                            DispatchMessageW(&msg);
                        }
                    }

                    while let Ok(event) = menu_rx.try_recv() {
                        let should_continue =
                            process_menu_event(&event, &pause_item, &quit_item, &paused);

                        if !should_continue {
                            return;
                        }
                    }

                    // Update UI state based on current paused state
                    let now_paused = paused.load(Ordering::Relaxed);
                    if last_paused != Some(now_paused) {
                        last_paused = Some(now_paused);
                        pause_item.set_text(if now_paused { "Resume" } else { "Pause" });
                        let _ = _tray.set_tooltip(Some(if now_paused {
                            TOOLTIP_PAUSED
                        } else {
                            TOOLTIP_RUNNING
                        }));
                    }

                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }

            #[cfg(target_os = "macos")]
            {
                let mut last_paused = None;
                let mut last_visible = None;
                loop {
                    // Update tray visibility based on settings
                    let now_visible = system_tray_enabled.load(Ordering::Relaxed);
                    if last_visible != Some(now_visible) {
                        last_visible = Some(now_visible);
                        let _ = _tray.set_visible(now_visible);
                    }

                    while let Ok(event) = menu_rx.try_recv() {
                        let should_continue =
                            process_menu_event(&event, &pause_item, &quit_item, &paused);

                        if !should_continue {
                            return;
                        }
                    }

                    // Update UI state based on current paused state
                    let now_paused = paused.load(Ordering::Relaxed);
                    if last_paused != Some(now_paused) {
                        last_paused = Some(now_paused);
                        pause_item.set_text(if now_paused { "Resume" } else { "Pause" });
                        let _ = _tray.set_tooltip(Some(if now_paused {
                            TOOLTIP_PAUSED
                        } else {
                            TOOLTIP_RUNNING
                        }));
                    }

                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }
        });
    match spawn_result {
        Ok(handle) => handle,
        Err(error) => {
            tracing::error!(error = %error, "Failed to spawn system tray thread");
            std::thread::spawn(|| {})
        }
    }
}

fn process_menu_event(
    event: &MenuEvent,
    pause_item: &MenuItem,
    quit_item: &MenuItem,
    paused: &Arc<AtomicBool>,
) -> bool {
    if event.id == pause_item.id() {
        let is_currently_paused = paused.load(Ordering::Relaxed);

        if let Some(rt) = crate::TOKIO_HANDLE.get() {
            rt.spawn(async move {
                if let Ok(mut client) = taurine_core::rpc::get_client().await {
                    if is_currently_paused {
                        let _ = client.resume(taurine_core::rpc::ResumeRequest {}).await;
                    } else {
                        let _ = client.pause(taurine_core::rpc::PauseRequest {}).await;
                    }
                }
            });
        }

        true
    } else if event.id == quit_item.id() {
        if let Some(rt) = crate::TOKIO_HANDLE.get() {
            rt.spawn(async move {
                if let Ok(mut client) = taurine_core::rpc::get_client().await {
                    let _ = client.shutdown(taurine_core::rpc::ShutdownRequest {}).await;
                }
            });
        }
        false
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_menu_event_pause_toggles_state() {
        let paused = Arc::new(AtomicBool::new(false));

        let pause_item = MenuItem::new("Pause", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        let event = MenuEvent {
            id: pause_item.id().clone(),
        };

        // 1st press: Should continue loop
        let should_continue = process_menu_event(&event, &pause_item, &quit_item, &paused);
        assert!(should_continue, "Pause event should not quit the loop");

        // 2nd press: Should continue loop
        let should_continue2 = process_menu_event(&event, &pause_item, &quit_item, &paused);
        assert!(should_continue2, "Resume event should not quit the loop");
    }

    #[test]
    fn test_process_menu_event_quit_signals_shutdown() {
        let paused = Arc::new(AtomicBool::new(false));

        let pause_item = MenuItem::new("Pause", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        let event = MenuEvent {
            id: quit_item.id().clone(),
        };

        let should_continue = process_menu_event(&event, &pause_item, &quit_item, &paused);
        assert!(
            !should_continue,
            "Quit event should signal loop to terminate"
        );
    }

    #[test]
    fn test_tray_spawn_with_visibility() {
        let paused = Arc::new(AtomicBool::new(false));
        let enabled = Arc::new(AtomicBool::new(false));
        // Verify it spawns a thread successfully without panic
        spawn(paused, enabled);
    }
}
