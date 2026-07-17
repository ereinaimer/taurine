use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use taurine_core::engine::{ActiveWindowInfo, EngineState};
use tracing::{debug, error};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, EventMask};
use x11rb::x11_utils::Serialize;

static DUMMY_WINDOW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static JOIN_HANDLE: Mutex<Option<std::thread::JoinHandle<()>>> = Mutex::new(None);

pub fn get_active_window_label_sync() -> Option<String> {
    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let net_active_window = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let active_window = conn
        .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?
        .value32()
        .and_then(|mut iter| iter.next())?;

    if active_window == 0 {
        return None;
    }

    let wm_class = conn
        .intern_atom(false, b"WM_CLASS")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let class_reply = conn
        .get_property(false, active_window, wm_class, AtomEnum::STRING, 0, 1024)
        .ok()?
        .reply()
        .ok()?;

    let class_name = if !class_reply.value.is_empty() {
        let parts: Vec<&str> = std::str::from_utf8(&class_reply.value)
            .ok()?
            .split('\0')
            .collect();
        if parts.len() > 1 && !parts[1].is_empty() {
            parts[1].to_string()
        } else {
            parts[0].to_string()
        }
    } else {
        String::new()
    };

    let wm_name = conn
        .intern_atom(false, b"_NET_WM_NAME")
        .ok()?
        .reply()
        .ok()?
        .atom;

    let title_reply = conn
        .get_property(
            false,
            active_window,
            wm_name,
            conn.intern_atom(false, b"UTF8_STRING")
                .ok()?
                .reply()
                .ok()?
                .atom,
            0,
            1024,
        )
        .ok()?
        .reply()
        .ok()?;

    let title = if !title_reply.value.is_empty() {
        Some(String::from_utf8_lossy(&title_reply.value).to_string())
    } else {
        None
    };

    let class_opt = if class_name.is_empty() {
        None
    } else {
        Some(class_name.clone())
    };
    let exec_name = class_opt.clone();

    let info = ActiveWindowInfo {
        title,
        class: class_opt,
        exec_name,
        exec_path: None,
    };

    serde_json::to_string(&info).ok()
}

pub fn start_listener(state: Arc<EngineState>, active_window_store: Arc<Mutex<Option<String>>>) {
    let handle = std::thread::Builder::new()
        .name("tau-lnx-x11".to_string())
        .spawn(move || {
            let (conn, screen_num) = match x11rb::connect(None) {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to connect to X11 server: {:?}", e);
                    return;
                }
            };

            let screen = &conn.setup().roots[screen_num];
            let root = screen.root;

            if let Err(e) = conn.change_window_attributes(
                root,
                &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                    .event_mask(EventMask::PROPERTY_CHANGE),
            ) {
                error!("Failed to select X11 property changes: {:?}", e);
                return;
            }

            let net_active_window = conn
                .intern_atom(false, b"_NET_ACTIVE_WINDOW")
                .unwrap()
                .reply()
                .unwrap()
                .atom;
            let net_wm_state = conn
                .intern_atom(false, b"_NET_WM_STATE")
                .unwrap()
                .reply()
                .unwrap()
                .atom;
            let net_wm_state_fullscreen = conn
                .intern_atom(false, b"_NET_WM_STATE_FULLSCREEN")
                .unwrap()
                .reply()
                .unwrap()
                .atom;

            let dummy_window = match conn.generate_id() {
                Ok(id) => id,
                Err(e) => {
                    error!("Failed to generate dummy window ID: {:?}", e);
                    return;
                }
            };

            if let Err(e) = conn.create_window(
                0,
                dummy_window,
                root,
                0,
                0,
                1,
                1,
                0,
                x11rb::protocol::xproto::WindowClass::INPUT_ONLY,
                x11rb::COPY_FROM_PARENT,
                &x11rb::protocol::xproto::CreateWindowAux::new(),
            ) {
                error!("Failed to create dummy window: {:?}", e);
                return;
            }

            let taurine_shutdown_atom = match conn
                .intern_atom(false, b"TAURINE_SHUTDOWN")
            {
                Ok(cookie) => match cookie.reply() {
                    Ok(reply) => reply.atom,
                    Err(e) => {
                        error!("Failed to reply TAURINE_SHUTDOWN atom: {:?}", e);
                        return;
                    }
                },
                Err(e) => {
                    error!("Failed to intern TAURINE_SHUTDOWN atom: {:?}", e);
                    return;
                }
            };

            DUMMY_WINDOW.store(dummy_window, Ordering::Relaxed);

            let _ = conn.flush();

            update_fullscreen_state(
                &conn,
                root,
                net_active_window,
                net_wm_state,
                net_wm_state_fullscreen,
                &state,
                &active_window_store,
            );

            while let Ok(event) = conn.wait_for_event() {
                match event {
                    x11rb::protocol::Event::PropertyNotify(ev)
                        if ev.atom == net_active_window || ev.atom == net_wm_state =>
                    {
                        update_fullscreen_state(
                            &conn,
                            root,
                            net_active_window,
                            net_wm_state,
                            net_wm_state_fullscreen,
                            &state,
                            &active_window_store,
                        );
                    }
                    x11rb::protocol::Event::ClientMessage(ev)
                        if ev.window == dummy_window && ev.type_ == taurine_shutdown_atom =>
                    {
                        debug!("Shutdown client message received. Exiting X11 fullscreen listener thread.");
                        let _ = conn.destroy_window(dummy_window);
                        let _ = conn.flush();
                        break;
                    }
                    _ => {}
                }
            }
        })
        .expect("Failed to spawn Linux X11 listener thread");

    if let Ok(mut lock) = JOIN_HANDLE.lock() {
        *lock = Some(handle);
    }
}

pub fn stop_listener() {
    let dummy_window = DUMMY_WINDOW.swap(0, Ordering::Relaxed);
    if dummy_window != 0
        && let Ok((conn, _)) = x11rb::connect(None)
        && let Ok(cookie) = conn.intern_atom(false, b"TAURINE_SHUTDOWN")
        && let Ok(taurine_shutdown_atom) = cookie.reply()
    {
        let event = x11rb::protocol::xproto::ClientMessageEvent {
            response_type: x11rb::protocol::xproto::CLIENT_MESSAGE_EVENT,
            format: 32,
            sequence: 0,
            window: dummy_window,
            type_: taurine_shutdown_atom.atom,
            data: x11rb::protocol::xproto::ClientMessageData::from([0u32; 5]),
        };
        let raw_event = event.serialize();
        let _ = conn.send_event(
            false,
            dummy_window,
            x11rb::protocol::xproto::EventMask::NO_EVENT,
            raw_event,
        );
        let _ = conn.flush();
    }
    let handle = if let Ok(mut lock) = JOIN_HANDLE.lock() {
        lock.take()
    } else {
        None
    };

    if let Some(h) = handle {
        let res = h.join();
        if let Err(e) = res {
            error!("Error joining Linux X11 listener thread: {:?}", e);
        }
    }
}

fn update_fullscreen_state(
    conn: &impl Connection,
    root: u32,
    net_active_window: u32,
    net_wm_state: u32,
    net_wm_state_fullscreen: u32,
    state: &Arc<EngineState>,
    active_window_store: &Arc<Mutex<Option<String>>>,
) {
    let mut is_full = false;
    let active_window_val = conn
        .get_property(
            false,
            root,
            net_active_window,
            x11rb::protocol::xproto::AtomEnum::WINDOW,
            0,
            1,
        )
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().and_then(|mut iter| iter.next()));

    if let Some(active_window) = active_window_val {
        let _ = conn.change_window_attributes(
            active_window,
            &x11rb::protocol::xproto::ChangeWindowAttributesAux::new()
                .event_mask(EventMask::PROPERTY_CHANGE),
        );

        let state_reply = conn
            .get_property(
                false,
                active_window,
                net_wm_state,
                x11rb::protocol::xproto::AtomEnum::ATOM,
                0,
                1024,
            )
            .ok()
            .and_then(|cookie| cookie.reply().ok());

        if let Some(states) = state_reply.as_ref().and_then(|reply| reply.value32()) {
            for s in states {
                if s == net_wm_state_fullscreen {
                    is_full = true;
                    break;
                }
            }
        }
    }

    state.is_os_fullscreen.store(is_full, Ordering::Relaxed);

    if let Ok(mut lock) = active_window_store.lock() {
        *lock = get_active_window_label_sync();
    }
}
