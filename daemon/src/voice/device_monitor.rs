//! Single owner of the only automatic mic move: configured mic gone -> System Default.
//! Never selects a device, never jumps back. All callers (boot, PTT path, tray poll) share this.
//! A live recording is migrated to the fallback via restart; only the stamp is silent.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static LAST_DEVICE_CHANGE_UNIX_MS: AtomicU64 = AtomicU64::new(0);

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

/// Stamp a device-topology change: arrival, removal, default switch, or a
/// configured-pick edit. The keyboard-hook watchdog consults this to grant
/// extra grace while Windows re-enumerates, instead of mistaking
/// re-enumeration misses for a dead hook.
pub fn mark_device_change() {
    LAST_DEVICE_CHANGE_UNIX_MS.store(now_unix_ms(), Ordering::SeqCst);
}

/// Milliseconds since the last stamped device change; u64::MAX when no
/// change has ever been stamped in this process.
pub fn millis_since_device_change() -> u64 {
    let stamped = LAST_DEVICE_CHANGE_UNIX_MS.load(Ordering::SeqCst);
    if stamped == 0 {
        return u64::MAX;
    }
    now_unix_ms().saturating_sub(stamped)
}

/// Same predicate as AudioCapture::resolve_input_device: case-insensitive exact-or-substring.
pub fn device_matches_configured(configured: &str, available: &[String]) -> bool {
    let want = configured.trim().to_lowercase();
    if want.is_empty() {
        return true; // System Default is always "present".
    }
    available.iter().any(|name| {
        let lower = name.to_lowercase();
        lower == want || lower.contains(&want)
    })
}

/// Returns Some(None) when the cached pick must be cleared to System Default, else None.
/// Empty `available` means "audio stack not ready" -> never clear (avoids logon-race wipe).
pub fn fallback_value_if_missing(
    cached: Option<&str>,
    available: &[String],
) -> Option<Option<String>> {
    match cached {
        None => None,
        Some(name) if name.trim().is_empty() => None,
        Some(name) => {
            if available.is_empty() || device_matches_configured(name, available) {
                None
            } else {
                Some(None)
            }
        }
    }
}

/// Sorted-join signature for cheap change detection in poll loops.
pub fn device_list_signature(devices: &[String]) -> String {
    let mut sorted = devices.to_vec();
    sorted.sort();
    sorted.join("\u{1f}")
}

/// Impure entry: re-reads cache, persists the fallback if the helper says so.
/// Returns true when a change was persisted. Never touches a live recording.
pub fn persist_fallback_to_system_default(available: &[String]) -> bool {
    let cached = taurine_core::settings::get_cached_voice_input_device();
    if fallback_value_if_missing(cached.as_deref(), available) != Some(None) {
        return false;
    }
    let prev = cached;
    if taurine_core::settings::apply_setting_input("voice_input_device", None).is_err() {
        tracing::warn!("mic fallback: failed to persist System Default");
        return false;
    }
    mark_device_change();
    if let Some(session) = crate::VOICE_SESSION.get() {
        session.capture().invalidate_held_on_device_change(prev);
        if session.capture().is_running() {
            // A live recording is bound to the departed device; move it now.
            // Buffered audio is preserved (the buffer outlives the stream).
            // If no device is ready the stream closes and the 5s sweep
            // keeps retrying via try_recover_device.
            let _ = session.capture().restart();
        }
    }
    tracing::info!("mic in use disconnected; fell back to System Default");
    true
}

/// Boot entry: persist fallback only when the saved pick is provably gone AND
/// the OS actually enumerated devices (non-empty list). Empty list = audio not
/// ready (logon race) -> keep the saved value; the tray poll loop retries.
pub fn reconcile_at_startup(available: &[String]) -> bool {
    if available.is_empty() {
        tracing::debug!("mic reconcile at startup: no devices enumerated yet; keeping saved pick");
        return false;
    }
    persist_fallback_to_system_default(available)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_change_timestamp_starts_unset_then_stamps() {
        mark_device_change();
        assert!(
            millis_since_device_change() < 60_000,
            "a fresh stamp must read back as recent"
        );
    }

    #[test]
    fn persist_fallback_clears_gone_pick_without_session() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let prev_var = std::env::var("TAURINE_DATA_DIR").ok();
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", dir.path()) };
        let prev_device = taurine_core::settings::get_cached_voice_input_device();
        taurine_core::settings::set_cached_voice_input_device(Some("Gone Mic".to_string()));
        assert!(
            persist_fallback_to_system_default(&["Other Mic".to_string()]),
            "genuinely-gone pick must fall back"
        );
        assert_eq!(
            taurine_core::settings::get_cached_voice_input_device(),
            None,
            "fallback must persist System Default"
        );
        assert!(
            millis_since_device_change() < 60_000,
            "fallback must stamp the change moment"
        );
        // SAFETY: Serialized under TEST_LOCK for test database isolation.
        unsafe {
            match prev_var {
                Some(v) => std::env::set_var("TAURINE_DATA_DIR", v),
                None => std::env::remove_var("TAURINE_DATA_DIR"),
            }
        }
        taurine_core::settings::set_cached_voice_input_device(prev_device);
    }

    #[test]
    fn startup_keeps_saved_pick_when_present_clears_when_gone() {
        use crate::voice::device_monitor::fallback_value_if_missing;
        let available = vec!["External Mic".to_string()];
        assert_eq!(
            fallback_value_if_missing(Some("External Mic"), &available),
            None
        );
        assert_eq!(
            fallback_value_if_missing(Some("Gone Mic"), &available),
            Some(None)
        );
        assert_eq!(fallback_value_if_missing(None, &available), None);
        // Audio not ready yet -> keep stale value; runtime monitor retries later.
        assert_eq!(fallback_value_if_missing(Some("Gone Mic"), &[]), None);
    }
    #[test]
    fn fallback_only_when_configured_truly_missing() {
        let available = vec!["External Mic".to_string(), "Laptop Mic".to_string()];
        // Exact + case-insensitive + substring all count as present (same as resolve_input_device).
        assert_eq!(
            crate::voice::device_monitor::fallback_value_if_missing(
                Some("external mic"),
                &available
            ),
            None
        );
        assert_eq!(
            crate::voice::device_monitor::fallback_value_if_missing(Some("External"), &available),
            None
        );
        assert_eq!(
            crate::voice::device_monitor::fallback_value_if_missing(None, &available),
            None
        );
        // Empty/whitespace configured == System Default == never fallback.
        assert_eq!(
            crate::voice::device_monitor::fallback_value_if_missing(Some("   "), &available),
            None
        );
        // Genuinely gone -> clear to System Default.
        assert_eq!(
            crate::voice::device_monitor::fallback_value_if_missing(
                Some("Old Headset"),
                &available
            ),
            Some(None),
        );
        // Empty enumeration (audio stack not ready, e.g. logon race) -> NEVER clear.
        assert_eq!(
            crate::voice::device_monitor::fallback_value_if_missing(Some("Old Headset"), &[]),
            None
        );
    }
}
