use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use taurine_core::engine::{ActiveWindowInfo, EngineState};
use tracing::{debug, error, info};
use wayland_client::{Connection, Dispatch, QueueHandle, protocol::wl_registry};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

static JOIN_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

struct AppState {
    manager: Option<ZwlrForeignToplevelManagerV1>,
    engine_state: Arc<EngineState>,
    active_window_store: Arc<Mutex<Option<String>>>,
    toplevels: Vec<ToplevelInfo>,
}

#[derive(Clone)]
struct ToplevelInfo {
    handle: ZwlrForeignToplevelHandleV1,
    title: Option<String>,
    app_id: Option<String>,
    is_active: bool,
    is_fullscreen: bool,
}

impl AppState {
    fn update_global_state(&self) {
        let mut any_fullscreen = false;
        let mut active_info = None;

        for t in &self.toplevels {
            if t.is_active {
                active_info = Some(t.clone());
            }
            if t.is_fullscreen {
                any_fullscreen = true;
            }
        }

        self.engine_state
            .is_os_fullscreen
            .store(any_fullscreen, Ordering::Relaxed);

        if let Ok(mut lock) = self.active_window_store.lock() {
            if let Some(active) = active_info {
                let info = ActiveWindowInfo {
                    title: active.title,
                    class: active.app_id.clone(),
                    exec_name: active.app_id,
                    exec_path: None,
                };
                *lock = serde_json::to_string(&info).ok();
            } else {
                *lock = None;
            }
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for AppState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            if interface == "zwlr_foreign_toplevel_manager_v1" {
                let manager = registry.bind::<ZwlrForeignToplevelManagerV1, _, _>(
                    name,
                    3, // Bind version 3
                    qh,
                    (),
                );
                state.manager = Some(manager);
            }
        }
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.toplevels.push(ToplevelInfo {
                    handle: toplevel,
                    title: None,
                    app_id: None,
                    is_active: false,
                    is_fullscreen: false,
                });
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                SHUTDOWN.store(true, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for AppState {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let toplevel = match state.toplevels.iter_mut().find(|t| &t.handle == handle) {
            Some(t) => t,
            None => return,
        };

        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                toplevel.title = Some(title);
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                toplevel.app_id = Some(app_id);
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: state_arr } => {
                // state_arr is a byte array representing an array of u32 (enums)
                // wayland_client provides raw bytes for arrays. We parse it:
                let mut is_active = false;
                let mut is_fullscreen = false;

                let chunks = state_arr.chunks_exact(4);
                for chunk in chunks {
                    let val = u32::from_ne_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    if val == zwlr_foreign_toplevel_handle_v1::State::Activated as u32 {
                        is_active = true;
                    } else if val == zwlr_foreign_toplevel_handle_v1::State::Fullscreen as u32 {
                        is_fullscreen = true;
                    }
                }

                toplevel.is_active = is_active;
                toplevel.is_fullscreen = is_fullscreen;
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.toplevels.retain(|t| &t.handle != handle);
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                // A set of updates is complete
                state.update_global_state();
            }
            _ => {}
        }
    }
}

pub fn start_listener(
    engine_state: Arc<EngineState>,
    active_window_store: Arc<Mutex<Option<String>>>,
) {
    let handle = std::thread::Builder::new()
        .name("tau-lnx-wlroots".to_string())
        .spawn(move || {
            let conn = match Connection::connect_to_env() {
                Ok(c) => c,
                Err(e) => {
                    error!(
                        "Failed to connect to Wayland display for wlroots backend: {:?}",
                        e
                    );
                    return;
                }
            };

            let mut event_queue = conn.new_event_queue();
            let qh = event_queue.handle();

            let _registry = conn.display().get_registry(&qh, ());

            let mut state = AppState {
                manager: None,
                engine_state,
                active_window_store,
                toplevels: Vec::new(),
            };

            if let Err(e) = event_queue.roundtrip(&mut state) {
                error!("wlroots event queue roundtrip failed: {:?}", e);
                return;
            }

            if state.manager.is_none() {
                error!("Compositor does not support zwlr_foreign_toplevel_manager_v1");
                return;
            }

            state.update_global_state();

            info!("wlroots toplevel listener started");
            SHUTDOWN.store(false, Ordering::Relaxed);

            while !SHUTDOWN.load(Ordering::Relaxed) {
                match event_queue.dispatch_pending(&mut state) {
                    Ok(0) => {
                        // no events pending, wait for events
                        if let Ok(guard) = event_queue.prepare_read() {
                            let _ = conn.flush();

                            // Simple polling loop with timeout to check shutdown flag
                            let mut fds = [libc::pollfd {
                                fd: conn.backend().poll_fd().as_raw_fd(),
                                events: libc::POLLIN,
                                revents: 0,
                            }];

                            // SAFETY: fds is properly initialized and points to a valid local array.
                            // The timeout (500ms) is a safe integer.
                            let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 500) };
                            if ret > 0 {
                                let _ = guard.read();
                            } else if ret < 0 {
                                let err = std::io::Error::last_os_error();
                                if err.raw_os_error() == Some(libc::EINTR) {
                                    drop(guard);
                                } else {
                                    error!("wlroots poll error: {:?}", err);
                                    break;
                                }
                            } else {
                                drop(guard);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("wlroots dispatch error: {:?}", e);
                        break;
                    }
                }
            }
            debug!("wlroots toplevel listener shutdown");
        })
        .expect("Failed to spawn Linux wlroots listener thread");

    if let Ok(mut lock) = JOIN_HANDLE.lock() {
        *lock = Some(handle);
    }
}

pub fn stop_listener() {
    SHUTDOWN.store(true, Ordering::Relaxed);
    let handle = if let Ok(mut lock) = JOIN_HANDLE.lock() {
        lock.take()
    } else {
        None
    };

    if let Some(h) = handle {
        let _ = h.join();
    }
}
