use notify_rust::Notification;
use tracing::debug;

pub fn notify_pause_toggled(paused: bool) {
    let (summary, body) = if paused {
        ("Taurine Paused", "Triggers are currently disabled.")
    } else {
        ("Taurine Resumed", "Triggers are active.")
    };

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

#[cfg(target_os = "linux")]
pub fn notify_shutdown_failed() {
    match Notification::new()
        .summary("Taurine Did Not Stop")
        .body(
            "Quit was requested but Taurine is still running. Try taurine down or check the logs.",
        )
        .show()
    {
        Ok(_) => {}
        Err(e) => debug!("Desktop notification failed: {}", e),
    }
}
