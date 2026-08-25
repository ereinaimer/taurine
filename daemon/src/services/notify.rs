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

pub fn notify_dictionary_download_started(mode: &str) {
    match Notification::new()
        .summary("Taurine Dictionary Downloading")
        .body(&format!(
            "Downloading offline {} dictionary in the background...",
            mode
        ))
        .show()
    {
        Ok(_) => {}
        Err(e) => debug!("Desktop notification failed: {}", e),
    }
}

pub fn notify_dictionary_installed(mode: &str) {
    match Notification::new()
        .summary("Taurine Dictionary Updated")
        .body(&format!("Offline {} dictionary is ready to use.", mode))
        .show()
    {
        Ok(_) => {}
        Err(e) => debug!("Desktop notification failed: {}", e),
    }
}
