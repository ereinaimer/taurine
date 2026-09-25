use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIconBuilder, TrayIconEvent};

use super::settings::TraySettings;
use super::snooze::SnoozeController;

const TOOLTIP: &str = "Taurine";

#[cfg(target_os = "windows")]
fn initialize_windows_ui() {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::UI::HiDpi::{
        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwareness,
        SetProcessDpiAwarenessContext, SetThreadDpiAwarenessContext,
    };

    // SAFETY: SetProcessDpiAwarenessContext and SetThreadDpiAwarenessContext configure process
    // and thread DPI scaling contexts for Win32 menus and windows. LoadLibraryW and GetProcAddress
    // safely load uxtheme.dll to invoke undocumented SetPreferredAppMode for dark mode support.
    unsafe {
        // 1. Enable Per-Monitor v2 DPI awareness for dynamically scaled context menus across displays
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) == 0 {
            // Fallback for earlier Windows 10/8.1 builds
            let _ = SetProcessDpiAwareness(2);
        }
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

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

#[cfg(target_os = "windows")]
const TRAY_SUBCLASS_ID: usize = 0x544155; // "TAU"
#[cfg(target_os = "windows")]
const SNOOZE_TIMER_ID: usize = 0x534E5A; // "SNZ"

#[cfg(target_os = "windows")]
struct TrayLiveState {
    paused: Arc<AtomicBool>,
    snooze: SnoozeController,
    resume_item: MenuItem,
}

#[cfg(target_os = "windows")]
impl TrayLiveState {
    fn update_label_and_redraw(&self) {
        let label = self.snooze.resume_label();
        self.resume_item.set_text(&label);

        // SAFETY: FindWindowW locates the active context menu window (#32768) and InvalidateRect/UpdateWindow
        // forces an immediate repaint of the menu items so the countdown live-updates on screen without erasing
        // the background (bErase = 0) to eliminate full-menu flicker.
        unsafe {
            let menu_hwnd = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                windows_sys::w!("#32768"),
                std::ptr::null(),
            );
            if !menu_hwnd.is_null() {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(menu_hwnd, std::ptr::null(), 0);
                windows_sys::Win32::Graphics::Gdi::UpdateWindow(menu_hwnd);
            }
        }
    }

    fn reset_label_and_redraw(&self) {
        self.resume_item.set_text("Resume");

        // SAFETY: FindWindowW locates the active context menu window (#32768) and InvalidateRect/UpdateWindow
        // forces an immediate repaint of the menu items so the text resets on screen without full-menu flicker.
        unsafe {
            let menu_hwnd = windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW(
                windows_sys::w!("#32768"),
                std::ptr::null(),
            );
            if !menu_hwnd.is_null() {
                windows_sys::Win32::Graphics::Gdi::InvalidateRect(menu_hwnd, std::ptr::null(), 0);
                windows_sys::Win32::Graphics::Gdi::UpdateWindow(menu_hwnd);
            }
        }
    }
}

#[cfg(target_os = "windows")]
// SAFETY: tray_menu_subclass_proc is invoked by ComCtl32 for the tray window on the GUI thread.
// ref_data is a valid pointer to TrayLiveState that lives on the stack of the tau-tray thread.
unsafe extern "system" fn tray_menu_subclass_proc(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    ref_data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        KillTimer, SetTimer, WM_ENTERMENULOOP, WM_EXITMENULOOP, WM_TIMER,
    };

    match msg {
        WM_ENTERMENULOOP => {
            // SAFETY: ref_data points to live_state valid for the duration of the tau-tray thread.
            let state = unsafe { &*(ref_data as *const TrayLiveState) };
            if state.paused.load(Ordering::Relaxed) && state.snooze.is_active() {
                state.update_label_and_redraw();
                // SAFETY: SetTimer configures the 1000ms live countdown timer during context menu display.
                unsafe {
                    SetTimer(hwnd, SNOOZE_TIMER_ID, 1000, None);
                }
            }
        }
        WM_TIMER if wparam == SNOOZE_TIMER_ID => {
            // SAFETY: ref_data points to live_state valid for the duration of the tau-tray thread.
            let state = unsafe { &*(ref_data as *const TrayLiveState) };
            if state.paused.load(Ordering::Relaxed) && state.snooze.is_active() {
                state.update_label_and_redraw();
            } else {
                // SAFETY: KillTimer halts the timer when snooze expires.
                unsafe {
                    KillTimer(hwnd, SNOOZE_TIMER_ID);
                }
                state.reset_label_and_redraw();
            }
            return 0;
        }
        WM_EXITMENULOOP => {
            // SAFETY: KillTimer halts the timer as soon as the context menu closes.
            unsafe {
                KillTimer(hwnd, SNOOZE_TIMER_ID);
            }
        }
        _ => {}
    }

    // SAFETY: DefSubclassProc forwards unhandled messages down the subclass chain.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

#[cfg(target_os = "windows")]
// SAFETY: enum_thread_windows_cb is invoked by Win32 for each thread window to capture the HWND.
unsafe extern "system" fn enum_thread_windows_cb(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> i32 {
    let target = lparam as *mut windows_sys::Win32::Foundation::HWND;
    // SAFETY: target is a valid pointer to a local HWND variable passed from find_current_thread_window.
    unsafe {
        *target = hwnd;
    }
    0
}

#[cfg(target_os = "windows")]
fn find_current_thread_window() -> windows_sys::Win32::Foundation::HWND {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumThreadWindows;

    let mut hwnd = std::ptr::null_mut();
    // SAFETY: EnumThreadWindows safely queries windows created on the current thread and writes to local hwnd.
    unsafe {
        EnumThreadWindows(
            GetCurrentThreadId(),
            Some(enum_thread_windows_cb),
            &mut hwnd as *mut _ as _,
        );
    }
    hwnd
}

#[derive(Clone)]
pub struct VoiceDeviceItem {
    pub id: tray_icon::menu::MenuId,
    pub name: Option<String>,
    pub item: CheckMenuItem,
}

pub fn populate_voice_device_submenu(
    submenu: &Submenu,
    current_device: Option<&str>,
) -> Vec<VoiceDeviceItem> {
    for _ in 0..submenu.items().len() {
        let _ = submenu.remove_at(0);
    }

    let mut device_items = Vec::new();

    let is_default = current_device.is_none() || current_device == Some("");
    let default_item = CheckMenuItem::new("System Default", true, is_default, None);
    let _ = submenu.append(&default_item);
    device_items.push(VoiceDeviceItem {
        id: default_item.id().clone(),
        name: None,
        item: default_item,
    });

    let _ = submenu.append(&PredefinedMenuItem::separator());

    let available_devices = crate::voice::AudioCapture::list_input_devices();
    for dev_name in available_devices {
        let is_checked = current_device == Some(dev_name.as_str());
        let item = CheckMenuItem::new(&dev_name, true, is_checked, None);
        let _ = submenu.append(&item);
        device_items.push(VoiceDeviceItem {
            id: item.id().clone(),
            name: Some(dev_name),
            item,
        });
    }

    device_items
}

pub struct TrayMenuItems {
    pub pause_submenu: Submenu,
    pub snooze_15m: MenuItem,
    pub snooze_30m: MenuItem,
    pub snooze_1h: MenuItem,
    pub pause_until_resumed: MenuItem,
    pub resume_item: MenuItem,
    pub instant_expand_item: CheckMenuItem,
    pub start_on_boot_item: CheckMenuItem,
    pub voice_input_submenu: Submenu,
    pub voice_input_items: std::sync::Mutex<Vec<VoiceDeviceItem>>,
    pub quit_item: MenuItem,
}

impl TrayMenuItems {
    pub fn new(initial_paused: bool) -> (Self, Menu) {
        let snooze_15m = MenuItem::new("15 minutes", true, None);
        let snooze_30m = MenuItem::new("30 minutes", true, None);
        let snooze_1h = MenuItem::new("1 hour", true, None);
        let pause_until_resumed = MenuItem::new("Until resumed", true, None);

        let pause_submenu = Submenu::new("Pause", true);
        let _ = pause_submenu.append(&snooze_15m);
        let _ = pause_submenu.append(&snooze_30m);
        let _ = pause_submenu.append(&snooze_1h);
        let _ = pause_submenu.append(&PredefinedMenuItem::separator());
        let _ = pause_submenu.append(&pause_until_resumed);

        let resume_item = MenuItem::new("Resume", true, None);

        let (instant_expand_init, start_on_boot_init) = TraySettings::load_quick_settings();

        let instant_expand_item =
            CheckMenuItem::new("Instant Expansion", true, instant_expand_init, None);
        let start_on_boot_item =
            CheckMenuItem::new("Start on Boot", true, start_on_boot_init, None);

        let voice_input_submenu = Submenu::new("Microphone", true);
        let current_device = TraySettings::get_voice_input_device();
        let voice_items =
            populate_voice_device_submenu(&voice_input_submenu, current_device.as_deref());
        let voice_input_items = std::sync::Mutex::new(voice_items);

        let quit_item = MenuItem::new("Quit", true, None);

        let menu = Menu::new();
        if initial_paused {
            let _ = menu.append(&resume_item);
        } else {
            let _ = menu.append(&pause_submenu);
        }
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&instant_expand_item);
        let _ = menu.append(&start_on_boot_item);
        let _ = menu.append(&voice_input_submenu);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&quit_item);

        let items = Self {
            pause_submenu,
            snooze_15m,
            snooze_30m,
            snooze_1h,
            pause_until_resumed,
            resume_item,
            instant_expand_item,
            start_on_boot_item,
            voice_input_submenu,
            voice_input_items,
            quit_item,
        };

        (items, menu)
    }
}

fn run_tray_loop_once(paused: &Arc<AtomicBool>, system_tray_enabled: &Arc<AtomicBool>) -> bool {
    #[cfg(target_os = "windows")]
    initialize_windows_ui();

    let initial_paused = paused.load(Ordering::Relaxed);
    let (items, menu) = TrayMenuItems::new(initial_paused);
    let snooze = SnoozeController::new();

    let running_icon = super::icons::running_icon();
    let paused_icon = super::icons::paused_icon();

    let initial_icon = if initial_paused {
        paused_icon.clone()
    } else {
        running_icon.clone()
    };

    let _tray = match TrayIconBuilder::new()
        .with_menu(Box::new(menu.clone()))
        .with_menu_on_left_click(false)
        .with_tooltip(TOOLTIP)
        .with_icon(initial_icon)
        .build()
    {
        Ok(tray) => tray,
        Err(error) => {
            tracing::warn!(error = %error, "System tray init failed");
            return false;
        }
    };

    let _ = _tray.set_visible(system_tray_enabled.load(Ordering::Relaxed));

    let menu_rx = MenuEvent::receiver();
    let _tray_rx = TrayIconEvent::receiver();

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::Shell::{RemoveWindowSubclass, SetWindowSubclass};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, PM_REMOVE, PeekMessageW, TranslateMessage,
        };

        let live_state = TrayLiveState {
            paused: paused.clone(),
            snooze: snooze.clone(),
            resume_item: items.resume_item.clone(),
        };

        let mut tray_hwnd = find_current_thread_window();
        if !tray_hwnd.is_null() {
            // SAFETY: SetWindowSubclass attaches our tray_menu_subclass_proc to the tray window.
            // live_state lives on this thread's stack until the thread terminates.
            unsafe {
                SetWindowSubclass(
                    tray_hwnd,
                    Some(tray_menu_subclass_proc),
                    TRAY_SUBCLASS_ID,
                    &live_state as *const _ as _,
                );
            }
        }

        let mut msg = unsafe { std::mem::zeroed() };
        let mut last_paused = Some(initial_paused);
        let mut menu_displayed_paused = initial_paused;
        let mut last_visible = None;
        let mut last_resume_label = "Resume".to_string();
        let mut sync_counter: u32 = 0;
        let mut last_settings_version: u64 = u64::MAX;
        loop {
            // Update tray visibility based on settings
            let now_visible = system_tray_enabled.load(Ordering::Relaxed);
            if last_visible != Some(now_visible) {
                last_visible = Some(now_visible);
                let _ = _tray.set_visible(now_visible);
            }

            // SAFETY: PeekMessageW, TranslateMessage, and DispatchMessageW are standard Win32
            // message pump routines processing thread-local GUI messages for the tray icon.
            unsafe {
                while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) > 0 {
                    if tray_hwnd.is_null() && !msg.hwnd.is_null() {
                        tray_hwnd = msg.hwnd;
                        // SAFETY: SetWindowSubclass attaches our tray_menu_subclass_proc to the tray window.
                        SetWindowSubclass(
                            tray_hwnd,
                            Some(tray_menu_subclass_proc),
                            TRAY_SUBCLASS_ID,
                            &live_state as *const _ as _,
                        );
                    }

                    // Update dynamic countdown label and voice device submenu immediately before displaying the popup menu
                    if msg.message == 0x0401
                        && matches!(
                            msg.lParam as u32,
                            0x0201 | 0x0202 | 0x0204 | 0x0205 | 0x007B
                        )
                    {
                        if paused.load(Ordering::Relaxed) {
                            let label = snooze.resume_label();
                            if label != last_resume_label {
                                items.resume_item.set_text(&label);
                                last_resume_label = label;
                            }
                        }
                        let current_dev = TraySettings::get_voice_input_device();
                        let updated_items = populate_voice_device_submenu(
                            &items.voice_input_submenu,
                            current_dev.as_deref(),
                        );
                        *items
                            .voice_input_items
                            .lock()
                            .unwrap_or_else(|p| p.into_inner()) = updated_items;
                    }

                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            while let Ok(event) = menu_rx.try_recv() {
                let should_continue = process_menu_event(&event, &items, paused, &snooze);

                if !should_continue {
                    if !tray_hwnd.is_null() {
                        // SAFETY: RemoveWindowSubclass unhooks the subclass before the thread terminates.
                        unsafe {
                            RemoveWindowSubclass(
                                tray_hwnd,
                                Some(tray_menu_subclass_proc),
                                TRAY_SUBCLASS_ID,
                            );
                        }
                    }
                    return true;
                }
            }

            // Update UI state based on current paused state
            let now_paused = paused.load(Ordering::Relaxed);
            if last_paused != Some(now_paused) {
                let previously_paused = last_paused.unwrap_or(false);
                last_paused = Some(now_paused);

                // If unpaused externally (e.g. via global keyboard shortcut), cancel any pending snooze
                if previously_paused && !now_paused {
                    snooze.cancel();
                }

                if now_paused && !menu_displayed_paused {
                    let label = snooze.resume_label();
                    items.resume_item.set_text(&label);
                    last_resume_label = label;
                    let _ = menu.remove(&items.pause_submenu);
                    let _ = menu.insert(&items.resume_item, 0);
                    menu_displayed_paused = true;
                } else if !now_paused && menu_displayed_paused {
                    items.resume_item.set_text("Resume");
                    last_resume_label = "Resume".to_string();
                    let _ = menu.remove(&items.resume_item);
                    let _ = menu.insert(&items.pause_submenu, 0);
                    menu_displayed_paused = false;
                }
                let _ = _tray.set_icon(Some(if now_paused {
                    paused_icon.clone()
                } else {
                    running_icon.clone()
                }));
            }

            // Update dynamic countdown label if snoozed
            if now_paused && snooze.is_active() {
                let label = snooze.resume_label();
                if label != last_resume_label {
                    items.resume_item.set_text(&label);
                    last_resume_label = label;
                }
            } else if now_paused && last_resume_label != "Resume" {
                last_resume_label = "Resume".to_string();
                items.resume_item.set_text("Resume");
            }

            // Synchronize checkmarks with the database only when settings changed
            // (plus a 5s heartbeat for edits made outside this process).
            sync_counter += 1;
            let heartbeat = sync_counter >= 50;
            let changed = TraySettings::load_quick_settings_if_changed(last_settings_version);
            if heartbeat || changed.is_some() {
                sync_counter = 0;
                if let Some(session) = crate::VOICE_SESSION.get() {
                    session.clean_expired_transcriber();
                }
                let (instant, boot) = match changed {
                    Some((instant, boot, version)) => {
                        last_settings_version = version;
                        (instant, boot)
                    }
                    None => TraySettings::load_quick_settings(),
                };
                if items.instant_expand_item.is_checked() != instant {
                    items.instant_expand_item.set_checked(instant);
                }
                if items.start_on_boot_item.is_checked() != boot {
                    items.start_on_boot_item.set_checked(boot);
                }
                if changed.is_some() {
                    let current_dev = TraySettings::get_voice_input_device();
                    let updated_items = populate_voice_device_submenu(
                        &items.voice_input_submenu,
                        current_dev.as_deref(),
                    );
                    *items
                        .voice_input_items
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = updated_items;
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let mut last_paused = Some(initial_paused);
        let mut menu_displayed_paused = initial_paused;
        let mut last_visible = None;
        let mut last_resume_label = "Resume".to_string();
        let mut sync_counter: u32 = 0;
        let mut last_settings_version: u64 = u64::MAX;
        loop {
            // Update tray visibility based on settings
            let now_visible = system_tray_enabled.load(Ordering::Relaxed);
            if last_visible != Some(now_visible) {
                last_visible = Some(now_visible);
                let _ = _tray.set_visible(now_visible);
            }

            while let Ok(event) = menu_rx.try_recv() {
                let should_continue = process_menu_event(&event, &items, paused, &snooze);

                if !should_continue {
                    return true;
                }
            }

            // Update UI state based on current paused state
            let now_paused = paused.load(Ordering::Relaxed);
            if last_paused != Some(now_paused) {
                let previously_paused = last_paused.unwrap_or(false);
                last_paused = Some(now_paused);

                // If unpaused externally (e.g. via global keyboard shortcut), cancel any pending snooze
                if previously_paused && !now_paused {
                    snooze.cancel();
                }

                if now_paused && !menu_displayed_paused {
                    let label = snooze.resume_label();
                    items.resume_item.set_text(&label);
                    last_resume_label = label;
                    let _ = menu.remove(&items.pause_submenu);
                    let _ = menu.insert(&items.resume_item, 0);
                    menu_displayed_paused = true;
                } else if !now_paused && menu_displayed_paused {
                    items.resume_item.set_text("Resume");
                    last_resume_label = "Resume".to_string();
                    let _ = menu.remove(&items.resume_item);
                    let _ = menu.insert(&items.pause_submenu, 0);
                    menu_displayed_paused = false;
                }
                let _ = _tray.set_icon(Some(if now_paused {
                    paused_icon.clone()
                } else {
                    running_icon.clone()
                }));
            }

            // Update dynamic countdown label if snoozed
            if now_paused && snooze.is_active() {
                let label = snooze.resume_label();
                if label != last_resume_label {
                    items.resume_item.set_text(&label);
                    last_resume_label = label;
                }
            } else if now_paused && last_resume_label != "Resume" {
                last_resume_label = "Resume".to_string();
                items.resume_item.set_text("Resume");
            }

            // Synchronize checkmarks with the database only when settings changed
            // (plus a 5s heartbeat for edits made outside this process).
            sync_counter += 1;
            let heartbeat = sync_counter >= 50;
            let changed = TraySettings::load_quick_settings_if_changed(last_settings_version);
            if heartbeat || changed.is_some() {
                sync_counter = 0;
                if let Some(session) = crate::VOICE_SESSION.get() {
                    session.clean_expired_transcriber();
                }
                let (instant, boot) = match changed {
                    Some((instant, boot, version)) => {
                        last_settings_version = version;
                        (instant, boot)
                    }
                    None => TraySettings::load_quick_settings(),
                };
                if items.instant_expand_item.is_checked() != instant {
                    items.instant_expand_item.set_checked(instant);
                }
                if items.start_on_boot_item.is_checked() != boot {
                    items.start_on_boot_item.set_checked(boot);
                }
                if changed.is_some() {
                    let current_dev = TraySettings::get_voice_input_device();
                    let updated_items = populate_voice_device_submenu(
                        &items.voice_input_submenu,
                        current_dev.as_deref(),
                    );
                    *items
                        .voice_input_items
                        .lock()
                        .unwrap_or_else(|p| p.into_inner()) = updated_items;
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    #[allow(unreachable_code)]
    false
}

pub fn spawn(paused: Arc<AtomicBool>, system_tray_enabled: Arc<AtomicBool>) -> JoinHandle<()> {
    let spawn_result = std::thread::Builder::new()
        .name("tau-tray".to_string())
        .spawn(move || {
            let mut backoff = crate::platform::circuit_breaker::ExponentialBackoff::default();
            loop {
                let p = paused.clone();
                let ste = system_tray_enabled.clone();
                let run_res = crate::platform::panic::catch_worker_panic(
                    "tau-tray",
                    std::panic::AssertUnwindSafe(|| run_tray_loop_once(&p, &ste)),
                );
                match run_res {
                    Ok(true) => {
                        // User clicked quit / clean exit requested
                        break;
                    }
                    Ok(false) => {
                        tracing::warn!("System tray event loop exited unexpectedly; restarting");
                        backoff.wait();
                    }
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            "System tray thread panicked; restarting with backoff"
                        );
                        backoff.wait();
                    }
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

type DaemonClient = taurine_core::system::rpc::daemon_control_client::DaemonControlClient<
    tonic::transport::Channel,
>;

/// Fire a daemon RPC off the tray thread, logging failures instead of dropping
/// them silently. Returns false when no runtime exists to run on.
fn spawn_daemon_call<Fut>(
    label: &'static str,
    call: impl FnOnce(DaemonClient) -> Fut + Send + 'static,
) -> bool
where
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let Some(rt) = crate::TOKIO_HANDLE.get() else {
        tracing::warn!("tray {label} dropped: tokio handle not initialized");
        return false;
    };
    rt.spawn(async move {
        match taurine_core::rpc::get_client().await {
            Ok(client) => call(client).await,
            Err(error) => tracing::warn!(%error, "tray {label} dropped: daemon unreachable"),
        }
    });
    true
}

/// Tray fallback path: the tokio runtime is down, so the flag was flipped
/// locally and the daemon RPC never ran. Forward the transition through the
/// helper so the coordinator does not diverge. Total failure (no sender at
/// all) logs an error naming the path.
fn notify_tray_transition(path: &'static str, now_paused: bool) {
    match crate::input::hotkey::pause_transition_sender() {
        Some(tx) => crate::input::hotkey::notify_pause_transition(&tx, now_paused),
        None => tracing::error!(
            "{path}: pause-transition sender unavailable; coordinator will diverge until next toggle"
        ),
    }
}

/// Process-global pause-audio flag for the tray fallback cues. The tray menu
/// never had the setting threaded to it; set once at daemon startup next to
/// the transition sender. Unset (tests) fails open so existing tests keep
/// their cue expectations.
static TRAY_PAUSE_AUDIO: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

pub fn set_tray_pause_audio(enabled: Arc<AtomicBool>) {
    *TRAY_PAUSE_AUDIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(enabled);
}

fn tray_pause_audio_enabled() -> bool {
    TRAY_PAUSE_AUDIO
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .is_none_or(|flag| flag.load(Ordering::Relaxed))
}

pub fn process_menu_event(
    event: &MenuEvent,
    items: &TrayMenuItems,
    paused: &Arc<AtomicBool>,
    snooze: &SnoozeController,
) -> bool {
    let event_id = &event.id;

    if event_id == items.snooze_15m.id()
        || event_id == items.snooze_30m.id()
        || event_id == items.snooze_1h.id()
    {
        let duration = if event_id == items.snooze_15m.id() {
            Duration::from_secs(15 * 60)
        } else if event_id == items.snooze_30m.id() {
            Duration::from_secs(30 * 60)
        } else {
            Duration::from_secs(60 * 60)
        };
        let paused_clone = paused.clone();
        snooze.start_snooze(duration, move || {
            paused_clone.store(false, Ordering::Relaxed);
            // The resume RPC below no-ops once the flag is already false, so
            // notify directly or the coordinator never learns about the expiry.
            notify_tray_transition("tray snooze expiry", false);
            spawn_daemon_call("resume", |mut client| async move {
                if let Err(error) = client.resume(taurine_core::rpc::ResumeRequest {}).await {
                    tracing::warn!(%error, "tray resume request failed");
                }
            });
        });
        items.resume_item.set_text(snooze.resume_label());

        if !spawn_daemon_call("pause", |mut client| async move {
            if let Err(error) = client.pause(taurine_core::rpc::PauseRequest {}).await {
                tracing::warn!(%error, "tray pause request failed");
            }
        }) {
            paused.store(true, Ordering::Relaxed);
            if tray_pause_audio_enabled() {
                crate::services::audio::play_pause_cue(true);
            }
            notify_tray_transition("tray snooze fallback", true);
        }

        true
    } else if event_id == items.pause_until_resumed.id() {
        snooze.cancel();
        items.resume_item.set_text("Resume");
        if !spawn_daemon_call("pause", |mut client| async move {
            if let Err(error) = client.pause(taurine_core::rpc::PauseRequest {}).await {
                tracing::warn!(%error, "tray pause request failed");
            }
        }) {
            paused.store(true, Ordering::Relaxed);
            if tray_pause_audio_enabled() {
                crate::services::audio::play_pause_cue(true);
            }
            notify_tray_transition("tray pause fallback", true);
        }
        true
    } else if event_id == items.resume_item.id() {
        snooze.cancel();
        items.resume_item.set_text("Resume");
        if !spawn_daemon_call("resume", |mut client| async move {
            if let Err(error) = client.resume(taurine_core::rpc::ResumeRequest {}).await {
                tracing::warn!(%error, "tray resume request failed");
            }
        }) {
            paused.store(false, Ordering::Relaxed);
            if tray_pause_audio_enabled() {
                crate::services::audio::play_pause_cue(false);
            }
            notify_tray_transition("tray resume fallback", false);
        }
        true
    } else if event_id == items.instant_expand_item.id() {
        match TraySettings::toggle_instant_expand() {
            Ok(new_val) => items.instant_expand_item.set_checked(new_val),
            Err(error) => tracing::warn!(%error, "tray instant-expand toggle failed"),
        }
        true
    } else if event_id == items.start_on_boot_item.id() {
        match TraySettings::toggle_start_on_boot() {
            Ok(new_val) => items.start_on_boot_item.set_checked(new_val),
            Err(error) => tracing::warn!(%error, "tray start-on-boot toggle failed"),
        }
        true
    } else if event_id == items.quit_item.id() {
        spawn_daemon_call("shutdown", |mut client| async move {
            if let Err(error) = client.shutdown(taurine_core::rpc::ShutdownRequest {}).await {
                tracing::warn!(%error, "tray shutdown request failed");
            }
        });
        false
    } else {
        let voice_items = items
            .voice_input_items
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        for vi in &voice_items {
            if vi.id == *event_id {
                let dev_name = vi.name.as_deref();
                if let Err(e) = TraySettings::set_voice_input_device(dev_name) {
                    tracing::warn!(error = %e, "Failed to set voice input device from tray");
                } else {
                    spawn_daemon_call("reload", |mut client| async move {
                        if let Err(error) = client.reload(taurine_core::rpc::ReloadRequest {}).await
                        {
                            tracing::warn!(%error, "tray reload request failed");
                        }
                    });
                }
                for item in &voice_items {
                    item.item.set_checked(item.id == *event_id);
                }
                return true;
            }
        }
        true
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_process_menu_event_snooze_15m() {
        let paused = Arc::new(AtomicBool::new(false));
        let snooze = SnoozeController::new();
        let (items, _) = TrayMenuItems::new(false);

        let event = MenuEvent {
            id: items.snooze_15m.id().clone(),
        };

        let should_continue = process_menu_event(&event, &items, &paused, &snooze);
        assert!(should_continue);
        assert!(paused.load(Ordering::Relaxed));
        assert!(snooze.is_active());
        let label = snooze.resume_label();
        assert!(
            label.starts_with("Resume (14m ") || label == "Resume (15m 00s)",
            "Unexpected resume label: {label}"
        );
    }

    #[tokio::test]
    async fn test_process_menu_event_snooze_30m() {
        let paused = Arc::new(AtomicBool::new(false));
        let snooze = SnoozeController::new();
        let (items, _) = TrayMenuItems::new(false);

        let event = MenuEvent {
            id: items.snooze_30m.id().clone(),
        };

        let should_continue = process_menu_event(&event, &items, &paused, &snooze);
        assert!(should_continue);
        assert!(paused.load(Ordering::Relaxed));
        assert!(snooze.is_active());
        let label = snooze.resume_label();
        assert!(
            label.starts_with("Resume (29m ") || label == "Resume (30m 00s)",
            "Unexpected resume label: {label}"
        );
    }

    #[tokio::test]
    async fn test_process_menu_event_snooze_1h() {
        let paused = Arc::new(AtomicBool::new(false));
        let snooze = SnoozeController::new();
        let (items, _) = TrayMenuItems::new(false);

        let event = MenuEvent {
            id: items.snooze_1h.id().clone(),
        };

        let should_continue = process_menu_event(&event, &items, &paused, &snooze);
        assert!(should_continue);
        assert!(paused.load(Ordering::Relaxed));
        assert!(snooze.is_active());
        let label = snooze.resume_label();
        assert!(
            label.starts_with("Resume (59m ") || label == "Resume (60m 00s)",
            "Unexpected resume label: {label}"
        );
    }

    #[tokio::test]
    async fn test_process_menu_event_pause_until_resumed() {
        let paused = Arc::new(AtomicBool::new(false));
        let snooze = SnoozeController::new();
        snooze.start_snooze(Duration::from_secs(60), || {});
        assert!(snooze.is_active());

        let (items, _) = TrayMenuItems::new(false);

        let event = MenuEvent {
            id: items.pause_until_resumed.id().clone(),
        };

        let should_continue = process_menu_event(&event, &items, &paused, &snooze);
        assert!(should_continue);
        assert!(paused.load(Ordering::Relaxed));
        assert!(!snooze.is_active());
        assert_eq!(snooze.resume_label(), "Resume");
    }

    #[tokio::test]
    async fn test_process_menu_event_resume() {
        let paused = Arc::new(AtomicBool::new(true));
        let snooze = SnoozeController::new();
        snooze.start_snooze(Duration::from_secs(60), || {});
        assert!(snooze.is_active());

        let (items, _) = TrayMenuItems::new(true);

        let event = MenuEvent {
            id: items.resume_item.id().clone(),
        };

        let should_continue = process_menu_event(&event, &items, &paused, &snooze);
        assert!(should_continue);
        assert!(!paused.load(Ordering::Relaxed));
        assert!(!snooze.is_active());
        assert_eq!(snooze.resume_label(), "Resume");
    }

    #[test]
    fn test_process_menu_event_quick_settings_toggles() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp_dir.path()) };

        struct EnvGuard(&'static str);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                // SAFETY: Serialized under TEST_LOCK for test database isolation.
                unsafe { std::env::remove_var(self.0) };
            }
        }
        let _guard = EnvGuard("TAURINE_DATA_DIR");

        let paused = Arc::new(AtomicBool::new(false));
        let snooze = SnoozeController::new();
        let (items, _) = TrayMenuItems::new(false);

        let (initial_instant, initial_boot) = TraySettings::load_quick_settings();

        let event_instant = MenuEvent {
            id: items.instant_expand_item.id().clone(),
        };
        let should_continue = process_menu_event(&event_instant, &items, &paused, &snooze);
        assert!(should_continue);
        let (toggled_instant, _) = TraySettings::load_quick_settings();
        assert_eq!(toggled_instant, !initial_instant);

        // Restore instant expand
        let _ = process_menu_event(&event_instant, &items, &paused, &snooze);
        let (restored_instant, _) = TraySettings::load_quick_settings();
        assert_eq!(restored_instant, initial_instant);

        let event_boot = MenuEvent {
            id: items.start_on_boot_item.id().clone(),
        };
        let should_continue_boot = process_menu_event(&event_boot, &items, &paused, &snooze);
        assert!(should_continue_boot);
        let (_, toggled_boot) = TraySettings::load_quick_settings();
        assert_eq!(toggled_boot, !initial_boot);

        // Restore start on boot
        let _ = process_menu_event(&event_boot, &items, &paused, &snooze);
        let (_, restored_boot) = TraySettings::load_quick_settings();
        assert_eq!(restored_boot, initial_boot);
    }

    #[test]
    fn test_process_menu_event_voice_device_selection() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let temp_dir = tempfile::tempdir().unwrap();
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp_dir.path()) };

        struct EnvGuard(&'static str);
        impl Drop for EnvGuard {
            fn drop(&mut self) {
                // SAFETY: Serialized under TEST_LOCK for test database isolation.
                unsafe { std::env::remove_var(self.0) };
            }
        }
        let _guard = EnvGuard("TAURINE_DATA_DIR");

        let paused = Arc::new(AtomicBool::new(false));
        let snooze = SnoozeController::new();
        let (items, _) = TrayMenuItems::new(false);

        let voice_items = items
            .voice_input_items
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        assert!(!voice_items.is_empty());
        let default_item = &voice_items[0];
        assert_eq!(default_item.name, None);
        assert!(default_item.item.is_checked());

        let event = MenuEvent {
            id: default_item.id.clone(),
        };
        let should_continue = process_menu_event(&event, &items, &paused, &snooze);
        assert!(should_continue);
        assert_eq!(TraySettings::get_voice_input_device(), None);
    }

    #[test]
    fn test_process_menu_event_quit_signals_shutdown() {
        let paused = Arc::new(AtomicBool::new(false));
        let snooze = SnoozeController::new();
        let (items, _) = TrayMenuItems::new(false);

        let event = MenuEvent {
            id: items.quit_item.id().clone(),
        };

        let should_continue = process_menu_event(&event, &items, &paused, &snooze);
        assert!(
            !should_continue,
            "Quit event should signal loop to terminate"
        );
    }

    #[test]
    fn test_tray_spawn_with_visibility() {
        let paused = Arc::new(AtomicBool::new(false));
        let enabled = Arc::new(AtomicBool::new(false));
        spawn(paused, enabled);
    }

    #[test]
    fn test_tray_spawn_with_paused_state() {
        let paused = Arc::new(AtomicBool::new(true));
        let enabled = Arc::new(AtomicBool::new(false));
        spawn(paused, enabled);
    }

    #[test]
    fn test_tray_pause_audio_gate_respects_setting() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set_tray_pause_audio(Arc::new(AtomicBool::new(false)));
        assert!(
            !tray_pause_audio_enabled(),
            "disabled setting must close the fallback cue gate"
        );
        set_tray_pause_audio(Arc::new(AtomicBool::new(true)));
        assert!(
            tray_pause_audio_enabled(),
            "enabled setting must open the fallback cue gate"
        );
    }
}
