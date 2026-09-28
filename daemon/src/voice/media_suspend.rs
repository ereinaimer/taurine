//! Best-effort pause/resume of SMTC media while dictating.
//!
//! Snapshot-at-start only: suspend records the sessions it paused; resume
//! replays Play only to those still Paused. Never errors to the caller.

/// Identity of one session this guard paused. `source_id` is the SMTC
/// SourceAppUserModelId, stable across suspend/resume for the same app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PausedSession {
    pub source_id: String,
    pub display_name: String,
}

pub trait MediaSuspender: Send + Sync {
    /// Pause every currently-Playing session; return only the ones paused.
    fn suspend_playing(&self) -> Vec<PausedSession>;
    /// Resume exactly the given sessions that are still Paused; skip
    /// user-resumed (now Playing) and vanished sessions silently.
    fn resume(&self, sessions: &[PausedSession]);
}

/// Non-Windows and hermetic-test backend: media control is Windows-SMTC
/// only (MPRIS/NowPlaying are explicit non-goals); suspend is a silent no-op.
pub struct NoopMediaSuspender;

impl MediaSuspender for NoopMediaSuspender {
    fn suspend_playing(&self) -> Vec<PausedSession> {
        Vec::new()
    }
    fn resume(&self, _sessions: &[PausedSession]) {}
}

#[cfg(test)]
pub struct FakeMediaSuspender {
    pub paused: std::sync::Mutex<Vec<PausedSession>>,
    pub resumes: std::sync::atomic::AtomicUsize,
}

#[cfg(test)]
impl FakeMediaSuspender {
    pub fn new() -> Self {
        Self {
            paused: std::sync::Mutex::new(Vec::new()),
            resumes: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[cfg(test)]
impl Default for FakeMediaSuspender {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MediaSuspender for FakeMediaSuspender {
    fn suspend_playing(&self) -> Vec<PausedSession> {
        vec![PausedSession {
            source_id: "fake-player".to_string(),
            display_name: "fake".to_string(),
        }]
    }
    fn resume(&self, sessions: &[PausedSession]) {
        self.resumes
            .fetch_add(sessions.len(), std::sync::atomic::Ordering::Relaxed);
    }
}

/// Windows SMTC backend. All WinRT calls run on the caller's thread, which
/// is always the dedicated `tau-media-suspend` background thread (never the
/// hotkey path). Total budget ~2s; every failure degrades to debug-silence.
#[cfg(all(windows, not(test)))]
pub struct SmtcMediaSuspender;

#[cfg(all(windows, not(test)))]
impl MediaSuspender for SmtcMediaSuspender {
    fn suspend_playing(&self) -> Vec<PausedSession> {
        suspend_playing_smtc()
    }
    fn resume(&self, sessions: &[PausedSession]) {
        resume_paused_smtc(sessions);
    }
}

#[cfg(all(windows, not(test)))]
fn com_mta_guard() -> Result<(), String> {
    use windows_sys::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};
    const S_OK: i32 = 0;
    const S_FALSE: i32 = 1;
    const RPC_E_CHANGED_MODE: i32 = 0x80010106u32 as i32;
    // SAFETY: CoInitializeEx with null reserved and COINIT_MULTITHREADED on a
    // background thread that owns no COM state; S_OK/S_FALSE mean this call
    // initialized MTA, RPC_E_CHANGED_MODE means the thread is already
    // initialized (STA or MTA) and still usable for SMTC calls below.
    let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
    if hr == S_OK || hr == S_FALSE || hr == RPC_E_CHANGED_MODE {
        Ok(())
    } else {
        Err(format!("CoInitializeEx failed: {hr:#X}"))
    }
}

#[cfg(all(windows, not(test)))]
fn suspend_playing_smtc() -> Vec<PausedSession> {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    if com_mta_guard().is_err() {
        return Vec::new();
    }
    let manager = match Manager::RequestAsync().and_then(|op| op.get()) {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!("media suspend: SMTC manager unavailable: {e}");
            return Vec::new();
        }
    };
    let sessions = match manager.GetSessions() {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("media suspend: session list unavailable: {e}");
            return Vec::new();
        }
    };
    let mut paused = Vec::new();
    for session in sessions {
        let status = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus());
        if status != Ok(Status::Playing) {
            continue;
        }
        let source_id = session
            .SourceAppUserModelId()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if session.TryPauseAsync().and_then(|op| op.get()).is_ok() {
            paused.push(PausedSession {
                source_id,
                display_name: String::new(),
            });
        }
    }
    paused
}

#[cfg(all(windows, not(test)))]
fn resume_paused_smtc(sessions: &[PausedSession]) {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    if sessions.is_empty() || com_mta_guard().is_err() {
        return;
    }
    let manager = match Manager::RequestAsync().and_then(|op| op.get()) {
        Ok(m) => m,
        Err(e) => {
            tracing::debug!("media resume: SMTC manager unavailable: {e}");
            return;
        }
    };
    let live = match manager.GetSessions() {
        Ok(s) => s,
        Err(_) => return,
    };
    for want in sessions {
        for session in &live {
            let id = session
                .SourceAppUserModelId()
                .map(|s| s.to_string())
                .unwrap_or_default();
            if id != want.source_id {
                continue;
            }
            let status = session
                .GetPlaybackInfo()
                .and_then(|info| info.PlaybackStatus());
            // Skip user-resumed (Playing) and vanished/closed sessions.
            if status == Ok(Status::Paused)
                && session.TryPlayAsync().and_then(|op| op.get()).is_err()
            {
                tracing::debug!("media resume: TryPlay failed for {}", want.source_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn noop_suspender_round_trips_empty() {
        let m = NoopMediaSuspender;
        let paused = m.suspend_playing();
        assert!(paused.is_empty());
        m.resume(&paused);
    }
}
