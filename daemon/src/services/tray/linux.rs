use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use ksni::TrayMethods;
use ksni::menu::{CheckmarkItem, MenuItem, StandardItem, SubMenu};
use tokio::sync::mpsc;
use tracing::{error, warn};

use super::settings::TraySettings;
use super::snooze::SnoozeController;

const TOOLTIP: &str = "Taurine";

pub fn spawn(paused: Arc<AtomicBool>, system_tray_enabled: Arc<AtomicBool>) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("tau-tray".to_string())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(build_error) => {
                    error!(%build_error, "failed to build tray event runtime");
                    return;
                }
            };
            runtime.block_on(run_tray(paused, system_tray_enabled));
        })
        .expect("tray thread spawn")
}

#[derive(Clone)]
struct KsniTray {
    paused: Arc<AtomicBool>,
    system_tray_enabled: Arc<AtomicBool>,
    snooze: SnoozeController,
    events: mpsc::UnboundedSender<TrayEvent>,
}

#[derive(Debug, PartialEq, Eq)]
enum TrayEvent {
    Snooze(Duration),
    PauseUntilResumed,
    Resume,
    ToggleInstantExpand,
    ToggleStartOnBoot,
    Quit,
}

impl ksni::Tray for KsniTray {
    fn id(&self) -> String {
        "taurine".into()
    }

    fn title(&self) -> String {
        "Taurine".into()
    }

    fn icon_name(&self) -> String {
        "input-keyboard".into()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        if self.paused.load(Ordering::Relaxed) {
            super::icons::paused_pixmap().to_vec()
        } else {
            super::icons::running_pixmap().to_vec()
        }
    }

    fn status(&self) -> ksni::Status {
        if self.system_tray_enabled.load(Ordering::Relaxed) {
            ksni::Status::Active
        } else {
            ksni::Status::Passive
        }
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: TOOLTIP.into(),
            description: TOOLTIP.into(),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        build_menu(
            self.paused.load(Ordering::Relaxed),
            &self.snooze,
            &self.events,
        )
    }
}

fn build_menu(
    is_paused: bool,
    snooze: &SnoozeController,
    events: &mpsc::UnboundedSender<TrayEvent>,
) -> Vec<MenuItem<KsniTray>> {
    let (instant_expand, start_on_boot) = TraySettings::load_quick_settings();

    let mut menu_items: Vec<MenuItem<KsniTray>> = Vec::new();

    if is_paused {
        let events_clone = events.clone();
        menu_items.push(
            StandardItem {
                label: snooze.resume_label(),
                activate: Box::new(move |_tray: &mut KsniTray| {
                    let _ = events_clone.send(TrayEvent::Resume);
                }),
                ..Default::default()
            }
            .into(),
        );
    } else {
        let events_15m = events.clone();
        let events_30m = events.clone();
        let events_1h = events.clone();
        let events_until_resumed = events.clone();

        let pause_submenu = SubMenu {
            label: "Pause".into(),
            submenu: vec![
                StandardItem {
                    label: "15 minutes".into(),
                    activate: Box::new(move |_tray: &mut KsniTray| {
                        let _ = events_15m.send(TrayEvent::Snooze(Duration::from_secs(15 * 60)));
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "30 minutes".into(),
                    activate: Box::new(move |_tray: &mut KsniTray| {
                        let _ = events_30m.send(TrayEvent::Snooze(Duration::from_secs(30 * 60)));
                    }),
                    ..Default::default()
                }
                .into(),
                StandardItem {
                    label: "1 hour".into(),
                    activate: Box::new(move |_tray: &mut KsniTray| {
                        let _ = events_1h.send(TrayEvent::Snooze(Duration::from_secs(60 * 60)));
                    }),
                    ..Default::default()
                }
                .into(),
                MenuItem::Separator,
                StandardItem {
                    label: "Until resumed".into(),
                    activate: Box::new(move |_tray: &mut KsniTray| {
                        let _ = events_until_resumed.send(TrayEvent::PauseUntilResumed);
                    }),
                    ..Default::default()
                }
                .into(),
            ],
            ..Default::default()
        };
        menu_items.push(pause_submenu.into());
    }

    menu_items.push(MenuItem::Separator);

    let events_instant = events.clone();
    menu_items.push(
        CheckmarkItem {
            label: "Instant Expansion".into(),
            checked: instant_expand,
            activate: Box::new(move |_tray: &mut KsniTray| {
                let _ = events_instant.send(TrayEvent::ToggleInstantExpand);
            }),
            ..Default::default()
        }
        .into(),
    );

    let events_boot = events.clone();
    menu_items.push(
        CheckmarkItem {
            label: "Start on Boot".into(),
            checked: start_on_boot,
            activate: Box::new(move |_tray: &mut KsniTray| {
                let _ = events_boot.send(TrayEvent::ToggleStartOnBoot);
            }),
            ..Default::default()
        }
        .into(),
    );

    menu_items.push(MenuItem::Separator);

    let events_quit = events.clone();
    menu_items.push(
        StandardItem {
            label: "Quit".into(),
            activate: Box::new(move |_tray: &mut KsniTray| {
                let _ = events_quit.send(TrayEvent::Quit);
            }),
            ..Default::default()
        }
        .into(),
    );

    menu_items
}

async fn run_tray(paused: Arc<AtomicBool>, system_tray_enabled: Arc<AtomicBool>) {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let snooze = SnoozeController::new();

    let mut handle = spawn_tray(&paused, &system_tray_enabled, &snooze, &events_tx, true).await;

    let mut last_paused = None;
    let mut last_visible = None;
    let mut live_counter: u32 = 0;
    loop {
        if handle.is_closed() {
            warn!("system tray connection lost, restarting");
            handle = spawn_tray(&paused, &system_tray_enabled, &snooze, &events_tx, false).await;
            last_paused = None;
            last_visible = None;
        }

        let now_visible = system_tray_enabled.load(Ordering::Relaxed);
        if last_visible != Some(now_visible) {
            last_visible = Some(now_visible);
            let _ = handle.update(|_state| {}).await;
        }

        let now_paused = paused.load(Ordering::Relaxed);
        if last_paused != Some(now_paused) {
            let previously_paused = last_paused.unwrap_or(false);
            last_paused = Some(now_paused);

            if previously_paused && !now_paused {
                snooze.cancel();
            }

            let _ = handle.update(|_state| {}).await;
        }

        while let Ok(event) = events_rx.try_recv() {
            match event {
                TrayEvent::Snooze(duration) => {
                    handle_pause();

                    let paused_clone = paused.clone();
                    snooze.start_snooze(duration, move || {
                        paused_clone.store(false, Ordering::Relaxed);
                        handle_resume();
                    });
                    let _ = handle.update(|_| {}).await;
                }
                TrayEvent::PauseUntilResumed => {
                    snooze.cancel();
                    handle_pause();
                    let _ = handle.update(|_| {}).await;
                }
                TrayEvent::Resume => {
                    snooze.cancel();
                    handle_resume();
                    let _ = handle.update(|_| {}).await;
                }
                TrayEvent::ToggleInstantExpand => {
                    let _ = TraySettings::toggle_instant_expand();
                    let _ = handle.update(|_| {}).await;
                }
                TrayEvent::ToggleStartOnBoot => {
                    let _ = TraySettings::toggle_start_on_boot();
                    let _ = handle.update(|_| {}).await;
                }
                TrayEvent::Quit => {
                    handle_shutdown();
                    let _ = handle.shutdown();
                    return;
                }
            }
        }

        // Live-update countdown once per second only while actively snoozed
        if paused.load(Ordering::Relaxed) && snooze.is_active() {
            live_counter += 1;
            if live_counter >= 10 {
                live_counter = 0;
                let _ = handle.update(|_| {}).await;
            }
        } else {
            live_counter = 0;
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn spawn_tray(
    paused: &Arc<AtomicBool>,
    system_tray_enabled: &Arc<AtomicBool>,
    snooze: &SnoozeController,
    events_tx: &mpsc::UnboundedSender<TrayEvent>,
    initial: bool,
) -> ksni::Handle<KsniTray> {
    let mut failed_logged = initial;
    loop {
        let tray = KsniTray {
            paused: paused.clone(),
            system_tray_enabled: system_tray_enabled.clone(),
            snooze: snooze.clone(),
            events: events_tx.clone(),
        };
        match tray.spawn().await {
            Ok(handle) => return handle,
            Err(spawn_error) => {
                if !failed_logged {
                    warn!(%spawn_error, "system tray unavailable, retrying");
                    failed_logged = true;
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

fn handle_pause() {
    if let Some(rt) = crate::TOKIO_HANDLE.get() {
        rt.spawn(async move {
            if let Ok(mut client) = taurine_core::rpc::get_client().await {
                let _ = client.pause(taurine_core::rpc::PauseRequest {}).await;
            }
        });
    }
}

fn handle_resume() {
    if let Some(rt) = crate::TOKIO_HANDLE.get() {
        rt.spawn(async move {
            if let Ok(mut client) = taurine_core::rpc::get_client().await {
                let _ = client.resume(taurine_core::rpc::ResumeRequest {}).await;
            }
        });
    }
}

fn handle_shutdown() {
    if let Some(rt) = crate::TOKIO_HANDLE.get() {
        rt.spawn(async move {
            if let Ok(mut client) = taurine_core::rpc::get_client().await {
                let _ = client.shutdown(taurine_core::rpc::ShutdownRequest {}).await;
            }
        });
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn test_menu_unpaused_structure() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let snooze = SnoozeController::new();
        let items = build_menu(false, &snooze, &tx);

        // Expect: SubMenu(Pause), Separator, Checkmark(Instant), Checkmark(Boot), Separator, Standard(Quit)
        assert_eq!(items.len(), 6);

        match &items[0] {
            MenuItem::SubMenu(submenu) => {
                assert_eq!(submenu.label, "Pause");
                assert_eq!(submenu.submenu.len(), 5); // 15m, 30m, 1h, separator, until resumed
                match &submenu.submenu[0] {
                    MenuItem::Standard(item) => {
                        assert_eq!(item.label, "15 minutes");
                        let mut mock_tray = KsniTray {
                            paused: Arc::new(AtomicBool::new(false)),
                            system_tray_enabled: Arc::new(AtomicBool::new(true)),
                            snooze: snooze.clone(),
                            events: tx.clone(),
                        };
                        (item.activate)(&mut mock_tray);
                        assert_eq!(
                            rx.try_recv().ok(),
                            Some(TrayEvent::Snooze(Duration::from_secs(15 * 60)))
                        );
                    }
                    _ => panic!("Expected StandardItem for 15 minutes"),
                }
                match &submenu.submenu[1] {
                    MenuItem::Standard(item) => {
                        assert_eq!(item.label, "30 minutes");
                        let mut mock_tray = KsniTray {
                            paused: Arc::new(AtomicBool::new(false)),
                            system_tray_enabled: Arc::new(AtomicBool::new(true)),
                            snooze: snooze.clone(),
                            events: tx.clone(),
                        };
                        (item.activate)(&mut mock_tray);
                        assert_eq!(
                            rx.try_recv().ok(),
                            Some(TrayEvent::Snooze(Duration::from_secs(30 * 60)))
                        );
                    }
                    _ => panic!("Expected StandardItem for 30 minutes"),
                }
                match &submenu.submenu[2] {
                    MenuItem::Standard(item) => {
                        assert_eq!(item.label, "1 hour");
                        let mut mock_tray = KsniTray {
                            paused: Arc::new(AtomicBool::new(false)),
                            system_tray_enabled: Arc::new(AtomicBool::new(true)),
                            snooze: snooze.clone(),
                            events: tx.clone(),
                        };
                        (item.activate)(&mut mock_tray);
                        assert_eq!(
                            rx.try_recv().ok(),
                            Some(TrayEvent::Snooze(Duration::from_secs(60 * 60)))
                        );
                    }
                    _ => panic!("Expected StandardItem for 1 hour"),
                }
                assert!(matches!(&submenu.submenu[3], MenuItem::Separator));
                match &submenu.submenu[4] {
                    MenuItem::Standard(item) => {
                        assert_eq!(item.label, "Until resumed");
                        let mut mock_tray = KsniTray {
                            paused: Arc::new(AtomicBool::new(false)),
                            system_tray_enabled: Arc::new(AtomicBool::new(true)),
                            snooze: snooze.clone(),
                            events: tx.clone(),
                        };
                        (item.activate)(&mut mock_tray);
                        assert_eq!(rx.try_recv().ok(), Some(TrayEvent::PauseUntilResumed));
                    }
                    _ => panic!("Expected StandardItem for Until resumed"),
                }
            }
            _ => panic!("Expected SubMenu for unpaused menu item 0"),
        }

        assert!(matches!(&items[1], MenuItem::Separator));

        match &items[2] {
            MenuItem::Checkmark(item) => {
                assert_eq!(item.label, "Instant Expansion");
                let mut mock_tray = KsniTray {
                    paused: Arc::new(AtomicBool::new(false)),
                    system_tray_enabled: Arc::new(AtomicBool::new(true)),
                    snooze: snooze.clone(),
                    events: tx.clone(),
                };
                (item.activate)(&mut mock_tray);
                assert_eq!(rx.try_recv().ok(), Some(TrayEvent::ToggleInstantExpand));
            }
            _ => panic!("Expected CheckmarkItem for Instant Expansion"),
        }

        match &items[3] {
            MenuItem::Checkmark(item) => {
                assert_eq!(item.label, "Start on Boot");
                let mut mock_tray = KsniTray {
                    paused: Arc::new(AtomicBool::new(false)),
                    system_tray_enabled: Arc::new(AtomicBool::new(true)),
                    snooze: snooze.clone(),
                    events: tx.clone(),
                };
                (item.activate)(&mut mock_tray);
                assert_eq!(rx.try_recv().ok(), Some(TrayEvent::ToggleStartOnBoot));
            }
            _ => panic!("Expected CheckmarkItem for Start on Boot"),
        }

        assert!(matches!(&items[4], MenuItem::Separator));

        match &items[5] {
            MenuItem::Standard(item) => {
                assert_eq!(item.label, "Quit");
                let mut mock_tray = KsniTray {
                    paused: Arc::new(AtomicBool::new(false)),
                    system_tray_enabled: Arc::new(AtomicBool::new(true)),
                    snooze: snooze.clone(),
                    events: tx.clone(),
                };
                (item.activate)(&mut mock_tray);
                assert_eq!(rx.try_recv().ok(), Some(TrayEvent::Quit));
            }
            _ => panic!("Expected StandardItem for Quit"),
        }
    }

    #[test]
    fn test_menu_paused_structure() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let snooze = SnoozeController::new();
        let items = build_menu(true, &snooze, &tx);

        // Expect: Standard(Resume), Separator, Checkmark(Instant), Checkmark(Boot), Separator, Standard(Quit)
        assert_eq!(items.len(), 6);

        match &items[0] {
            MenuItem::Standard(item) => {
                assert_eq!(item.label, "Resume");
                let mut mock_tray = KsniTray {
                    paused: Arc::new(AtomicBool::new(true)),
                    system_tray_enabled: Arc::new(AtomicBool::new(true)),
                    snooze: snooze.clone(),
                    events: tx.clone(),
                };
                (item.activate)(&mut mock_tray);
                assert_eq!(rx.try_recv().ok(), Some(TrayEvent::Resume));
            }
            _ => panic!("Expected StandardItem for Resume"),
        }

        assert!(matches!(&items[1], MenuItem::Separator));
        assert!(matches!(&items[2], MenuItem::Checkmark(_)));
        assert!(matches!(&items[3], MenuItem::Checkmark(_)));
        assert!(matches!(&items[4], MenuItem::Separator));
        assert!(matches!(&items[5], MenuItem::Standard(_)));
    }

    #[test]
    fn test_menu_snoozed_structure() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let snooze = SnoozeController::new();
        snooze.start_snooze(Duration::from_secs(15 * 60), || {});
        let items = build_menu(true, &snooze, &tx);

        assert_eq!(items.len(), 6);

        match &items[0] {
            MenuItem::Standard(item) => {
                assert!(
                    item.label.starts_with("Resume (14m ") || item.label == "Resume (15m 00s)",
                    "Unexpected label: {}",
                    item.label
                );
                let mut mock_tray = KsniTray {
                    paused: Arc::new(AtomicBool::new(true)),
                    system_tray_enabled: Arc::new(AtomicBool::new(true)),
                    snooze: snooze.clone(),
                    events: tx.clone(),
                };
                (item.activate)(&mut mock_tray);
                assert_eq!(rx.try_recv().ok(), Some(TrayEvent::Resume));
            }
            _ => panic!("Expected StandardItem for Resume"),
        }
    }
}
