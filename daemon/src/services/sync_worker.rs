use rusqlite::OptionalExtension;
use std::time::Duration;
use taurine_core::cloud::CloudClient;
use taurine_core::db::crud::triggers::{
    CloudSnippetPayload, ReconcileOutcome, purge_deleted_tombstones, reconcile_cloud_snippet,
};
use taurine_core::db::crud::{
    CloudActivityEventPayload, CloudSettingPayload, CloudTriggerDailyStatPayload,
    CloudUserAppStatPayload, CloudUserStatPayload, SettingReconcileOutcome,
    reconcile_cloud_activity_events, reconcile_cloud_app_stats, reconcile_cloud_setting,
    reconcile_cloud_trigger_daily_stats, reconcile_cloud_user_stats,
};

/// The execution outcome of a sync cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    NotAuthenticated,
    Success {
        pulled_snippets: usize,
        pushed_snippets: usize,
        pulled_settings: usize,
        pushed_settings: usize,
        pulled_events: usize,
        pushed_stats: usize,
    },
    Error(String),
}

/// Periodic background worker that pushes quota deltas, pulls updates,
/// and uploads local trigger modifications.
pub struct SyncWorker {
    interval: Duration,
}

impl SyncWorker {
    /// Creates a new `SyncWorker` with the specified execution interval.
    pub fn new(interval: Duration) -> Self {
        Self { interval }
    }

    /// Returns the configured execution interval.
    pub fn interval(&self) -> Duration {
        self.interval
    }

    /// Spawns a background thread running periodic sync cycles.
    pub fn spawn<F, E>(conn_provider: F, interval: Duration) -> std::thread::JoinHandle<()>
    where
        F: Fn() -> Result<rusqlite::Connection, E> + Send + 'static,
        E: std::fmt::Display,
    {
        std::thread::Builder::new()
            .name("tau-sync".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!("Failed to create runtime for sync worker: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    let mut ticker = tokio::time::interval(interval);
                    let client = CloudClient::default_or_from_env();
                    loop {
                        ticker.tick().await;
                        match conn_provider() {
                            Ok(conn) => {
                                let status = Self::run_sync_cycle(&conn, Some(&client)).await;
                                Self::log_sync_status(status);
                            }
                            Err(e) => {
                                tracing::warn!("Sync worker failed to acquire db connection: {e}");
                            }
                        }
                    }
                });
            })
            .expect("spawn tau-sync thread")
    }

    /// Spawns a background thread running event-driven sync cycles with micro-debouncing and cooldown.
    pub fn spawn_event_driven<F, E>(
        conn_provider: F,
        fallback_interval: Duration,
        notify: std::sync::Arc<tokio::sync::Notify>,
    ) -> std::thread::JoinHandle<()>
    where
        F: Fn() -> Result<rusqlite::Connection, E> + Send + 'static,
        E: std::fmt::Display,
    {
        std::thread::Builder::new()
            .name("tau-sync".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!("Failed to create runtime for sync worker: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    let mut fallback_ticker = tokio::time::interval(fallback_interval);
                    fallback_ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                    let client = CloudClient::default_or_from_env();
                    let mut last_sync = std::time::Instant::now() - Duration::from_secs(60);
                    const DEBOUNCE_MS: u64 = 300;
                    const COOLDOWN_SECS: u64 = 3;

                    // Initial tick on boot
                    fallback_ticker.tick().await;
                    if let Ok(conn) = conn_provider() {
                        let status = Self::run_sync_cycle(&conn, Some(&client)).await;
                        last_sync = std::time::Instant::now();
                        Self::log_sync_status(status);
                    }

                    loop {
                        tokio::select! {
                            _ = fallback_ticker.tick() => {
                                tracing::debug!("Fallback periodic sync check triggered");
                            }
                            _ = notify.notified() => {
                                tracing::debug!("Event-driven sync notification received; debouncing...");
                                tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
                            }
                        }

                        // Cooldown guard to avoid spamming cloud API
                        if last_sync.elapsed() < Duration::from_secs(COOLDOWN_SECS) {
                            tokio::time::sleep(
                                Duration::from_secs(COOLDOWN_SECS)
                                    .saturating_sub(last_sync.elapsed()),
                            )
                            .await;
                        }

                        match conn_provider() {
                            Ok(conn) => {
                                let status = Self::run_sync_cycle(&conn, Some(&client)).await;
                                last_sync = std::time::Instant::now();
                                Self::log_sync_status(status);
                            }
                            Err(e) => {
                                tracing::warn!("Sync worker failed to acquire db connection: {e}");
                            }
                        }
                    }
                });
            })
            .expect("spawn tau-sync thread")
    }

    pub fn log_sync_status(status: SyncStatus) {
        match status {
            SyncStatus::Success {
                pulled_snippets,
                pushed_snippets,
                pulled_settings,
                pushed_settings,
                pulled_events,
                pushed_stats,
            } => {
                if pulled_snippets > 0
                    || pushed_snippets > 0
                    || pulled_settings > 0
                    || pushed_settings > 0
                    || pulled_events > 0
                    || pushed_stats > 0
                {
                    tracing::info!(
                        pulled_snippets,
                        pushed_snippets,
                        pulled_settings,
                        pushed_settings,
                        pulled_events,
                        pushed_stats,
                        "Cloud sync cycle succeeded"
                    );
                } else {
                    tracing::debug!("Cloud sync cycle succeeded (no changes)");
                }

                if pulled_snippets > 0 || pulled_settings > 0 {
                    taurine_core::rpc::notify_daemon_reload_internal(true);
                }
            }
            SyncStatus::Error(err) => {
                tracing::warn!("Cloud sync cycle failed: {err}");
            }
            SyncStatus::NotAuthenticated => {
                tracing::debug!("Cloud sync skipped: user not authenticated");
            }
        }
    }

    async fn refresh_session_tokens(
        client: &CloudClient,
        tokens: &mut taurine_core::cloud::AuthTokens,
    ) -> Result<(), String> {
        let prev_user_id = tokens.user_id.clone();
        match client.refresh_session(&tokens.refresh_token).await {
            Ok(mut new_tokens) => {
                if new_tokens.user_id.is_empty() {
                    new_tokens.user_id = prev_user_id;
                }
                if let Err(e) = taurine_core::cloud::store_cloud_tokens(&new_tokens) {
                    tracing::warn!("Failed to store refreshed cloud tokens in keyring: {e}");
                }
                *tokens = new_tokens;
                tracing::debug!("Successfully refreshed cloud auth tokens");
                Ok(())
            }
            Err(e) => {
                tracing::warn!("Failed to refresh cloud auth tokens: {e}");
                Err(format!("Session refresh failed: {e}"))
            }
        }
    }

    pub(crate) fn unix_secs_to_rfc3339(ts: i64) -> String {
        let mut days = ts / 86400;
        let mut rem_secs = ts % 86400;
        if rem_secs < 0 {
            rem_secs += 86400;
            days -= 1;
        }
        let hours = rem_secs / 3600;
        let mins = (rem_secs % 3600) / 60;
        let secs = rem_secs % 60;

        let z = days + 719468;
        let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
        let doe = (z - era * 146097) as u32;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };

        format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{secs:02}Z")
    }

    /// Executes a single bidirectional sync cycle:
    /// 1. Checks user authentication via stored tokens.
    /// 2. Performs 30-day tombstone garbage collection.
    /// 3. Pushes pending quota depletion deltas.
    /// 4. Pulls remote snippet updates and reconciles via LWW.
    /// 5. Uploads pending local trigger modifications.
    pub async fn run_sync_cycle(
        conn: &rusqlite::Connection,
        client: Option<&CloudClient>,
    ) -> SyncStatus {
        // 0. Flush any pending in-memory quota deltas to SQLite (applies locally regardless of cloud auth)
        if let Ok(key) = taurine_core::db::key::get_or_create_db_key() {
            let _ = crate::engine::quota_guard::QuotaGuard::global().flush_to_db(conn, &key);
        }

        let client = match client {
            Some(c) => c,
            None => return SyncStatus::NotAuthenticated,
        };

        let mut tokens = match taurine_core::cloud::get_cloud_tokens() {
            Ok(Some(t)) => t,
            Ok(None) => return SyncStatus::NotAuthenticated,
            Err(e) => return SyncStatus::Error(format!("Keystore error: {e}")),
        };

        // Proactively refresh auth session if token is expired or expiring within 60s
        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if tokens.expires_at.is_some_and(|exp| exp <= now_epoch + 60) {
            tracing::debug!(
                "Auth token is expired or expiring within 60s; proactively refreshing..."
            );
            if let Err(e) = Self::refresh_session_tokens(client, &mut tokens).await {
                return SyncStatus::Error(e);
            }
        }

        // 30-day tombstone GC
        if let Err(e) = purge_deleted_tombstones(conn, 30 * 86400) {
            tracing::warn!("Tombstone GC error: {e}");
        }

        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        let mut pushed_snippets = 0;
        let mut pushed_settings = 0;
        let mut pushed_stats = 0;
        let mut pulled_snippets = 0;
        let mut pulled_settings = 0;

        // 1. Push Phase (Outbox Drain)
        loop {
            let batch = match taurine_core::db::crud::fetch_outbox_batch(conn, 50) {
                Ok(b) => b,
                Err(e) => return SyncStatus::Error(format!("Failed to fetch outbox batch: {e}")),
            };

            if batch.is_empty() {
                break;
            }

            let mut max_batch_id = 0i64;

            for entry in &batch {
                if entry.id > max_batch_id {
                    max_batch_id = entry.id;
                }

                match entry.entity_type.as_str() {
                    "snippet" => {
                        let push_url = format!(
                            "{}/rest/v1/snippets?on_conflict=id",
                            client.config().supabase_url.trim_end_matches('/')
                        );

                        let mut payload = entry.payload.clone();
                        if let serde_json::Value::Object(ref mut map) = payload {
                            map.remove("local_id");
                            map.remove("name");
                            if !map.contains_key("user_id") {
                                map.insert(
                                    "user_id".to_string(),
                                    serde_json::Value::String(tokens.user_id.clone()),
                                );
                            }
                        }

                        let mut push_res = http_client
                            .post(&push_url)
                            .header("apikey", &client.config().anon_key)
                            .header("Authorization", format!("Bearer {}", tokens.access_token))
                            .header("Content-Type", "application/json")
                            .header("Prefer", "resolution=merge-duplicates")
                            .json(&payload)
                            .send()
                            .await;

                        if let Ok(ref r) = push_res
                            && r.status() == reqwest::StatusCode::UNAUTHORIZED
                        {
                            tracing::debug!("Received 401 pushing snippet; refreshing session...");
                            if Self::refresh_session_tokens(client, &mut tokens)
                                .await
                                .is_ok()
                            {
                                push_res = http_client
                                    .post(&push_url)
                                    .header("apikey", &client.config().anon_key)
                                    .header(
                                        "Authorization",
                                        format!("Bearer {}", tokens.access_token),
                                    )
                                    .header("Content-Type", "application/json")
                                    .header("Prefer", "resolution=merge-duplicates")
                                    .json(&payload)
                                    .send()
                                    .await;
                            }
                        }

                        match push_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    return SyncStatus::Error(format!(
                                        "Failed to push snippet ({status}): {text}"
                                    ));
                                }
                                pushed_snippets += 1;
                            }
                            Err(e) => {
                                return SyncStatus::Error(format!(
                                    "Network error pushing snippet: {e}"
                                ));
                            }
                        }
                    }
                    "setting" => {
                        if !taurine_core::db::crud::is_setting_syncable(&entry.entity_id) {
                            continue;
                        }

                        let settings_url = format!(
                            "{}/rest/v1/user_settings?on_conflict=user_id,key",
                            client.config().supabase_url.trim_end_matches('/')
                        );

                        let mut payload = entry.payload.clone();
                        if let serde_json::Value::Object(ref mut map) = payload
                            && !map.contains_key("user_id")
                        {
                            map.insert(
                                "user_id".to_string(),
                                serde_json::Value::String(tokens.user_id.clone()),
                            );
                        }

                        let mut setting_res = http_client
                            .post(&settings_url)
                            .header("apikey", &client.config().anon_key)
                            .header("Authorization", format!("Bearer {}", tokens.access_token))
                            .header("Content-Type", "application/json")
                            .header("Prefer", "resolution=merge-duplicates")
                            .json(&payload)
                            .send()
                            .await;

                        if let Ok(ref r) = setting_res
                            && r.status() == reqwest::StatusCode::UNAUTHORIZED
                        {
                            tracing::debug!("Received 401 pushing setting; refreshing session...");
                            if Self::refresh_session_tokens(client, &mut tokens)
                                .await
                                .is_ok()
                            {
                                setting_res = http_client
                                    .post(&settings_url)
                                    .header("apikey", &client.config().anon_key)
                                    .header(
                                        "Authorization",
                                        format!("Bearer {}", tokens.access_token),
                                    )
                                    .header("Content-Type", "application/json")
                                    .header("Prefer", "resolution=merge-duplicates")
                                    .json(&payload)
                                    .send()
                                    .await;
                            }
                        }

                        match setting_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    return SyncStatus::Error(format!(
                                        "Failed to push setting ({status}): {text}"
                                    ));
                                }
                                pushed_settings += 1;
                            }
                            Err(e) => {
                                return SyncStatus::Error(format!(
                                    "Network error pushing setting: {e}"
                                ));
                            }
                        }
                    }
                    "quota" => {
                        let quota_url = format!(
                            "{}/rest/v1/rpc/record_quota_burn",
                            client.config().supabase_url.trim_end_matches('/')
                        );

                        let device_id = taurine_core::cloud::get_device_hardware_id();
                        let rpc_payload = serde_json::json!({
                            "p_week_epoch": entry.payload.get("week_epoch").and_then(|v| v.as_i64()).unwrap_or(0),
                            "p_delta": entry.payload.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            "p_device_id": device_id,
                            "p_mutation_id": entry.payload.get("mutation_id").and_then(|v| v.as_str()).unwrap_or_default(),
                        });

                        let mut quota_res = http_client
                            .post(&quota_url)
                            .header("apikey", &client.config().anon_key)
                            .header("Authorization", format!("Bearer {}", tokens.access_token))
                            .header("Content-Type", "application/json")
                            .json(&rpc_payload)
                            .send()
                            .await;

                        if let Ok(ref r) = quota_res
                            && r.status() == reqwest::StatusCode::UNAUTHORIZED
                        {
                            tracing::debug!(
                                "Received 401 posting quota burn RPC; refreshing session..."
                            );
                            if Self::refresh_session_tokens(client, &mut tokens)
                                .await
                                .is_ok()
                            {
                                quota_res = http_client
                                    .post(&quota_url)
                                    .header("apikey", &client.config().anon_key)
                                    .header(
                                        "Authorization",
                                        format!("Bearer {}", tokens.access_token),
                                    )
                                    .header("Content-Type", "application/json")
                                    .json(&rpc_payload)
                                    .send()
                                    .await;
                            }
                        }

                        match quota_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    return SyncStatus::Error(format!(
                                        "Failed to record quota burn ({status}): {text}"
                                    ));
                                }

                                if let Ok(val) = res.json::<serde_json::Value>().await {
                                    let new_balance = val
                                        .get("new_balance")
                                        .and_then(|v| v.as_f64())
                                        .or_else(|| val.as_f64());

                                    if let Some(balance) = new_balance {
                                        crate::engine::quota_guard::QuotaGuard::global()
                                            .set_balance(balance);
                                    }
                                }
                            }
                            Err(e) => {
                                return SyncStatus::Error(format!(
                                    "Network error recording quota burn: {e}"
                                ));
                            }
                        }
                    }
                    "event" | "activity_event" => {
                        let events_url = format!(
                            "{}/rest/v1/activity_events",
                            client.config().supabase_url.trim_end_matches('/')
                        );

                        let db_event = conn
                            .query_row(
                                "SELECT id, trigger_id, timestamp, date, category, app_key, word_count, time_saved_ms, content, version, created_at
                                 FROM activity_events WHERE id = ?1",
                                [&entry.entity_id],
                                |row| {
                                    let cat_str: String = row.get(4)?;
                                    let word_count: i64 = row.get(6)?;
                                    let time_saved_ms: i64 = row.get(7)?;
                                    let version: i64 = row.get(9)?;
                                    Ok(CloudActivityEventPayload {
                                        user_id: Some(tokens.user_id.clone()),
                                        device_id: taurine_core::cloud::get_device_hardware_id(),
                                        id: row.get(0)?,
                                        trigger_id: row.get(1)?,
                                        timestamp: row.get(2)?,
                                        date: row.get(3)?,
                                        category: cat_str,
                                        app_key: row.get(5)?,
                                        word_count: word_count as u64,
                                        time_saved_ms: time_saved_ms as u64,
                                        content: row.get(8)?,
                                        version: version as u64,
                                    })
                                },
                            )
                            .optional()
                            .unwrap_or(None);

                        let payload = if let Some(p) = db_event {
                            serde_json::to_value(&p).unwrap_or_else(|_| entry.payload.clone())
                        } else {
                            let mut p = entry.payload.clone();
                            if let serde_json::Value::Object(ref mut map) = p {
                                if !map.contains_key("user_id") {
                                    map.insert(
                                        "user_id".to_string(),
                                        serde_json::Value::String(tokens.user_id.clone()),
                                    );
                                }
                                if !map.contains_key("device_id") {
                                    map.insert(
                                        "device_id".to_string(),
                                        serde_json::Value::String(
                                            taurine_core::cloud::get_device_hardware_id(),
                                        ),
                                    );
                                }
                            }
                            p
                        };

                        let mut events_res = http_client
                            .post(&events_url)
                            .header("apikey", &client.config().anon_key)
                            .header("Authorization", format!("Bearer {}", tokens.access_token))
                            .header("Content-Type", "application/json")
                            .header("Prefer", "resolution=merge-duplicates")
                            .json(&payload)
                            .send()
                            .await;

                        if let Ok(ref r) = events_res
                            && r.status() == reqwest::StatusCode::UNAUTHORIZED
                        {
                            tracing::debug!(
                                "Received 401 pushing activity event; refreshing session..."
                            );
                            if Self::refresh_session_tokens(client, &mut tokens)
                                .await
                                .is_ok()
                            {
                                events_res = http_client
                                    .post(&events_url)
                                    .header("apikey", &client.config().anon_key)
                                    .header(
                                        "Authorization",
                                        format!("Bearer {}", tokens.access_token),
                                    )
                                    .header("Content-Type", "application/json")
                                    .header("Prefer", "resolution=merge-duplicates")
                                    .json(&payload)
                                    .send()
                                    .await;
                            }
                        }

                        match events_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    return SyncStatus::Error(format!(
                                        "Failed to push activity event ({status}): {text}"
                                    ));
                                }
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                return SyncStatus::Error(format!(
                                    "Network error pushing activity event: {e}"
                                ));
                            }
                        }
                    }
                    "trigger_daily_stat" => {
                        let stats_url = format!(
                            "{}/rest/v1/trigger_daily_stats",
                            client.config().supabase_url.trim_end_matches('/')
                        );

                        let parts: Vec<&str> = entry.entity_id.splitn(3, ':').collect();
                        let db_stat = if parts.len() == 3 {
                            conn.query_row(
                                "SELECT trigger_id, date, app_key, executions, words_spoken, words_expanded, time_saved_ms, version
                                 FROM trigger_daily_stats WHERE trigger_id = ?1 AND date = ?2 AND app_key = ?3",
                                rusqlite::params![parts[0], parts[1], parts[2]],
                                |row| {
                                    let executions: i64 = row.get(3)?;
                                    let words_spoken: i64 = row.get(4)?;
                                    let words_expanded: i64 = row.get(5)?;
                                    let time_saved_ms: i64 = row.get(6)?;
                                    let version: i64 = row.get(7)?;
                                    Ok(CloudTriggerDailyStatPayload {
                                        user_id: Some(tokens.user_id.clone()),
                                        device_id: taurine_core::cloud::get_device_hardware_id(),
                                        trigger_id: row.get(0)?,
                                        date: row.get(1)?,
                                        app_key: row.get(2)?,
                                        executions: executions as u64,
                                        words_spoken: words_spoken as u64,
                                        words_expanded: words_expanded as u64,
                                        time_saved_ms: time_saved_ms as u64,
                                        version: version as u64,
                                    })
                                },
                            )
                            .optional()
                            .unwrap_or(None)
                        } else {
                            None
                        };

                        let payload = if let Some(p) = db_stat {
                            serde_json::to_value(&p).unwrap_or_else(|_| entry.payload.clone())
                        } else {
                            let mut p = entry.payload.clone();
                            if let serde_json::Value::Object(ref mut map) = p {
                                if !map.contains_key("user_id") {
                                    map.insert(
                                        "user_id".to_string(),
                                        serde_json::Value::String(tokens.user_id.clone()),
                                    );
                                }
                                if !map.contains_key("device_id") {
                                    map.insert(
                                        "device_id".to_string(),
                                        serde_json::Value::String(
                                            taurine_core::cloud::get_device_hardware_id(),
                                        ),
                                    );
                                }
                            }
                            p
                        };

                        let mut stats_res = http_client
                            .post(&stats_url)
                            .header("apikey", &client.config().anon_key)
                            .header("Authorization", format!("Bearer {}", tokens.access_token))
                            .header("Content-Type", "application/json")
                            .header("Prefer", "resolution=merge-duplicates")
                            .json(&payload)
                            .send()
                            .await;

                        if let Ok(ref r) = stats_res
                            && r.status() == reqwest::StatusCode::UNAUTHORIZED
                        {
                            tracing::debug!(
                                "Received 401 pushing trigger daily stat; refreshing session..."
                            );
                            if Self::refresh_session_tokens(client, &mut tokens)
                                .await
                                .is_ok()
                            {
                                stats_res = http_client
                                    .post(&stats_url)
                                    .header("apikey", &client.config().anon_key)
                                    .header(
                                        "Authorization",
                                        format!("Bearer {}", tokens.access_token),
                                    )
                                    .header("Content-Type", "application/json")
                                    .header("Prefer", "resolution=merge-duplicates")
                                    .json(&payload)
                                    .send()
                                    .await;
                            }
                        }

                        match stats_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    return SyncStatus::Error(format!(
                                        "Failed to push trigger daily stat ({status}): {text}"
                                    ));
                                }
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                return SyncStatus::Error(format!(
                                    "Network error pushing trigger daily stat: {e}"
                                ));
                            }
                        }
                    }
                    "stat" | "user_stat" => {
                        let user_stats_url = format!(
                            "{}/rest/v1/user_stats",
                            client.config().supabase_url.trim_end_matches('/')
                        );

                        let db_stat = conn
                            .query_row(
                                "SELECT date, executions, ai_executions, voice_executions, words_spoken, words_expanded, time_saved_ms, version
                                 FROM stats WHERE date = ?1",
                                [&entry.entity_id],
                                |row| {
                                    Ok(CloudUserStatPayload {
                                        user_id: Some(tokens.user_id.clone()),
                                        device_id: taurine_core::cloud::get_device_hardware_id(),
                                        date: row.get(0)?,
                                        executions: row.get(1)?,
                                        ai_executions: row.get(2)?,
                                        voice_executions: row.get(3)?,
                                        words_spoken: row.get(4)?,
                                        words_expanded: row.get(5)?,
                                        time_saved_ms: row.get(6)?,
                                        version: row.get(7)?,
                                    })
                                },
                            )
                            .optional()
                            .unwrap_or(None);

                        let payload = if let Some(p) = db_stat {
                            serde_json::to_value(&p).unwrap_or_else(|_| entry.payload.clone())
                        } else {
                            let mut p = entry.payload.clone();
                            if let serde_json::Value::Object(ref mut map) = p {
                                if !map.contains_key("user_id") {
                                    map.insert(
                                        "user_id".to_string(),
                                        serde_json::Value::String(tokens.user_id.clone()),
                                    );
                                }
                                if !map.contains_key("device_id") {
                                    map.insert(
                                        "device_id".to_string(),
                                        serde_json::Value::String(
                                            taurine_core::cloud::get_device_hardware_id(),
                                        ),
                                    );
                                }
                            }
                            p
                        };

                        let mut user_stats_res = http_client
                            .post(&user_stats_url)
                            .header("apikey", &client.config().anon_key)
                            .header("Authorization", format!("Bearer {}", tokens.access_token))
                            .header("Content-Type", "application/json")
                            .header("Prefer", "resolution=merge-duplicates")
                            .json(&payload)
                            .send()
                            .await;

                        if let Ok(ref r) = user_stats_res
                            && r.status() == reqwest::StatusCode::UNAUTHORIZED
                        {
                            tracing::debug!(
                                "Received 401 pushing user stats; refreshing session..."
                            );
                            if Self::refresh_session_tokens(client, &mut tokens)
                                .await
                                .is_ok()
                            {
                                user_stats_res = http_client
                                    .post(&user_stats_url)
                                    .header("apikey", &client.config().anon_key)
                                    .header(
                                        "Authorization",
                                        format!("Bearer {}", tokens.access_token),
                                    )
                                    .header("Content-Type", "application/json")
                                    .header("Prefer", "resolution=merge-duplicates")
                                    .json(&payload)
                                    .send()
                                    .await;
                            }
                        }

                        match user_stats_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    return SyncStatus::Error(format!(
                                        "Failed to push user stats ({status}): {text}"
                                    ));
                                }
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                return SyncStatus::Error(format!(
                                    "Network error pushing user stats: {e}"
                                ));
                            }
                        }
                    }
                    "app_stat" | "user_app_stat" => {
                        let user_app_stats_url = format!(
                            "{}/rest/v1/user_app_stats",
                            client.config().supabase_url.trim_end_matches('/')
                        );

                        let parts: Vec<&str> = entry.entity_id.splitn(2, ':').collect();
                        let db_app_stat = if parts.len() == 2 {
                            conn.query_row(
                                "SELECT app_key, date, executions, words_spoken, words_expanded, time_saved_ms, version
                                 FROM app_stats WHERE app_key = ?1 AND date = ?2",
                                rusqlite::params![parts[0], parts[1]],
                                |row| {
                                    let executions: i64 = row.get(2)?;
                                    let words_spoken: i64 = row.get(3)?;
                                    let words_expanded: i64 = row.get(4)?;
                                    let time_saved_ms: i64 = row.get(5)?;
                                    let version: i64 = row.get(6)?;
                                    Ok(CloudUserAppStatPayload {
                                        user_id: Some(tokens.user_id.clone()),
                                        device_id: taurine_core::cloud::get_device_hardware_id(),
                                        app_key: row.get(0)?,
                                        date: row.get(1)?,
                                        executions: executions as u64,
                                        words_spoken: words_spoken as u64,
                                        words_expanded: words_expanded as u64,
                                        time_saved_ms: time_saved_ms as u64,
                                        version: version as u64,
                                    })
                                },
                            )
                            .optional()
                            .unwrap_or(None)
                        } else {
                            None
                        };

                        let payload = if let Some(p) = db_app_stat {
                            serde_json::to_value(&p).unwrap_or_else(|_| entry.payload.clone())
                        } else {
                            let mut p = entry.payload.clone();
                            if let serde_json::Value::Object(ref mut map) = p {
                                if !map.contains_key("user_id") {
                                    map.insert(
                                        "user_id".to_string(),
                                        serde_json::Value::String(tokens.user_id.clone()),
                                    );
                                }
                                if !map.contains_key("device_id") {
                                    map.insert(
                                        "device_id".to_string(),
                                        serde_json::Value::String(
                                            taurine_core::cloud::get_device_hardware_id(),
                                        ),
                                    );
                                }
                            }
                            p
                        };

                        let mut app_stats_res = http_client
                            .post(&user_app_stats_url)
                            .header("apikey", &client.config().anon_key)
                            .header("Authorization", format!("Bearer {}", tokens.access_token))
                            .header("Content-Type", "application/json")
                            .header("Prefer", "resolution=merge-duplicates")
                            .json(&payload)
                            .send()
                            .await;

                        if let Ok(ref r) = app_stats_res
                            && r.status() == reqwest::StatusCode::UNAUTHORIZED
                        {
                            tracing::debug!(
                                "Received 401 pushing app stats; refreshing session..."
                            );
                            if Self::refresh_session_tokens(client, &mut tokens)
                                .await
                                .is_ok()
                            {
                                app_stats_res = http_client
                                    .post(&user_app_stats_url)
                                    .header("apikey", &client.config().anon_key)
                                    .header(
                                        "Authorization",
                                        format!("Bearer {}", tokens.access_token),
                                    )
                                    .header("Content-Type", "application/json")
                                    .header("Prefer", "resolution=merge-duplicates")
                                    .json(&payload)
                                    .send()
                                    .await;
                            }
                        }

                        match app_stats_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    return SyncStatus::Error(format!(
                                        "Failed to push app stats ({status}): {text}"
                                    ));
                                }
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                return SyncStatus::Error(format!(
                                    "Network error pushing app stats: {e}"
                                ));
                            }
                        }
                    }
                    unknown => {
                        tracing::warn!("Unknown outbox entity_type: {unknown}");
                    }
                }
            }

            if let Err(e) = taurine_core::db::crud::delete_outbox_batch(conn, max_batch_id) {
                return SyncStatus::Error(format!(
                    "Failed to delete outbox batch up to id {max_batch_id}: {e}"
                ));
            }
        }

        // 2. Pull Phase (Checkpoint Watermark)
        let last_cp = taurine_core::db::crud::get_sync_checkpoint(conn, "snippets").unwrap_or(0);
        let rfc3339_cp = Self::unix_secs_to_rfc3339(last_cp);
        let snippets_url = format!(
            "{}/rest/v1/snippets?updated_at=gt.{}&order=updated_at.asc",
            client.config().supabase_url.trim_end_matches('/'),
            rfc3339_cp
        );

        let mut pull_res = http_client
            .get(&snippets_url)
            .header("apikey", &client.config().anon_key)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(ref r) = pull_res
            && r.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            tracing::debug!("Received 401 pulling snippets; refreshing session...");
            if Self::refresh_session_tokens(client, &mut tokens)
                .await
                .is_ok()
            {
                pull_res = http_client
                    .get(&snippets_url)
                    .header("apikey", &client.config().anon_key)
                    .header("Authorization", format!("Bearer {}", tokens.access_token))
                    .header("Accept", "application/json")
                    .send()
                    .await;
            }
        }

        match pull_res {
            Ok(res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    return SyncStatus::Error(format!(
                        "Failed to pull snippets ({status}): {text}"
                    ));
                }
                match res.json::<Vec<CloudSnippetPayload>>().await {
                    Ok(remote_snippets) => {
                        let mut max_ts = last_cp;
                        for snippet in &remote_snippets {
                            if snippet.updated_at > max_ts {
                                max_ts = snippet.updated_at;
                            }
                            match reconcile_cloud_snippet(conn, snippet) {
                                Ok(outcome) => {
                                    if matches!(
                                        outcome,
                                        ReconcileOutcome::InsertedRemote
                                            | ReconcileOutcome::UpdatedFromRemote
                                    ) {
                                        pulled_snippets += 1;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to reconcile snippet {}: {e}",
                                        snippet.id
                                    );
                                }
                            }
                        }
                        if max_ts > last_cp
                            && let Err(e) = taurine_core::db::crud::set_sync_checkpoint(
                                conn, "snippets", max_ts,
                            )
                        {
                            tracing::warn!("Failed to set snippets sync checkpoint: {e}");
                        }
                    }
                    Err(e) => {
                        return SyncStatus::Error(format!(
                            "Failed to deserialize remote snippets: {e}"
                        ));
                    }
                }
            }
            Err(e) => {
                return SyncStatus::Error(format!("Network error pulling snippets: {e}"));
            }
        }

        // Pull remote settings
        let settings_url = format!(
            "{}/rest/v1/user_settings?order=version.asc,updated_at.asc",
            client.config().supabase_url.trim_end_matches('/')
        );

        let mut settings_pull_res = http_client
            .get(&settings_url)
            .header("apikey", &client.config().anon_key)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(ref r) = settings_pull_res
            && r.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            tracing::debug!("Received 401 pulling settings; refreshing session...");
            if Self::refresh_session_tokens(client, &mut tokens)
                .await
                .is_ok()
            {
                settings_pull_res = http_client
                    .get(&settings_url)
                    .header("apikey", &client.config().anon_key)
                    .header("Authorization", format!("Bearer {}", tokens.access_token))
                    .header("Accept", "application/json")
                    .send()
                    .await;
            }
        }

        match settings_pull_res {
            Ok(res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    return SyncStatus::Error(format!(
                        "Failed to pull settings ({status}): {text}"
                    ));
                }
                match res.json::<Vec<CloudSettingPayload>>().await {
                    Ok(remote_settings) => {
                        for setting in &remote_settings {
                            match reconcile_cloud_setting(conn, setting) {
                                Ok(outcome) => {
                                    if matches!(outcome, SettingReconcileOutcome::UpdatedFromRemote)
                                    {
                                        pulled_settings += 1;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        "Failed to reconcile setting {}: {e}",
                                        setting.key
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        return SyncStatus::Error(format!(
                            "Failed to deserialize remote settings: {e}"
                        ));
                    }
                }
            }
            Err(e) => {
                return SyncStatus::Error(format!("Network error pulling settings: {e}"));
            }
        }

        // Pull remote quota ledger for the current week
        let current_week = taurine_core::db::crud::quota::calculate_week_start_epoch(now_epoch);
        let quota_pull_url = format!(
            "{}/rest/v1/quota_ledger?week_start_epoch=eq.{}&limit=1",
            client.config().supabase_url.trim_end_matches('/'),
            current_week
        );

        let mut quota_pull_res = http_client
            .get(&quota_pull_url)
            .header("apikey", &client.config().anon_key)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(ref r) = quota_pull_res
            && r.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            tracing::debug!("Received 401 pulling quota ledger; refreshing session...");
            if Self::refresh_session_tokens(client, &mut tokens)
                .await
                .is_ok()
            {
                quota_pull_res = http_client
                    .get(&quota_pull_url)
                    .header("apikey", &client.config().anon_key)
                    .header("Authorization", format!("Bearer {}", tokens.access_token))
                    .header("Accept", "application/json")
                    .send()
                    .await;
            }
        }

        if let Ok(res) = quota_pull_res
            && res.status().is_success()
        {
            #[derive(serde::Deserialize)]
            struct CloudQuotaRow {
                week_start_epoch: i64,
                remaining_percentage: f64,
                #[allow(dead_code)]
                last_expansion_at: Option<i64>,
                updated_at: i64,
            }

            if let Ok(rows) = res.json::<Vec<CloudQuotaRow>>().await
                && let Some(remote) = rows.first()
            {
                let local_quota: Option<(i64, f64, Option<i64>, i64)> = conn
                    .query_row(
                        "SELECT week_start_epoch, remaining_percentage, last_expansion_at, updated_at
                         FROM quota_ledger WHERE week_start_epoch = ?1",
                        [current_week],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                            ))
                        },
                    )
                    .optional()
                    .unwrap_or(None);

                let db_key = taurine_core::db::key::get_or_create_db_key();
                if let Ok(key) = db_key {
                    match local_quota {
                        None => {
                            if let Ok(row) =
                                taurine_core::db::crud::quota::set_quota_balance_from_cloud(
                                    conn,
                                    &key,
                                    remote.week_start_epoch,
                                    remote.remaining_percentage,
                                    remote.updated_at,
                                )
                            {
                                crate::engine::quota_guard::QuotaGuard::global()
                                    .set_balance(row.remaining_percentage);
                            }
                        }
                        Some((_, local_pct, _local_last_exp, local_updated_at)) => {
                            if (remote.updated_at > local_updated_at
                                || remote.remaining_percentage < local_pct)
                                && let Ok(row) =
                                    taurine_core::db::crud::quota::set_quota_balance_from_cloud(
                                        conn,
                                        &key,
                                        remote.week_start_epoch,
                                        remote.remaining_percentage,
                                        remote.updated_at,
                                    )
                            {
                                crate::engine::quota_guard::QuotaGuard::global()
                                    .set_balance(row.remaining_percentage);
                            }
                        }
                    }
                }
            }
        }

        // Pull remote activity events (last 30 days)
        let thirty_days_ago_secs = now_epoch.saturating_sub(30 * 86400);
        let thirty_days_ago_date =
            Self::unix_secs_to_rfc3339(thirty_days_ago_secs)[..10].to_string();

        let events_pull_url = format!(
            "{}/rest/v1/activity_events?date=gte.{}&order=timestamp.asc",
            client.config().supabase_url.trim_end_matches('/'),
            thirty_days_ago_date
        );

        let mut events_pull_res = http_client
            .get(&events_pull_url)
            .header("apikey", &client.config().anon_key)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(ref r) = events_pull_res
            && r.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            tracing::debug!("Received 401 pulling activity events; refreshing session...");
            if Self::refresh_session_tokens(client, &mut tokens)
                .await
                .is_ok()
            {
                events_pull_res = http_client
                    .get(&events_pull_url)
                    .header("apikey", &client.config().anon_key)
                    .header("Authorization", format!("Bearer {}", tokens.access_token))
                    .header("Accept", "application/json")
                    .send()
                    .await;
            }
        }

        let mut pulled_events = 0;
        match events_pull_res {
            Ok(res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    return SyncStatus::Error(format!(
                        "Failed to pull activity events ({status}): {text}"
                    ));
                }
                match res.json::<Vec<CloudActivityEventPayload>>().await {
                    Ok(remote_events) => {
                        match reconcile_cloud_activity_events(conn, &remote_events) {
                            Ok(count) => {
                                pulled_events += count;
                            }
                            Err(e) => {
                                tracing::warn!("Failed to reconcile activity events: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        return SyncStatus::Error(format!(
                            "Failed to deserialize remote activity events: {e}"
                        ));
                    }
                }
            }
            Err(e) => {
                return SyncStatus::Error(format!("Network error pulling activity events: {e}"));
            }
        }

        // Pull remote trigger daily stats (last 30 days)
        let trigger_stats_pull_url = format!(
            "{}/rest/v1/trigger_daily_stats?date=gte.{}&order=date.asc",
            client.config().supabase_url.trim_end_matches('/'),
            thirty_days_ago_date
        );

        let mut trigger_stats_pull_res = http_client
            .get(&trigger_stats_pull_url)
            .header("apikey", &client.config().anon_key)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(ref r) = trigger_stats_pull_res
            && r.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            tracing::debug!("Received 401 pulling trigger daily stats; refreshing session...");
            if Self::refresh_session_tokens(client, &mut tokens)
                .await
                .is_ok()
            {
                trigger_stats_pull_res = http_client
                    .get(&trigger_stats_pull_url)
                    .header("apikey", &client.config().anon_key)
                    .header("Authorization", format!("Bearer {}", tokens.access_token))
                    .header("Accept", "application/json")
                    .send()
                    .await;
            }
        }

        match trigger_stats_pull_res {
            Ok(res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    return SyncStatus::Error(format!(
                        "Failed to pull trigger daily stats ({status}): {text}"
                    ));
                }
                match res.json::<Vec<CloudTriggerDailyStatPayload>>().await {
                    Ok(remote_stats) => {
                        if let Err(e) = reconcile_cloud_trigger_daily_stats(conn, &remote_stats) {
                            tracing::warn!("Failed to reconcile trigger daily stats: {e}");
                        }
                    }
                    Err(e) => {
                        return SyncStatus::Error(format!(
                            "Failed to deserialize remote trigger daily stats: {e}"
                        ));
                    }
                }
            }
            Err(e) => {
                return SyncStatus::Error(format!(
                    "Network error pulling trigger daily stats: {e}"
                ));
            }
        }

        // Pull remote global user stats (last 30 days)
        let user_stats_pull_url = format!(
            "{}/rest/v1/user_stats?date=gte.{}&order=date.asc",
            client.config().supabase_url.trim_end_matches('/'),
            thirty_days_ago_date
        );

        let mut user_stats_pull_res = http_client
            .get(&user_stats_pull_url)
            .header("apikey", &client.config().anon_key)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(ref r) = user_stats_pull_res
            && r.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            tracing::debug!("Received 401 pulling user stats; refreshing session...");
            if Self::refresh_session_tokens(client, &mut tokens)
                .await
                .is_ok()
            {
                user_stats_pull_res = http_client
                    .get(&user_stats_pull_url)
                    .header("apikey", &client.config().anon_key)
                    .header("Authorization", format!("Bearer {}", tokens.access_token))
                    .header("Accept", "application/json")
                    .send()
                    .await;
            }
        }

        match user_stats_pull_res {
            Ok(res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    return SyncStatus::Error(format!(
                        "Failed to pull user stats ({status}): {text}"
                    ));
                }
                match res.json::<Vec<CloudUserStatPayload>>().await {
                    Ok(remote_stats) => {
                        if let Err(e) = reconcile_cloud_user_stats(conn, &remote_stats) {
                            tracing::warn!("Failed to reconcile user stats: {e}");
                        }
                    }
                    Err(e) => {
                        return SyncStatus::Error(format!(
                            "Failed to deserialize remote user stats: {e}"
                        ));
                    }
                }
            }
            Err(e) => {
                return SyncStatus::Error(format!("Network error pulling user stats: {e}"));
            }
        }

        // Pull remote user app stats (last 30 days)
        let app_stats_pull_url = format!(
            "{}/rest/v1/user_app_stats?date=gte.{}&order=date.asc",
            client.config().supabase_url.trim_end_matches('/'),
            thirty_days_ago_date
        );

        let mut app_stats_pull_res = http_client
            .get(&app_stats_pull_url)
            .header("apikey", &client.config().anon_key)
            .header("Authorization", format!("Bearer {}", tokens.access_token))
            .header("Accept", "application/json")
            .send()
            .await;

        if let Ok(ref r) = app_stats_pull_res
            && r.status() == reqwest::StatusCode::UNAUTHORIZED
        {
            tracing::debug!("Received 401 pulling app stats; refreshing session...");
            if Self::refresh_session_tokens(client, &mut tokens)
                .await
                .is_ok()
            {
                app_stats_pull_res = http_client
                    .get(&app_stats_pull_url)
                    .header("apikey", &client.config().anon_key)
                    .header("Authorization", format!("Bearer {}", tokens.access_token))
                    .header("Accept", "application/json")
                    .send()
                    .await;
            }
        }

        match app_stats_pull_res {
            Ok(res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    return SyncStatus::Error(format!(
                        "Failed to pull app stats ({status}): {text}"
                    ));
                }
                match res.json::<Vec<CloudUserAppStatPayload>>().await {
                    Ok(remote_stats) => {
                        if let Err(e) = reconcile_cloud_app_stats(conn, &remote_stats) {
                            tracing::warn!("Failed to reconcile app stats: {e}");
                        }
                    }
                    Err(e) => {
                        return SyncStatus::Error(format!(
                            "Failed to deserialize remote app stats: {e}"
                        ));
                    }
                }
            }
            Err(e) => {
                return SyncStatus::Error(format!("Network error pulling app stats: {e}"));
            }
        }

        SyncStatus::Success {
            pulled_snippets,
            pushed_snippets,
            pulled_settings,
            pushed_settings,
            pulled_events,
            pushed_stats,
        }
    }
}
