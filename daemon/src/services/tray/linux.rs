use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use ksni::TrayMethods;
use ksni::menu::StandardItem;
use tokio::sync::mpsc;
use tracing::{error, warn};

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

struct KsniTray {
    paused: Arc<AtomicBool>,
    system_tray_enabled: Arc<AtomicBool>,
    events: mpsc::UnboundedSender<TrayEvent>,
}

enum TrayEvent {
    TogglePause,
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

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let pause_label = if self.paused.load(Ordering::Relaxed) {
            "Resume"
        } else {
            "Pause"
        };
        vec![
            StandardItem {
                label: pause_label.into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.events.send(TrayEvent::TogglePause);
                }),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.events.send(TrayEvent::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

async fn run_tray(paused: Arc<AtomicBool>, system_tray_enabled: Arc<AtomicBool>) {
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();

    let mut handle = spawn_tray(&paused, &system_tray_enabled, &events_tx, true).await;

    let mut last_paused = None;
    let mut last_visible = None;
    loop {
        if handle.is_closed() {
            warn!("system tray connection lost, restarting");
            handle = spawn_tray(&paused, &system_tray_enabled, &events_tx, false).await;
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
            last_paused = Some(now_paused);
            let _ = handle.update(|_state| {}).await;
        }

        while let Ok(event) = events_rx.try_recv() {
            match event {
                TrayEvent::TogglePause => handle_toggle_pause(&paused),
                TrayEvent::Quit => {
                    handle_shutdown();
                    handle.shutdown();
                    return;
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

async fn spawn_tray(
    paused: &Arc<AtomicBool>,
    system_tray_enabled: &Arc<AtomicBool>,
    events_tx: &mpsc::UnboundedSender<TrayEvent>,
    initial: bool,
) -> ksni::Handle<KsniTray> {
    let mut failed_logged = initial;
    loop {
        let tray = KsniTray {
            paused: paused.clone(),
            system_tray_enabled: system_tray_enabled.clone(),
            events: events_tx.clone(),
        };
        match tray.spawn().await {
            Ok(handle) => return handle,
            Err(spawn_error) => {
                if !failed_logged {
                    warn!(%spawn_error, "system tray unavailable, retrying");
                    failed_logged = true;
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

fn handle_toggle_pause(paused: &Arc<AtomicBool>) {
    let paused = paused.clone();
    if let Some(rt) = crate::TOKIO_HANDLE.get() {
        rt.spawn(async move {
            if let Ok(mut client) = taurine_core::rpc::get_client().await {
                if paused.load(Ordering::Relaxed) {
                    let _ = client.resume(taurine_core::rpc::ResumeRequest {}).await;
                } else {
                    let _ = client.pause(taurine_core::rpc::PauseRequest {}).await;
                }
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
