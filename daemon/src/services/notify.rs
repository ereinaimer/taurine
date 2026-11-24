use notify_rust::Notification;
use tracing::debug;

#[allow(dead_code)]
pub fn notify_pause_toggled(paused: bool) {
    let (summary, body) = if paused {
        ("Nite Paused", "Triggers are currently disabled.")
    } else {
        ("Nite Resumed", "Triggers are active.")
    };

    match Notification::new()
        // .app_id("Nite")
        //.appname("Nite")
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
        .summary("Nite Did Not Stop")
        .body("Quit was requested but Nite is still running. Try nite down or check the logs.")
        .show()
    {
        Ok(_) => {}
        Err(e) => debug!("Desktop notification failed: {}", e),
    }
}
