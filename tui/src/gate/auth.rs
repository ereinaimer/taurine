//! Google OAuth for the TUI sign-in gate.
//!
//! Runs on a worker thread so the event loop keeps drawing the waiting
//! state. The listener is non-blocking so cancelling stops it promptly.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use taurine_core::db::crud::UserTier;

pub(crate) type LoginResult = Result<String, String>;

/// Starts a Google sign-in and returns the channel its result lands on.
pub(crate) fn spawn(cancel: Arc<AtomicBool>) -> Receiver<LoginResult> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run(&cancel));
    });
    rx
}

fn run(cancel: &AtomicBool) -> LoginResult {
    let (verifier, challenge) = taurine_core::cloud::generate_pkce_challenge();
    let client = taurine_core::cloud::CloudClient::default_or_from_env();
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("Could not start the sign-in listener: {error}"))?;
    let port = listener
        .local_addr()
        .map_err(|error| format!("Could not read the sign-in port: {error}"))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let auth_url = client.auth_url_for_provider("google", &redirect, &challenge);

    taurine_core::cloud::open_browser(&auth_url)
        .map_err(|error| format!("Could not open your browser: {error}"))?;

    let code = wait_for_code(&listener, cancel)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("Could not start the sign-in runtime: {error}"))?;
    let tokens = runtime
        .block_on(client.exchange_code_for_session(&code, &verifier, &redirect))
        .map_err(|error| format!("Sign-in failed: {error}"))?;
    taurine_core::cloud::store_tokens(&tokens)
        .map_err(|error| format!("Could not save your session: {error}"))?;

    let tier = match runtime.block_on(client.get_profile(&tokens.access_token, &tokens.user_id)) {
        Ok(profile) => UserTier::from_cloud_tier(&profile.tier),
        Err(error) => {
            tracing::warn!("Failed to fetch profile tier: {error}; defaulting to Free");
            UserTier::Free
        }
    };

    let conn = taurine_core::db::init::setup()
        .map_err(|error| format!("Could not open the local database: {error}"))?;
    taurine_core::db::crud::set_user_tier(&conn, tier)
        .map_err(|error| format!("Could not save your plan: {error}"))?;
    let _ = taurine_core::db::crud::triggers::backfill_untracked_triggers_to_outbox(&conn);

    if taurine_core::service::is_service_running() {
        taurine_core::system::rpc::notify_daemon_reload();
    } else {
        let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
        let _ = taurine_core::service::up(settings.start_on_boot);
    }

    Ok(taurine_core::cloud::extract_jwt_email(&tokens.access_token)
        .unwrap_or_else(|| tokens.user_id.clone()))
}

fn wait_for_code(listener: &TcpListener, cancel: &AtomicBool) -> Result<String, String> {
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("Could not prepare the sign-in listener: {error}"))?;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Sign-in cancelled.".to_string());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let mut reader = BufReader::new(&stream);
                let mut request_line = String::new();
                reader
                    .read_line(&mut request_line)
                    .map_err(|error| format!("Could not read the sign-in callback: {error}"))?;
                let code = taurine_core::cloud::extract_code_from_request_line(&request_line)
                    .ok_or_else(|| "The browser returned no authorization code.".to_string())?;
                let body = "<!DOCTYPE html><html><head><title>Taurine Login</title></head><body><h1>Authentication Successful</h1><p>You can close this tab and return to your terminal.</p></body></html>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
                return Ok(code);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) => {
                return Err(format!("Sign-in listener failed: {error}"));
            }
        }
    }
}
