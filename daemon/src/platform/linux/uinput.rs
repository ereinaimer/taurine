use evdev::uinput::VirtualDevice;
use evdev::{
    AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, MiscCode, RelativeAxisCode,
};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tracing::{error, trace};

static UINPUT_DEVICE: OnceLock<Mutex<VirtualDevice>> = OnceLock::new();

const UINPUT_STATE_UNINITIALIZED: u8 = 0;
const UINPUT_STATE_INITIALIZING: u8 = 1;
const UINPUT_STATE_READY: u8 = 2;
const UINPUT_STATE_FAILED: u8 = 3;

static UINPUT_STATE: AtomicU8 = AtomicU8::new(UINPUT_STATE_UNINITIALIZED);

pub fn is_uinput_ready() -> bool {
    UINPUT_STATE.load(Ordering::SeqCst) == UINPUT_STATE_READY
}

fn create_virtual_device() -> Result<VirtualDevice, String> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for code in 1..256 {
        keys.insert(KeyCode::new(code as u16));
    }
    keys.insert(KeyCode::BTN_LEFT);
    keys.insert(KeyCode::BTN_RIGHT);
    keys.insert(KeyCode::BTN_MIDDLE);

    let mut relative_axes = AttributeSet::<RelativeAxisCode>::new();
    relative_axes.insert(RelativeAxisCode::REL_X);
    relative_axes.insert(RelativeAxisCode::REL_Y);
    relative_axes.insert(RelativeAxisCode::REL_WHEEL);

    let mut msc = AttributeSet::<MiscCode>::new();
    msc.insert(MiscCode::MSC_SCAN);

    VirtualDevice::builder()
        .map_err(|e| format!("Uinput VirtualDeviceBuilder failed: {}", e))?
        .name(crate::platform::linux::VIRTUAL_DEVICE_NAME)
        .input_id(InputId::new(BusType::BUS_USB, 0x1234, 0x5678, 0x0001))
        .with_keys(&keys)
        .map_err(|e| format!("Failed to set uinput keys: {}", e))?
        .with_relative_axes(&relative_axes)
        .map_err(|e| format!("Failed to set uinput relative axes: {}", e))?
        .with_msc(&msc)
        .map_err(|e| format!("Failed to set uinput msc codes: {}", e))?
        .build()
        .map_err(|e| format!("Failed to create uinput device: {}", e))
}

pub fn init_uinput() -> Result<(), String> {
    match UINPUT_STATE.compare_exchange(
        UINPUT_STATE_UNINITIALIZED,
        UINPUT_STATE_INITIALIZING,
        Ordering::SeqCst,
        Ordering::SeqCst,
    ) {
        Ok(_) => match create_virtual_device() {
            Ok(device) => {
                let _ = UINPUT_DEVICE.set(Mutex::new(device));
                UINPUT_STATE.store(UINPUT_STATE_READY, Ordering::SeqCst);
                Ok(())
            }
            Err(e) => {
                UINPUT_STATE.store(UINPUT_STATE_FAILED, Ordering::SeqCst);
                Err(e)
            }
        },
        Err(current) => {
            if current == UINPUT_STATE_READY {
                Ok(())
            } else if current == UINPUT_STATE_FAILED {
                Err("uinput initialization already failed previously".to_string())
            } else {
                let mut retries = 0;
                while UINPUT_STATE.load(Ordering::SeqCst) == UINPUT_STATE_INITIALIZING
                    && retries < 50
                {
                    thread::sleep(Duration::from_millis(10));
                    retries += 1;
                }
                if UINPUT_STATE.load(Ordering::SeqCst) == UINPUT_STATE_READY {
                    Ok(())
                } else {
                    Err("uinput initialization failed or timed out".to_string())
                }
            }
        }
    }
}

pub fn simulate_key(key: KeyCode, is_press: bool) {
    let events = [
        InputEvent::new(EventType::MISC.0, MiscCode::MSC_SCAN.0, key.code() as i32),
        InputEvent::new(EventType::KEY.0, key.code(), if is_press { 1 } else { 0 }),
    ];
    emit_batch(&events);
}

pub fn emit_batch(events: &[InputEvent]) {
    if events.is_empty() {
        return;
    }

    let state = UINPUT_STATE.load(Ordering::SeqCst);
    if state == UINPUT_STATE_UNINITIALIZED {
        let _ = init_uinput();
    } else if state == UINPUT_STATE_INITIALIZING {
        let mut retries = 0;
        while UINPUT_STATE.load(Ordering::SeqCst) == UINPUT_STATE_INITIALIZING && retries < 50 {
            thread::sleep(Duration::from_millis(10));
            retries += 1;
        }
    }

    if let Some(mutex) = UINPUT_DEVICE.get() {
        if let Ok(mut device) = mutex.lock()
            && let Err(e) = device.emit(events)
        {
            error!("Failed to emit uinput event: {}", e);
        }
    } else {
        trace!("uinput simulate called before initialization or device unavailable");
    }
}

pub fn simulate_keypress(key: KeyCode) {
    simulate_key(key, true);
    if is_uinput_ready() {
        // Increased hold duration for better reliability across different desktop environments.
        thread::sleep(Duration::from_millis(12));
    }
    simulate_key(key, false);
}

pub fn simulate_type_string(s: &str, lookup: &std::collections::HashMap<char, (KeyCode, bool)>) {
    let ready = is_uinput_ready();
    for c in s.chars() {
        if let Some((key, shift)) = lookup.get(&c) {
            if *shift {
                simulate_key(KeyCode::KEY_LEFTSHIFT, true);
                if ready {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            simulate_keypress(*key);
            if *shift {
                if ready {
                    thread::sleep(Duration::from_millis(1));
                }
                simulate_key(KeyCode::KEY_LEFTSHIFT, false);
            }
            if ready {
                // Increase delay between characters for better OS event synchronization.
                thread::sleep(Duration::from_millis(8));
            }
        }
    }
}

pub fn simulate_mouse_button(button: KeyCode, is_press: bool) {
    let events = [InputEvent::new(
        evdev::EventType::KEY.0,
        button.code(),
        if is_press { 1 } else { 0 },
    )];
    emit_batch(&events);
}

pub fn simulate_mouse_scroll(delta: i32) {
    let events = [InputEvent::new(
        evdev::EventType::RELATIVE.0,
        RelativeAxisCode::REL_WHEEL.0,
        delta,
    )];
    emit_batch(&events);
}
