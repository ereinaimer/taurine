use std::sync::Arc;
use std::sync::atomic::Ordering;
use taurine_core::engine::EngineState;
use tracing::{debug, error};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, EventMask};

pub fn start_listener(state: Arc<EngineState>) {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        debug!("Wayland detected. Fullscreen detection is disabled. Defaulting to false.");
        state.is_os_fullscreen.store(false, Ordering::Relaxed);
        return;
    }

    std::thread::spawn(move || {
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

        let _ = conn.flush();

        update_fullscreen_state(
            &conn,
            root,
            net_active_window,
            net_wm_state,
            net_wm_state_fullscreen,
            &state,
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
                    );
                }
                _ => {}
            }
        }
    });
}

fn update_fullscreen_state(
    conn: &impl Connection,
    root: u32,
    net_active_window: u32,
    net_wm_state: u32,
    net_wm_state_fullscreen: u32,
    state: &Arc<EngineState>,
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
        // Register to catch when the window goes fullscreen without losing focus
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
}
