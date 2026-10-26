//! Shared audio input device discovery.
//!
//! The tray microphone menu and the TUI settings picker both enumerate the
//! live system list, so the cpal plumbing lives here instead of in either
//! front end.

use cpal::traits::{DeviceTrait, HostTrait};

/// User-facing device name on cpal 0.18.
///
/// Windows reports the endpoint as a bare name in `name()` and the endpoint
/// description ("Microphone (Realtek(R) Audio)") in `extended().next()` when
/// it differs. Other backends already put the specific label in `name()` with
/// the first extended line equal (ALSA) or empty (CoreAudio), so preferring
/// the first extended line restores the specific device label everywhere.
pub fn friendly_device_name(device: &cpal::Device) -> Option<String> {
    let desc = device.description().ok()?;
    let mut extended = desc.extended();
    if let Some(first) = extended.next()
        && !first.trim().is_empty()
    {
        return Some(first.to_owned());
    }
    Some(desc.name().to_owned())
}

/// Enumerate all unique audio input device names currently available on the
/// system, in host order.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Some(name) = friendly_device_name(&device)
                && !names.contains(&name)
            {
                names.push(name);
            }
        }
    }
    names
}
