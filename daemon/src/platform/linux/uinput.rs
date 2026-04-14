use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, EventType, InputEvent, KeyCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use tracing::error;

static UINPUT_DEVICE: OnceLock<Mutex<VirtualDevice>> = OnceLock::new();
static UINPUT_INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn init_uinput() -> Result<(), String> {
    if UINPUT_INITIALIZED.load(Ordering::SeqCst) {
        return Ok(());
    }

    let mut keys = AttributeSet::<KeyCode>::new();
    for code in 1..256 {
        keys.insert(KeyCode::new(code as u16));
    }

    let device = VirtualDevice::builder()
        .map_err(|e| format!("Uinput VirtualDeviceBuilder failed: {}", e))?
        .name(crate::platform::linux::VIRTUAL_DEVICE_NAME)
        .vendor_id(0x1234)
        .product_id(0x5678)
        .with_keys(&keys)
        .map_err(|e| format!("Failed to set uinput keys: {}", e))?
        .build()
        .map_err(|e| format!("Failed to create uinput device: {}", e))?;

    UINPUT_DEVICE
        .set(Mutex::new(device))
        .map_err(|_| "Failed to store uinput device in OnceLock".to_string())?;

    UINPUT_INITIALIZED.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn simulate_key(key: KeyCode, is_press: bool) {
    if let Some(mutex) = UINPUT_DEVICE.get() {
        if let Ok(mut device) = mutex.lock() {
            let value = if is_press { 1 } else { 0 };
            let event = InputEvent::new(EventType::KEY.0, key.code(), value);
            // Must emit EV_SYN after creating actual events.
            let syn = InputEvent::new(EventType::SYNCHRONIZATION.0, 0, 0);
            if let Err(e) = device.emit(&[event, syn]) {
                error!("Failed to emit uinput event: {}", e);
            }
        }
    } else {
        error!("uinput simulate called before initialization");
    }
}

pub fn simulate_keypress(key: KeyCode) {
    simulate_key(key, true);
    // Increased hold duration for better reliability across different desktop environments.
    thread::sleep(Duration::from_millis(12));
    simulate_key(key, false);
}

pub fn simulate_type_string(s: &str, lookup: &std::collections::HashMap<char, (KeyCode, bool)>) {
    for c in s.chars() {
        if let Some((key, shift)) = lookup.get(&c) {
            if *shift {
                simulate_key(KeyCode::KEY_LEFTSHIFT, true);
                thread::sleep(Duration::from_millis(1));
            }
            simulate_keypress(*key);
            if *shift {
                thread::sleep(Duration::from_millis(1));
                simulate_key(KeyCode::KEY_LEFTSHIFT, false);
            }
            // Increase delay between characters for better OS event synchronization.
            thread::sleep(Duration::from_millis(8));
        }
    }
}
