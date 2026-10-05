//! Terminal tab title for fullscreen sessions.
//!
//! The tab reads `Taurine` while the TUI (or a CLI overlay dialog) owns
//! the screen; the previous title comes back on exit. Every write is
//! best-effort so a hostile stdout never blocks the session.

use std::io;
use std::sync::OnceLock;

use crossterm::{execute, terminal::SetTitle};

pub(crate) const TUI_TITLE: &str = "Taurine";

/// Console title active before the first session this process opened.
/// Every teardown path restores this same value.
static PREVIOUS_TITLE: OnceLock<Option<String>> = OnceLock::new();

/// Save the current title once and switch the tab to `Taurine`.
pub(crate) fn acquire() {
    let _ = PREVIOUS_TITLE.set(read_title());
    #[cfg(unix)]
    push_title();
    set(TUI_TITLE);
}

/// Restore the pre-session title.
pub(crate) fn release() {
    #[cfg(unix)]
    pop_title();
    if let Some(previous) = PREVIOUS_TITLE.get().and_then(Option::as_ref) {
        set(previous);
    }
}

fn set(title: &str) {
    let _ = execute!(io::stdout(), SetTitle(title));
}

#[cfg(windows)]
fn read_title() -> Option<String> {
    const CAPACITY: u32 = 1024;
    let mut buffer = vec![0u16; CAPACITY as usize];
    // SAFETY: GetConsoleTitleW writes at most CAPACITY UTF-16 units into
    // our exclusively owned buffer and returns the length without the
    // trailing nul; the buffer outlives the call and nothing else
    // aliases it while the call runs.
    let length = unsafe { GetConsoleTitleW(buffer.as_mut_ptr(), CAPACITY) };
    if length == 0 {
        return None;
    }
    String::from_utf16(&buffer[..length as usize]).ok()
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetConsoleTitleW(buffer: *mut u16, size: u32) -> u32;
}

#[cfg(not(windows))]
fn read_title() -> Option<String> {
    None
}

// honey: xterm title stack; terminals without it ignore both writes,
// and most Unix prompts repaint the title on return anyway.
#[cfg(unix)]
fn push_title() {
    use std::io::Write;
    let _ = write!(io::stdout(), "\x1b[22;0t");
}

#[cfg(unix)]
fn pop_title() {
    use std::io::Write;
    let _ = write!(io::stdout(), "\x1b[23;0t");
}
