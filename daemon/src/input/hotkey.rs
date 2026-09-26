#[cfg(target_os = "linux")]
use evdev::KeyCode;
#[cfg(not(target_os = "linux"))]
use rdev::{Event, EventType};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use taurine_core::keys::{
    Hotkey, KeyPress, LogicalKey, Modifier, ModifierState, Modifiers, hotkey_matches, parse_hotkey,
};

#[cfg(target_os = "linux")]
use crate::input::hotkey_evaluator::logical_key_from_evdev;
#[cfg(not(target_os = "linux"))]
use crate::input::hotkey_evaluator::logical_key_from_rdev;

pub static PTT_KEY_DOWN: AtomicBool = AtomicBool::new(false);
pub static HANDSFREE_KEY_DOWN: AtomicBool = AtomicBool::new(false);

// Parsed voice hotkey specs, refreshed on settings change so the hook thread
// never pays a string parse per keystroke. Empty/unparsable settings cache
// as None (hotkey inert), matching the previous per-event behaviour.
static CACHED_PTT_SPEC: RwLock<Option<VoiceHotkeySpec>> = RwLock::new(None);
static CACHED_HANDSFREE_SPEC: RwLock<Option<VoiceHotkeySpec>> = RwLock::new(None);

pub fn refresh_cached_voice_specs(ptt: &str, handsfree: &str) {
    *CACHED_PTT_SPEC.write().expect("voice ptt spec lock") = VoiceHotkeySpec::parse(ptt);
    *CACHED_HANDSFREE_SPEC
        .write()
        .expect("voice handsfree spec lock") = VoiceHotkeySpec::parse(handsfree);
}

pub fn cached_voice_ptt_spec() -> Option<VoiceHotkeySpec> {
    CACHED_PTT_SPEC.read().expect("voice ptt spec lock").clone()
}

pub fn cached_voice_handsfree_spec() -> Option<VoiceHotkeySpec> {
    CACHED_HANDSFREE_SPEC
        .read()
        .expect("voice handsfree spec lock")
        .clone()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub hotkey: Hotkey,
}

pub fn parse_pause_hotkey_setting(setting: &str) -> Option<HotkeySpec> {
    parse_hotkey(setting)
        .ok()
        .map(|hotkey| HotkeySpec { hotkey })
}

/// Quiet window for pause-press bursts: presses landing within this gap of
/// each other belong to one burst (key smash guard). No time-debounce on
/// individual presses - every distinct chord press queues exactly one token.
pub const QUIET_WINDOW_MS: u64 = 300;

/// Odd bursts toggle the pause flag; even bursts settle back to the
/// pre-burst state (a double-tap smashes back to where it started).
pub fn burst_should_toggle(count: u64) -> bool {
    count % 2 == 1
}

pub fn notify_pause_transition(tx: &tokio::sync::mpsc::Sender<bool>, now_paused: bool) {
    if let Err(e) = tx.try_send(now_paused) {
        tracing::warn!(
            now_paused,
            error = %e,
            "Pause transition notification dropped; coordinator may diverge until next toggle"
        );
    }
}

/// Process-global press sender. The hook paths (Windows/macOS supervisor
/// chain, Linux evdev chain) never owned a channel for this: threading one
/// more param through both supervisor chains is a far bigger diff than a
/// single global following the existing VOICE_SESSION precedent. Set once at
/// daemon startup; tests swap it under TEST_LOCK.
static PAUSE_PRESS_TX: Mutex<Option<std::sync::mpsc::Sender<()>>> = Mutex::new(None);

/// Lifetime press counter for the per-press debug line. The hook thread
/// cannot know burst boundaries (that is the counter's job); this is the
/// running total, not the current burst size.
static PAUSE_PRESS_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_pause_press_sender(tx: std::sync::mpsc::Sender<()>) {
    *PAUSE_PRESS_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tx);
}

#[cfg(test)]
pub fn clear_pause_press_sender() {
    *PAUSE_PRESS_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

/// Queue one press token for the burst counter. Never blocks (unbounded
/// std channel); a missing or dead counter only logs - the press-side cue
/// already played, and the next press re-syncs.
pub fn push_pause_press() {
    let press = PAUSE_PRESS_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let guard = PAUSE_PRESS_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_ref() {
        Some(tx) => {
            if tx.send(()).is_err() {
                tracing::debug!(press, "Pause press dropped; counter thread is gone");
            } else {
                tracing::debug!(press, "Pause press queued for burst counter");
            }
        }
        None => {
            tracing::debug!(press, "Pause press with no counter; ignoring");
        }
    }
}

/// Optimistic tray-icon preview: flipped synchronously on every distinct
/// pause-chord press (same arm as the audio cue), so the icon reacts
/// instantly while the burst counter still owns the deferred toggle.
/// Best-effort hint only — the committed `paused` flag stays authoritative
/// for menus, notifications, and subsystem suspend/resume.
static PAUSE_ICON_PREVIEW: AtomicBool = AtomicBool::new(false);

/// Current icon hint. Tray loops render this; everything else reads `paused`.
pub fn pause_icon_preview() -> bool {
    PAUSE_ICON_PREVIEW.load(Ordering::Relaxed)
}

/// Flip the hint on each distinct press. Odd flips predict the toggle; even
/// flips swing back by themselves, so settled-even bursts need no revert.
pub fn push_pause_icon_preview() {
    PAUSE_ICON_PREVIEW.fetch_xor(true, Ordering::Relaxed);
}

/// Reconcile the hint with committed state. Called by the burst counter
/// after settling (authoritative) and by tray loops when they observe a
/// committed change from any other path (menu, RPC, snooze expiry).
/// Tests also use this to reset the global under TEST_LOCK.
pub fn sync_pause_icon_preview(committed_paused: bool) {
    PAUSE_ICON_PREVIEW.store(committed_paused, Ordering::Relaxed);
}

/// Process-global transition sender for paths that never had the channel
/// threaded to them (tray/snooze fallbacks). Set once at daemon startup.
static PAUSE_TRANSITION_TX: Mutex<Option<tokio::sync::mpsc::Sender<bool>>> = Mutex::new(None);

pub fn set_pause_transition_sender(tx: tokio::sync::mpsc::Sender<bool>) {
    *PAUSE_TRANSITION_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(tx);
}

// Reader for the tray fallback path in tray::native, which only builds on
// Windows and macOS; Linux tray never reads the sender.
#[cfg(any(windows, target_os = "macos"))]
pub fn pause_transition_sender() -> Option<tokio::sync::mpsc::Sender<bool>> {
    PAUSE_TRANSITION_TX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Count-then-actuate worker: blocks for the first press of a burst, then
/// drains within the quiet window. Odd bursts flip the shared flag and
/// notify; even bursts only log. All senders dropped (shutdown) exits
/// without applying a partial count.
pub fn spawn_pause_counter(
    press_rx: std::sync::mpsc::Receiver<()>,
    paused: Arc<AtomicBool>,
    pause_transition_tx: tokio::sync::mpsc::Sender<bool>,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("tau-pause-count".to_string())
        .spawn(move || {
            loop {
                if press_rx.recv().is_err() {
                    break;
                }
                let mut count: u64 = 1;
                loop {
                    match press_rx.recv_timeout(std::time::Duration::from_millis(QUIET_WINDOW_MS)) {
                        Ok(()) => count += 1,
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                }
                tracing::debug!(count, "Pause burst settled");
                if burst_should_toggle(count) {
                    // Atomic flip: concurrent tray fallbacks compose instead
                    // of losing an update to a load/store race.
                    let was_paused = paused.fetch_xor(true, Ordering::SeqCst);
                    let now_paused = !was_paused;
                    // Authoritative reconcile: the preview already flipped odd
                    // times to this value; store again to heal drift from
                    // external paths that moved `paused` mid-burst.
                    sync_pause_icon_preview(now_paused);
                    notify_pause_transition(&pause_transition_tx, now_paused);
                } else {
                    // Even bursts swing the preview back by themselves (one
                    // flip per press); re-sync to heal concurrent drift.
                    sync_pause_icon_preview(paused.load(Ordering::SeqCst));
                }
            }
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceHotkeySpec {
    /// A chord composed purely of modifiers, e.g. "win+lctrl" or "win+lctrl+lalt".
    ModifierChord(Modifiers),
    /// A standard hotkey with a non-modifier base key and optional modifiers, e.g. "ctrl+shift+v" or "f8".
    Standard(Hotkey),
}

impl VoiceHotkeySpec {
    pub fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }

        if let Ok(hotkey) = parse_hotkey(trimmed) {
            return Some(Self::Standard(hotkey));
        }

        let tokens = taurine_core::keys::split_tokens(trimmed).ok()?;
        let mut modifiers = Modifiers::new();
        for token in tokens {
            let modifier = Modifier::from_alias(&token)?;
            modifiers.insert(modifier).ok()?;
        }

        if modifiers.is_empty() {
            None
        } else {
            Some(Self::ModifierChord(modifiers))
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn matches_press(&self, event: &Event, active_modifiers: Modifiers) -> bool {
        let EventType::KeyPress(key) = event.event_type else {
            return false;
        };
        let Some(logical) = logical_key_from_rdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                        && req_mods.matches_active(active_modifiers)
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                logical == hotkey.key && hotkey.modifiers.matches_active(active_modifiers)
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn matches_release(&self, event: &Event) -> bool {
        let EventType::KeyRelease(key) = event.event_type else {
            return false;
        };
        let Some(logical) = logical_key_from_rdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                if logical == hotkey.key {
                    true
                } else if let LogicalKey::Modifier(m) = logical
                    && hotkey.modifiers.family_state(m.family()) != ModifierState::Absent
                {
                    true
                } else {
                    false
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub fn matches_press_evdev(
        &self,
        key: KeyCode,
        is_press: bool,
        active_modifiers: Modifiers,
    ) -> bool {
        if !is_press {
            return false;
        }
        let Some(logical) = logical_key_from_evdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                        && req_mods.matches_active(active_modifiers)
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                logical == hotkey.key && hotkey.modifiers.matches_active(active_modifiers)
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub fn matches_release_evdev(&self, key: KeyCode, is_release: bool) -> bool {
        if !is_release {
            return false;
        }
        let Some(logical) = logical_key_from_evdev(key) else {
            return false;
        };

        match self {
            Self::ModifierChord(req_mods) => match logical {
                LogicalKey::Modifier(m) => {
                    req_mods.family_state(m.family()) != ModifierState::Absent
                }
                _ => false,
            },
            Self::Standard(hotkey) => {
                if logical == hotkey.key {
                    true
                } else if let LogicalKey::Modifier(m) = logical
                    && hotkey.modifiers.family_state(m.family()) != ModifierState::Absent
                {
                    true
                } else {
                    false
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn is_pause_chord(event: &Event, modifiers: Modifiers, spec: &HotkeySpec) -> bool {
    let EventType::KeyPress(key) = event.event_type else {
        return false;
    };

    let Some(key) = logical_key_from_rdev(key) else {
        return false;
    };

    hotkey_matches(spec.hotkey, KeyPress { modifiers, key })
}

#[cfg(target_os = "linux")]
pub fn is_pause_chord_evdev(
    key: KeyCode,
    is_press: bool,
    modifiers: Modifiers,
    spec: &HotkeySpec,
) -> bool {
    if !is_press {
        return false;
    }

    let Some(key) = logical_key_from_evdev(key) else {
        return false;
    };

    hotkey_matches(spec.hotkey, KeyPress { modifiers, key })
}

#[cfg(test)]
#[cfg(not(target_os = "linux"))]
mod tests {
    use super::*;
    use rdev::{EventType, Key};
    use taurine_core::keys::{Modifier, Modifiers, parse_hotkey};

    fn ev(event_type: EventType) -> Event {
        Event {
            event_type,
            time: std::time::SystemTime::now(),
            name: None,
        }
    }

    fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
        let mut bitset = Modifiers::new();
        for modifier in modifiers {
            bitset.insert_active(*modifier);
        }
        bitset
    }

    #[test]
    fn parse_accepts_case_and_spacing_variants_without_implying_shift() {
        let expected = parse_hotkey("alt+`").expect("pause hotkey should parse");

        assert_eq!(
            parse_pause_hotkey_setting("Alt + `")
                .expect("default pause hotkey should parse")
                .hotkey,
            expected
        );
        assert_eq!(
            parse_pause_hotkey_setting("alt + `")
                .expect("lowercase pause hotkey should parse")
                .hotkey,
            expected
        );
        assert_eq!(
            parse_pause_hotkey_setting("Alt+`")
                .expect("compact pause hotkey should parse")
                .hotkey,
            expected
        );
    }

    #[test]
    fn matches_only_on_exact_keypress_with_required_modifiers() {
        let spec = parse_pause_hotkey_setting("Alt + `").unwrap();
        let press_bt = ev(EventType::KeyPress(Key::BackQuote));
        assert!(is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
        assert!(!is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftCtrl]),
            &spec
        ));
        assert!(!is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftShift]),
            &spec
        ));
        assert!(!is_pause_chord(&press_bt, Modifiers::new(), &spec));
        assert!(!is_pause_chord(
            &press_bt,
            modifiers_with(&[Modifier::LeftAlt, Modifier::LeftShift]),
            &spec
        ));

        let release_bt = ev(EventType::KeyRelease(Key::BackQuote));
        assert!(!is_pause_chord(
            &release_bt,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
    }

    #[test]
    fn voice_hotkey_spec_parses_modifier_chords_and_standards() {
        let ptt = VoiceHotkeySpec::parse("win+lctrl").expect("modifier chord should parse");
        assert!(matches!(ptt, VoiceHotkeySpec::ModifierChord(_)));

        let hf =
            VoiceHotkeySpec::parse("win+lctrl+lalt").expect("3-key modifier chord should parse");
        assert!(matches!(hf, VoiceHotkeySpec::ModifierChord(_)));

        let std = VoiceHotkeySpec::parse("ctrl+shift+v").expect("standard hotkey should parse");
        assert!(matches!(std, VoiceHotkeySpec::Standard(_)));

        let fn_key = VoiceHotkeySpec::parse("f8").expect("function key should parse");
        assert!(matches!(fn_key, VoiceHotkeySpec::Standard(_)));

        assert!(VoiceHotkeySpec::parse("").is_none());
        assert!(VoiceHotkeySpec::parse("invalid_nonexistent_key_name").is_none());
    }

    #[test]
    fn cached_voice_specs_update_only_on_refresh() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        refresh_cached_voice_specs("ctrl+shift+v", "f8");
        assert_eq!(
            cached_voice_ptt_spec(),
            VoiceHotkeySpec::parse("ctrl+shift+v")
        );
        assert_eq!(cached_voice_handsfree_spec(), VoiceHotkeySpec::parse("f8"));

        // Swap the source strings without a refresh: the press path must
        // keep matching the stale cached specs (no per-event re-parse).
        taurine_core::settings::set_cached_voice_ptt_hotkey("f9".to_string());
        taurine_core::settings::set_cached_voice_handsfree_hotkey("f10".to_string());
        assert_eq!(
            cached_voice_ptt_spec(),
            VoiceHotkeySpec::parse("ctrl+shift+v")
        );
        assert_eq!(cached_voice_handsfree_spec(), VoiceHotkeySpec::parse("f8"));

        // A refresh picks up the new settings.
        refresh_cached_voice_specs("f9", "f10");
        assert_eq!(cached_voice_ptt_spec(), VoiceHotkeySpec::parse("f9"));
        assert_eq!(cached_voice_handsfree_spec(), VoiceHotkeySpec::parse("f10"));

        // Empty or unparsable settings stay inert (None), as before.
        refresh_cached_voice_specs("", "invalid_nonexistent_key_name");
        assert!(cached_voice_ptt_spec().is_none());
        assert!(cached_voice_handsfree_spec().is_none());

        // Restore defaults so other tests see a clean cache.
        taurine_core::settings::set_cached_voice_ptt_hotkey("win+lctrl".to_string());
        taurine_core::settings::set_cached_voice_handsfree_hotkey("win+lctrl+lalt".to_string());
        refresh_cached_voice_specs("win+lctrl", "win+lctrl+lalt");
    }

    #[test]
    fn voice_hotkey_spec_matches_modifier_chord_press_and_release() {
        let spec = VoiceHotkeySpec::parse("win+lctrl").unwrap();

        // Press LeftCtrl with LeftMeta and LeftCtrl active
        let press_lctrl = ev(EventType::KeyPress(Key::ControlLeft));
        let mods_meta_ctrl = modifiers_with(&[Modifier::LeftMeta, Modifier::LeftCtrl]);
        assert!(spec.matches_press(&press_lctrl, mods_meta_ctrl));

        // Press LeftCtrl without LeftMeta active
        let mods_ctrl_only = modifiers_with(&[Modifier::LeftCtrl]);
        assert!(!spec.matches_press(&press_lctrl, mods_ctrl_only));

        // Press non-modifier key A with both modifiers active
        let press_a = ev(EventType::KeyPress(Key::KeyA));
        assert!(!spec.matches_press(&press_a, mods_meta_ctrl));

        // Release LeftCtrl
        let release_lctrl = ev(EventType::KeyRelease(Key::ControlLeft));
        assert!(spec.matches_release(&release_lctrl));

        // Release LeftMeta
        let release_meta = ev(EventType::KeyRelease(Key::MetaLeft));
        assert!(spec.matches_release(&release_meta));

        // Release unrelated key
        let release_a = ev(EventType::KeyRelease(Key::KeyA));
        assert!(!spec.matches_release(&release_a));
    }

    #[test]
    fn voice_hotkey_spec_matches_standard_press_and_release() {
        let spec = VoiceHotkeySpec::parse("ctrl+shift+v").unwrap();

        let press_v = ev(EventType::KeyPress(Key::KeyV));
        let mods_ctrl_shift = modifiers_with(&[Modifier::LeftCtrl, Modifier::LeftShift]);
        assert!(spec.matches_press(&press_v, mods_ctrl_shift));

        let mods_ctrl_only = modifiers_with(&[Modifier::LeftCtrl]);
        assert!(!spec.matches_press(&press_v, mods_ctrl_only));

        let release_v = ev(EventType::KeyRelease(Key::KeyV));
        assert!(spec.matches_release(&release_v));

        let release_ctrl = ev(EventType::KeyRelease(Key::ControlLeft));
        assert!(spec.matches_release(&release_ctrl));

        let release_a = ev(EventType::KeyRelease(Key::KeyA));
        assert!(!spec.matches_release(&release_a));
    }

    #[test]
    fn burst_should_toggle_only_on_odd_counts() {
        assert!(!super::burst_should_toggle(0));
        assert!(super::burst_should_toggle(1));
        assert!(!super::burst_should_toggle(2));
        assert!(super::burst_should_toggle(3));
        assert!(super::burst_should_toggle(5));
    }

    #[test]
    fn pause_counter_applies_odd_bursts_and_drops_even_ones() {
        use std::sync::atomic::Ordering;
        use std::time::{Duration, Instant};

        // Poll a condition until it holds or the deadline passes; fixed
        // sleeps flake on loaded runners where the counter thread may not be
        // scheduled within a small margin past the quiet window.
        fn poll_until(deadline: Instant, mut cond: impl FnMut() -> bool) -> bool {
            while !cond() {
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            true
        }

        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        super::clear_pause_press_sender();
        let paused = Arc::new(AtomicBool::new(false));
        let (transition_tx, mut transition_rx) = tokio::sync::mpsc::channel::<bool>(8);
        let (press_tx, press_rx) = std::sync::mpsc::channel::<()>();
        let counter =
            super::spawn_pause_counter(press_rx, paused.clone(), transition_tx).expect("counter");
        super::set_pause_press_sender(press_tx);

        // Single press: toggles once the quiet window elapses.
        super::push_pause_press();
        assert!(
            poll_until(Instant::now() + Duration::from_secs(10), || paused
                .load(Ordering::Relaxed)),
            "odd burst must toggle"
        );
        assert_eq!(transition_rx.try_recv(), Ok(true));
        assert!(
            transition_rx.try_recv().is_err(),
            "exactly one transition per burst"
        );

        // Rapid pair: settles even, no toggle, no notification. Watch the
        // channel well past the quiet window; an even burst never notifies,
        // so any message here is a failure regardless of scheduling delays.
        super::push_pause_press();
        super::push_pause_press();
        let quiet_until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < quiet_until {
            if let Ok(v) = transition_rx.try_recv() {
                panic!("even burst must not notify, got {v:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            paused.load(Ordering::Relaxed),
            "even burst must not toggle back"
        );

        super::clear_pause_press_sender();
        counter.join().expect("counter exits once senders drop");
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;
    use taurine_core::keys::{Modifier, Modifiers};

    fn modifiers_with(modifiers: &[Modifier]) -> Modifiers {
        let mut bitset = Modifiers::new();
        for modifier in modifiers {
            bitset.insert_active(*modifier);
        }
        bitset
    }

    #[test]
    fn linux_pause_chord_matches_only_on_exact_keypress_with_required_modifiers() {
        let spec = parse_pause_hotkey_setting("Alt + `").unwrap();

        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            Modifiers::new(),
            &spec
        ));
        assert!(is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftCtrl]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftShift]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            true,
            modifiers_with(&[Modifier::LeftAlt, Modifier::LeftShift]),
            &spec
        ));
        assert!(!is_pause_chord_evdev(
            KeyCode::KEY_GRAVE,
            false,
            modifiers_with(&[Modifier::LeftAlt]),
            &spec
        ));
    }

    #[test]
    fn linux_voice_hotkey_spec_matches_modifier_chords_and_standards() {
        let spec = VoiceHotkeySpec::parse("win+lctrl").unwrap();

        let mods_meta_ctrl = modifiers_with(&[Modifier::LeftMeta, Modifier::LeftCtrl]);
        assert!(spec.matches_press_evdev(KeyCode::KEY_LEFTCTRL, true, mods_meta_ctrl));
        assert!(!spec.matches_press_evdev(KeyCode::KEY_LEFTCTRL, false, mods_meta_ctrl));
        assert!(!spec.matches_press_evdev(
            KeyCode::KEY_LEFTCTRL,
            true,
            modifiers_with(&[Modifier::LeftCtrl])
        ));

        assert!(spec.matches_release_evdev(KeyCode::KEY_LEFTCTRL, true));
        assert!(spec.matches_release_evdev(KeyCode::KEY_LEFTMETA, true));
        assert!(!spec.matches_release_evdev(KeyCode::KEY_A, true));
    }

    #[test]
    fn linux_cached_voice_specs_drive_evdev_press_and_release_matching() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        refresh_cached_voice_specs("ctrl+shift+v", "f8");

        // Press path resolves through the cache — the same call chain as the
        // evdev listener, with no per-keystroke re-parse.
        let mods_ctrl_shift = modifiers_with(&[Modifier::LeftCtrl, Modifier::LeftShift]);
        assert!(
            cached_voice_ptt_spec().is_some_and(|spec| spec.matches_press_evdev(
                KeyCode::KEY_V,
                true,
                mods_ctrl_shift
            ))
        );
        assert!(
            cached_voice_handsfree_spec().is_some_and(|spec| spec.matches_press_evdev(
                KeyCode::KEY_F8,
                true,
                Modifiers::new()
            ))
        );
        assert!(
            cached_voice_ptt_spec().is_some_and(|spec| !spec.matches_press_evdev(
                KeyCode::KEY_V,
                true,
                Modifiers::new()
            ))
        );

        // Release path resolves through the cache as well.
        assert!(
            cached_voice_ptt_spec()
                .is_some_and(|spec| spec.matches_release_evdev(KeyCode::KEY_V, true))
        );
        assert!(
            cached_voice_handsfree_spec()
                .is_some_and(|spec| spec.matches_release_evdev(KeyCode::KEY_F8, true))
        );

        // Swapping the source strings without a refresh leaves evdev matching
        // on the stale cached specs.
        taurine_core::settings::set_cached_voice_ptt_hotkey("f9".to_string());
        taurine_core::settings::set_cached_voice_handsfree_hotkey("f10".to_string());
        assert!(
            cached_voice_ptt_spec().is_some_and(|spec| spec.matches_press_evdev(
                KeyCode::KEY_V,
                true,
                mods_ctrl_shift
            ))
        );
        assert!(
            cached_voice_handsfree_spec().is_some_and(|spec| spec.matches_press_evdev(
                KeyCode::KEY_F8,
                true,
                Modifiers::new()
            ))
        );

        // Restore defaults so other tests see a clean cache.
        taurine_core::settings::set_cached_voice_ptt_hotkey("win+lctrl".to_string());
        taurine_core::settings::set_cached_voice_handsfree_hotkey("win+lctrl+lalt".to_string());
        refresh_cached_voice_specs("win+lctrl", "win+lctrl+lalt");
    }
}

#[cfg(test)]
mod icon_preview_tests {
    use super::*;

    /// Each distinct press flips the hint; a pair swings back by itself, so
    /// even bursts never need a revert from the counter.
    #[test]
    fn preview_flips_per_press_and_pair_swings_back() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sync_pause_icon_preview(false);

        push_pause_icon_preview();
        assert!(
            pause_icon_preview(),
            "first press must show paused instantly"
        );
        push_pause_icon_preview();
        assert!(
            !pause_icon_preview(),
            "second press must swing the hint back before the burst settles"
        );

        sync_pause_icon_preview(false);
    }

    /// External commits (menu, RPC, snooze) reconcile the hint.
    #[test]
    fn preview_sync_follows_committed_state() {
        let _lock = taurine_core::testing::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        sync_pause_icon_preview(false);

        sync_pause_icon_preview(true);
        assert!(pause_icon_preview());
        sync_pause_icon_preview(false);
        assert!(!pause_icon_preview());
    }
}
