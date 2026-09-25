//! Single owner of the only automatic mic move: configured mic gone -> System Default.
//! Never selects a device, never jumps back. All callers (boot, PTT path, tray poll) share this.

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
    if let Some(session) = crate::VOICE_SESSION.get() {
        session.capture().invalidate_held_on_device_change(prev);
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
