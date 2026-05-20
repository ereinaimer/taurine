use notify_rust::Notification;
use tracing::debug;

pub fn notify_pause_toggled(paused: bool) {
    let (summary, body) = if paused {
        ("Taurine Paused", "Automations are currently disabled.")
    } else {
        ("Taurine Resumed", "Automations are active.")
    };

    // Best-effort: notifications must never crash the hook thread.
    match Notification::new()
        // .app_id("Taurine")
        //.appname("Taurine")
        .summary(summary)
        .body(body)
        .show()
    {
        Ok(_) => {}
        Err(e) => debug!("Desktop notification failed: {}", e),
    }
}
