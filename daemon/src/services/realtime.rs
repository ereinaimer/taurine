use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::Message;

/// Converts an HTTP or HTTPS Supabase base URL to a WebSocket URL and appends the realtime endpoint.
pub fn build_realtime_ws_url(base_url: &str, anon_key: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let ws_base = if let Some(stripped) = trimmed.strip_prefix("https://") {
        format!("wss://{stripped}")
    } else if let Some(stripped) = trimmed.strip_prefix("http://") {
        format!("ws://{stripped}")
    } else {
        trimmed.to_string()
    };
    format!("{ws_base}/realtime/v1/websocket?apikey={anon_key}&vsn=1.0.0")
}

/// Generates the Phoenix channel join message subscribing to snippets and user_settings table changes.
pub fn phoenix_join_message() -> String {
    serde_json::json!({
        "topic": "realtime:public",
        "event": "phx_join",
        "payload": {
            "config": {
                "postgres_changes": [
                    { "event": "*", "schema": "public", "table": "snippets" },
                    { "event": "*", "schema": "public", "table": "user_settings" }
                ]
            }
        },
        "ref": "1"
    })
    .to_string()
}

/// Generates the Phoenix channel heartbeat message.
pub fn phoenix_heartbeat_message() -> String {
    serde_json::json!({
        "topic": "phoenix",
        "event": "heartbeat",
        "payload": {},
        "ref": "hb"
    })
    .to_string()
}

/// Inspects an incoming text payload to determine if it represents a remote Postgres change event.
pub fn is_postgres_change_message(text: &str) -> bool {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };

    if val.get("event").and_then(|e| e.as_str()) == Some("postgres_changes") {
        return true;
    }

    if let Some(payload) = val.get("payload") {
        if payload.get("postgres_changes").is_some() {
            return true;
        }
        if let Some(data) = payload.get("data")
            && (data.get("table").is_some() || data.get("type").is_some())
        {
            return true;
        }
    }

    false
}

/// Processes an active WebSocket connection, sending the Phoenix join message,
/// sending 25-second heartbeats, and signaling `sync_notify` whenever a Postgres change arrives.
pub async fn process_realtime_websocket<S>(
    ws_stream: tokio_tungstenite::WebSocketStream<S>,
    sync_notify: Arc<Notify>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut write, mut read) = ws_stream.split();
    let join_msg = phoenix_join_message();
    write.send(Message::Text(join_msg.into())).await?;

    let mut heartbeat_ticker = tokio::time::interval(Duration::from_secs(25));
    heartbeat_ticker.tick().await;

    loop {
        tokio::select! {
            _ = heartbeat_ticker.tick() => {
                let hb = phoenix_heartbeat_message();
                if let Err(e) = write.send(Message::Text(hb.into())).await {
                    tracing::warn!("Failed to send Phoenix heartbeat: {e}");
                    break;
                }
            }
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if is_postgres_change_message(&text) {
                            tracing::info!("Realtime remote change received; triggering immediate sync pull");
                            sync_notify.notify_one();
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::debug!("Realtime WebSocket server sent close frame");
                        break;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::warn!("Realtime WebSocket read error: {e}");
                        break;
                    }
                    None => {
                        tracing::debug!("Realtime WebSocket stream ended");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Connects to a WebSocket endpoint and processes Phoenix channel messages until disconnected.
#[allow(dead_code)]
pub async fn connect_and_listen(
    ws_url: &str,
    sync_notify: Arc<Notify>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await?;
    process_realtime_websocket(ws_stream, sync_notify).await
}

/// Background loop that monitors authentication, connects to Supabase Realtime WebSocket,
/// reconnects with progressive backoff on failure, and triggers catch-up sync pulls upon reconnection.
pub async fn realtime_listener_loop(sync_notify: Arc<Notify>) {
    let mut consecutive_failures: u32 = 0;
    let mut is_reconnect = false;

    loop {
        let is_authenticated = nite_core::cloud::get_cloud_tokens()
            .ok()
            .flatten()
            .is_some();
        if !is_authenticated {
            tracing::debug!("Realtime listener: not authenticated; waiting 5s");
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        }

        let cloud_config = nite_core::cloud::CloudConfig::default_or_from_env();
        let ws_url = build_realtime_ws_url(&cloud_config.supabase_url, &cloud_config.anon_key);

        tracing::debug!("Connecting to Supabase Realtime at {ws_url}...");
        match tokio_tungstenite::connect_async(&ws_url).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Connected to Supabase Realtime WebSocket");
                consecutive_failures = 0;

                if is_reconnect {
                    tracing::info!(
                        "Realtime reconnection established; triggering catch-up sync pull"
                    );
                    sync_notify.notify_one();
                }
                is_reconnect = true;

                if let Err(e) = process_realtime_websocket(ws_stream, sync_notify.clone()).await {
                    tracing::warn!("Realtime connection ended with error: {e}");
                } else {
                    tracing::debug!("Realtime connection closed cleanly");
                }
                tracing::warn!("Realtime connection closed; reconnecting...");
            }
            Err(e) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                let backoff_secs = match consecutive_failures {
                    1 => 3,
                    2 => 5,
                    _ => 15,
                };
                tracing::warn!("Realtime connection error ({e}); retrying in {backoff_secs}s...");
                tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            }
        }
    }
}

/// Spawns the background thread named `nite-realtime` running the realtime listener loop.
pub fn spawn_realtime_listener(sync_notify: Arc<Notify>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("nite-realtime".to_string())
        .spawn(move || {
            let _ = crate::platform::panic::catch_worker_panic("nite-realtime", move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!("Failed to create runtime for realtime listener: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    realtime_listener_loop(sync_notify).await;
                });
            });
        })
        .expect("spawn nite-realtime thread")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_realtime_ws_url() {
        let url_https = build_realtime_ws_url("https://example.supabase.co", "test-anon-key");
        assert_eq!(
            url_https,
            "wss://example.supabase.co/realtime/v1/websocket?apikey=test-anon-key&vsn=1.0.0"
        );

        let url_https_trailing =
            build_realtime_ws_url("https://example.supabase.co/", "test-anon-key");
        assert_eq!(
            url_https_trailing,
            "wss://example.supabase.co/realtime/v1/websocket?apikey=test-anon-key&vsn=1.0.0"
        );

        let url_http = build_realtime_ws_url("http://127.0.0.1:54321", "local-key");
        assert_eq!(
            url_http,
            "ws://127.0.0.1:54321/realtime/v1/websocket?apikey=local-key&vsn=1.0.0"
        );

        let url_http_trailing = build_realtime_ws_url("http://127.0.0.1:54321/", "local-key");
        assert_eq!(
            url_http_trailing,
            "ws://127.0.0.1:54321/realtime/v1/websocket?apikey=local-key&vsn=1.0.0"
        );

        let url_wss = build_realtime_ws_url("wss://custom.domain/", "key");
        assert_eq!(
            url_wss,
            "wss://custom.domain/realtime/v1/websocket?apikey=key&vsn=1.0.0"
        );
    }

    #[test]
    fn test_is_postgres_change_message() {
        let change_json = serde_json::json!({
            "topic": "realtime:public",
            "event": "postgres_changes",
            "payload": {
                "data": {
                    "type": "UPDATE",
                    "table": "snippets"
                }
            },
            "ref": null
        })
        .to_string();
        assert!(is_postgres_change_message(&change_json));

        let data_json = serde_json::json!({
            "topic": "realtime:public",
            "event": "other",
            "payload": {
                "data": {
                    "table": "user_settings"
                }
            }
        })
        .to_string();
        assert!(is_postgres_change_message(&data_json));

        let phx_reply = serde_json::json!({
            "topic": "realtime:public",
            "event": "phx_reply",
            "payload": {
                "status": "ok",
                "response": {}
            },
            "ref": "1"
        })
        .to_string();
        assert!(!is_postgres_change_message(&phx_reply));

        let invalid = "not valid json";
        assert!(!is_postgres_change_message(invalid));
    }

    #[tokio::test]
    async fn test_realtime_websocket_receives_postgres_changes_and_notifies() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock ws listener");
        let local_addr = listener.local_addr().expect("local addr");
        let ws_url = format!("ws://{local_addr}/realtime/v1/websocket?apikey=test&vsn=1.0.0");

        let server_handle = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept connection");
            let mut ws = tokio_tungstenite::accept_async(stream)
                .await
                .expect("accept ws handshake");

            // Verify join message received
            if let Some(Ok(Message::Text(msg))) = ws.next().await {
                assert!(msg.contains("phx_join"), "Expected phx_join message: {msg}");
            }

            // Send postgres_changes message to client
            let change_msg = serde_json::json!({
                "topic": "realtime:public",
                "event": "postgres_changes",
                "payload": {
                    "data": {
                        "type": "INSERT",
                        "table": "snippets",
                        "record": { "id": "test-snippet-1" }
                    }
                },
                "ref": null
            })
            .to_string();
            ws.send(Message::Text(change_msg.into()))
                .await
                .expect("send change message");

            // Wait a moment for client to receive, then close
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        let sync_notify = Arc::new(Notify::new());
        let sync_notify_clone = sync_notify.clone();

        let client_handle = tokio::spawn(async move {
            let _ = connect_and_listen(&ws_url, sync_notify_clone).await;
        });

        // Await notification triggered by postgres_changes event
        let notified = tokio::time::timeout(Duration::from_secs(3), sync_notify.notified()).await;
        assert!(
            notified.is_ok(),
            "sync_notify must be notified upon receiving postgres_changes"
        );

        let _ = server_handle.await;
        client_handle.abort();
    }
}
