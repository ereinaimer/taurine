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

pub(crate) fn is_fatal_client_error(status: reqwest::StatusCode) -> bool {
    status.is_client_error()
        && status != reqwest::StatusCode::UNAUTHORIZED
        && status != reqwest::StatusCode::TOO_MANY_REQUESTS
        && status != reqwest::StatusCode::REQUEST_TIMEOUT
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

    fn handle_outbox_failure(
        conn: &rusqlite::Connection,
        entry: &taurine_core::db::crud::OutboxEntry,
        status: reqwest::StatusCode,
        text: &str,
    ) -> Option<SyncStatus> {
        let error_msg = format!("{status}: {text}");
        if is_fatal_client_error(status) || entry.retry_count >= 5 {
            tracing::error!(
                "Quarantining outbox entry {} ({}): {error_msg}",
                entry.id,
                entry.entity_type
            );
            let _ = taurine_core::db::crud::quarantine_outbox_entry(conn, entry.id, &error_msg);
            None
        } else {
            tracing::warn!(
                "Recording failure for outbox entry {} ({}): {error_msg}",
                entry.id,
                entry.entity_type
            );
            let _ = taurine_core::db::crud::record_outbox_failure(conn, entry.id, &error_msg);
            Some(SyncStatus::Error(format!(
                "Failed to push {} ({status}): {text}",
                entry.entity_type
            )))
        }
    }

    fn handle_outbox_network_error(
        conn: &rusqlite::Connection,
        entry: &taurine_core::db::crud::OutboxEntry,
        e: &reqwest::Error,
    ) -> Option<SyncStatus> {
        let error_msg = format!("Network error: {e}");
        if entry.retry_count >= 5 {
            tracing::error!(
                "Quarantining outbox entry {} ({}) after network errors: {error_msg}",
                entry.id,
                entry.entity_type
            );
            let _ = taurine_core::db::crud::quarantine_outbox_entry(conn, entry.id, &error_msg);
            None
        } else {
            tracing::warn!(
                "Recording failure for outbox entry {} ({}): {error_msg}",
                entry.id,
                entry.entity_type
            );
            let _ = taurine_core::db::crud::record_outbox_failure(conn, entry.id, &error_msg);
            Some(SyncStatus::Error(format!(
                "Network error pushing {}: {e}",
                entry.entity_type
            )))
        }
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

        if taurine_core::db::crud::get_sync_checkpoint(conn, "snippets_backfilled").unwrap_or(0)
            == 0
        {
            let _ = taurine_core::db::crud::triggers::backfill_untracked_triggers_to_outbox(conn);
            let _ = taurine_core::db::crud::set_sync_checkpoint(conn, "snippets_backfilled", 1);
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
            if let Err(e) = client.refresh_session_and_store(&mut tokens).await {
                return SyncStatus::Error(e);
            }
        }

        // 30-day tombstone GC
        if let Err(e) = purge_deleted_tombstones(conn, 30 * 86400) {
            tracing::warn!("Tombstone GC error: {e}");
        }

        let mut pushed_snippets = 0;
        let mut pushed_settings = 0;
        let mut pushed_stats = 0;
        let mut pulled_snippets = 0;
        let mut pulled_settings = 0;

        // Backfill any untracked local triggers into outbox before draining
        let _ = taurine_core::db::crud::triggers::backfill_untracked_triggers_to_outbox(conn);

        // 1. Push Phase (Outbox Drain)
        let mut attempted_ids = std::collections::HashSet::new();
        loop {
            let batch = match taurine_core::db::crud::fetch_outbox_batch(conn, 50) {
                Ok(b) => b,
                Err(e) => return SyncStatus::Error(format!("Failed to fetch outbox batch: {e}")),
            };

            let unattempted: Vec<_> = batch
                .into_iter()
                .filter(|e| !attempted_ids.contains(&e.id))
                .collect();

            if unattempted.is_empty() {
                break;
            }

            for entry in &unattempted {
                attempted_ids.insert(entry.id);

                match entry.entity_type.as_str() {
                    "snippet" => {
                        let mut payload = if entry.operation == "upsert"
                            && let Ok(Some(trigger)) = taurine_core::db::crud::triggers::get_trigger(
                                conn,
                                &entry.entity_id,
                            ) {
                            let aliases = taurine_core::db::crud::triggers::list_aliases(
                                conn,
                                &entry.entity_id,
                            )
                            .unwrap_or_default();
                            let assets = taurine_core::db::crud::triggers::load_trigger_assets(
                                conn,
                                &entry.entity_id,
                            )
                            .unwrap_or_default();
                            let snippet_obj =
                                taurine_core::db::crud::triggers::serialize_trigger_to_cloud_snippet(
                                    &trigger, &aliases, &assets,
                                );
                            serde_json::to_value(&snippet_obj)
                                .unwrap_or_else(|_| entry.payload.clone())
                        } else {
                            entry.payload.clone()
                        };

                        if let serde_json::Value::Object(ref mut map) = payload {
                            map.remove("local_id");
                            map.remove("name");
                            if !map.contains_key("user_id") {
                                map.insert(
                                    "user_id".to_string(),
                                    serde_json::Value::String(tokens.user_id.clone()),
                                );
                            }
                            if let Some(target_os_val) = map.get("target_os")
                                && target_os_val.is_null()
                            {
                                map.insert(
                                    "target_os".to_string(),
                                    serde_json::Value::String("all".to_string()),
                                );
                            } else if !map.contains_key("target_os") {
                                map.insert(
                                    "target_os".to_string(),
                                    serde_json::Value::String("all".to_string()),
                                );
                            }
                        } else if let serde_json::Value::Array(ref mut arr) = payload {
                            for item in arr.iter_mut() {
                                if let serde_json::Value::Object(map) = item {
                                    map.remove("local_id");
                                    map.remove("name");
                                    if !map.contains_key("user_id") {
                                        map.insert(
                                            "user_id".to_string(),
                                            serde_json::Value::String(tokens.user_id.clone()),
                                        );
                                    }
                                    if let Some(target_os_val) = map.get("target_os")
                                        && target_os_val.is_null()
                                    {
                                        map.insert(
                                            "target_os".to_string(),
                                            serde_json::Value::String("all".to_string()),
                                        );
                                    } else if !map.contains_key("target_os") {
                                        map.insert(
                                            "target_os".to_string(),
                                            serde_json::Value::String("all".to_string()),
                                        );
                                    }
                                }
                            }
                        }

                        let push_res = client
                            .postgrest_post("snippets?on_conflict=id", &payload, true, &mut tokens)
                            .await;

                        match push_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    if let Some(err_status) =
                                        Self::handle_outbox_failure(conn, entry, status, &text)
                                    {
                                        return err_status;
                                    }
                                    continue;
                                }
                                let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                                pushed_snippets += 1;
                            }
                            Err(e) => {
                                if let Some(err_status) =
                                    Self::handle_outbox_network_error(conn, entry, &e)
                                {
                                    return err_status;
                                }
                                continue;
                            }
                        }
                    }
                    "setting" => {
                        if !taurine_core::db::crud::is_setting_syncable(&entry.entity_id) {
                            let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                            continue;
                        }

                        let mut payload = entry.payload.clone();
                        if let serde_json::Value::Object(ref mut map) = payload
                            && !map.contains_key("user_id")
                        {
                            map.insert(
                                "user_id".to_string(),
                                serde_json::Value::String(tokens.user_id.clone()),
                            );
                        }

                        let setting_res = client
                            .postgrest_post(
                                "user_settings?on_conflict=user_id,key",
                                &payload,
                                true,
                                &mut tokens,
                            )
                            .await;

                        match setting_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    if let Some(err_status) =
                                        Self::handle_outbox_failure(conn, entry, status, &text)
                                    {
                                        return err_status;
                                    }
                                    continue;
                                }
                                let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                                pushed_settings += 1;
                            }
                            Err(e) => {
                                if let Some(err_status) =
                                    Self::handle_outbox_network_error(conn, entry, &e)
                                {
                                    return err_status;
                                }
                                continue;
                            }
                        }
                    }
                    "quota" => {
                        let device_id = taurine_core::cloud::get_device_hardware_id();
                        let rpc_payload = serde_json::json!({
                            "p_week_epoch": entry.payload.get("week_epoch").and_then(|v| v.as_i64()).unwrap_or(0),
                            "p_delta": entry.payload.get("delta").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            "p_device_id": device_id,
                            "p_mutation_id": entry.payload.get("mutation_id").and_then(|v| v.as_str()).unwrap_or_default(),
                        });

                        let quota_res = client
                            .postgrest_post(
                                "rpc/record_quota_burn",
                                &rpc_payload,
                                false,
                                &mut tokens,
                            )
                            .await;

                        match quota_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    if let Some(err_status) =
                                        Self::handle_outbox_failure(conn, entry, status, &text)
                                    {
                                        return err_status;
                                    }
                                    continue;
                                }

                                let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);

                                if let Ok(val) = res.json::<serde_json::Value>().await {
                                    let new_balance = val
                                        .get("new_balance")
                                        .and_then(|v| v.as_f64())
                                        .or_else(|| val.as_f64());

                                    if let Some(balance) = new_balance {
                                        crate::engine::quota_guard::QuotaGuard::global()
                                            .set_balance(balance);
                                        if let Ok(key) =
                                            taurine_core::db::key::get_or_create_db_key()
                                        {
                                            let week_epoch = entry
                                                .payload
                                                .get("week_epoch")
                                                .and_then(|v| v.as_i64())
                                                .unwrap_or_else(|| {
                                                    taurine_core::db::crud::quota::calculate_week_start_epoch(now_epoch)
                                                });
                                            let _ = taurine_core::db::crud::quota::set_quota_balance_from_cloud(
                                                conn,
                                                &key,
                                                week_epoch,
                                                balance,
                                                now_epoch,
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                if let Some(err_status) =
                                    Self::handle_outbox_network_error(conn, entry, &e)
                                {
                                    return err_status;
                                }
                                continue;
                            }
                        }
                    }
                    "event" | "activity_event" => {
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
                                        updated_at: None,
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

                        let events_res = client
                            .postgrest_post("activity_events", &payload, true, &mut tokens)
                            .await;

                        match events_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    if let Some(err_status) =
                                        Self::handle_outbox_failure(conn, entry, status, &text)
                                    {
                                        return err_status;
                                    }
                                    continue;
                                }
                                let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                if let Some(err_status) =
                                    Self::handle_outbox_network_error(conn, entry, &e)
                                {
                                    return err_status;
                                }
                                continue;
                            }
                        }
                    }
                    "trigger_daily_stat" => {
                        let parts: Vec<&str> = entry.entity_id.splitn(3, ':').collect();
                        let db_stat = if parts.len() == 3 {
                            let device_id = taurine_core::cloud::get_device_hardware_id();
                            conn.query_row(
                                "SELECT trigger_id, date, app_key, executions, words_spoken, words_expanded, time_saved_ms, version
                                 FROM trigger_daily_stats WHERE device_id = ?1 AND trigger_id = ?2 AND date = ?3 AND app_key = ?4",
                                rusqlite::params![device_id, parts[0], parts[1], parts[2]],
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
                                        updated_at: None,
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

                        let stats_res = client
                            .postgrest_post("trigger_daily_stats", &payload, true, &mut tokens)
                            .await;

                        match stats_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    if let Some(err_status) =
                                        Self::handle_outbox_failure(conn, entry, status, &text)
                                    {
                                        return err_status;
                                    }
                                    continue;
                                }
                                let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                if let Some(err_status) =
                                    Self::handle_outbox_network_error(conn, entry, &e)
                                {
                                    return err_status;
                                }
                                continue;
                            }
                        }
                    }
                    "stat" | "user_stat" => {
                        let device_id = taurine_core::cloud::get_device_hardware_id();
                        let db_stat = conn
                            .query_row(
                                "SELECT date, executions, ai_executions, voice_executions, words_spoken, words_expanded, time_saved_ms, version
                                 FROM stats WHERE device_id = ?1 AND date = ?2",
                                rusqlite::params![device_id, &entry.entity_id],
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
                                        updated_at: None,
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

                        let user_stats_res = client
                            .postgrest_post("user_stats", &payload, true, &mut tokens)
                            .await;

                        match user_stats_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    if let Some(err_status) =
                                        Self::handle_outbox_failure(conn, entry, status, &text)
                                    {
                                        return err_status;
                                    }
                                    continue;
                                }
                                let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                if let Some(err_status) =
                                    Self::handle_outbox_network_error(conn, entry, &e)
                                {
                                    return err_status;
                                }
                                continue;
                            }
                        }
                    }
                    "app_stat" | "user_app_stat" => {
                        let parts: Vec<&str> = entry.entity_id.splitn(2, ':').collect();
                        let db_app_stat = if parts.len() == 2 {
                            let device_id = taurine_core::cloud::get_device_hardware_id();
                            conn.query_row(
                                "SELECT app_key, date, executions, words_spoken, words_expanded, time_saved_ms, version
                                 FROM app_stats WHERE device_id = ?1 AND app_key = ?2 AND date = ?3",
                                rusqlite::params![device_id, parts[0], parts[1]],
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
                                        updated_at: None,
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

                        let app_stats_res = client
                            .postgrest_post("user_app_stats", &payload, true, &mut tokens)
                            .await;

                        match app_stats_res {
                            Ok(res) => {
                                if !res.status().is_success() {
                                    let status = res.status();
                                    let text = res.text().await.unwrap_or_default();
                                    if let Some(err_status) =
                                        Self::handle_outbox_failure(conn, entry, status, &text)
                                    {
                                        return err_status;
                                    }
                                    continue;
                                }
                                let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                                pushed_stats += 1;
                            }
                            Err(e) => {
                                if let Some(err_status) =
                                    Self::handle_outbox_network_error(conn, entry, &e)
                                {
                                    return err_status;
                                }
                                continue;
                            }
                        }
                    }
                    unknown => {
                        tracing::warn!("Unknown outbox entity_type: {unknown}");
                        let _ = taurine_core::db::crud::delete_outbox_entry(conn, entry.id);
                    }
                }
            }
        }

        // 2. Pull Phase (Checkpoint Watermark)
        let last_cp = taurine_core::db::crud::get_sync_checkpoint(conn, "snippets").unwrap_or(0);
        let rfc3339_cp = CloudClient::format_rfc3339_timestamp(last_cp);
        let pull_res = client
            .postgrest_get(
                &format!("snippets?updated_at=gt.{rfc3339_cp}&order=updated_at.asc"),
                &mut tokens,
            )
            .await;

        match pull_res {
            Ok(res) => {
                if !res.status().is_success() {
                    let status = res.status();
                    let text = res.text().await.unwrap_or_default();
                    return SyncStatus::Error(format!(
                        "Failed to pull snippets ({status}): {text}"
                    ));
                }
                let text = match res.text().await {
                    Ok(t) => t,
                    Err(e) => {
                        return SyncStatus::Error(format!("Failed to read snippets response: {e}"));
                    }
                };

                let items: Vec<serde_json::Value> = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        return SyncStatus::Error(format!(
                            "Failed to parse remote snippets JSON array: {e}"
                        ));
                    }
                };

                let mut max_ts = last_cp;
                for item in items {
                    let item_updated_at = item
                        .get("updated_at")
                        .and_then(|v| {
                            if let Some(s) = v.as_str() {
                                CloudClient::parse_rfc3339_timestamp(s).ok()
                            } else {
                                v.as_i64()
                            }
                        })
                        .unwrap_or(0);

                    if item_updated_at > max_ts {
                        max_ts = item_updated_at;
                    }

                    match serde_json::from_value::<CloudSnippetPayload>(item) {
                        Ok(snippet) => {
                            if snippet.updated_at > max_ts {
                                max_ts = snippet.updated_at;
                            }
                            match reconcile_cloud_snippet(conn, &snippet) {
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
                        Err(e) => {
                            tracing::warn!("Skipping malformed remote snippet: {e}");
                        }
                    }
                }
                if max_ts > last_cp
                    && let Err(e) =
                        taurine_core::db::crud::set_sync_checkpoint(conn, "snippets", max_ts)
                {
                    tracing::warn!("Failed to set snippets sync checkpoint: {e}");
                }
            }
            Err(e) => {
                return SyncStatus::Error(format!("Network error pulling snippets: {e}"));
            }
        }

        // Pull remote settings
        let last_settings_cp =
            taurine_core::db::crud::get_sync_checkpoint(conn, "user_settings").unwrap_or(0);
        let rfc3339_settings_cp = CloudClient::format_rfc3339_timestamp(last_settings_cp);
        let settings_pull_res = client
            .postgrest_get(
                &format!("user_settings?updated_at=gt.{rfc3339_settings_cp}&order=updated_at.asc"),
                &mut tokens,
            )
            .await;

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
                        let mut max_ts = last_settings_cp;
                        for setting in &remote_settings {
                            let ts = setting
                                .updated_at
                                .as_deref()
                                .and_then(|s| {
                                    taurine_core::chrono::DateTime::parse_from_rfc3339(s).ok()
                                })
                                .map(|dt| dt.timestamp())
                                .unwrap_or(0);
                            if ts > max_ts {
                                max_ts = ts;
                            }
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
                        if max_ts > last_settings_cp
                            && let Err(e) = taurine_core::db::crud::set_sync_checkpoint(
                                conn,
                                "user_settings",
                                max_ts,
                            )
                        {
                            tracing::warn!("Failed to set user_settings sync checkpoint: {e}");
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
        let quota_pull_res = client
            .postgrest_get(
                &format!("quota_ledger?week_start_epoch=eq.{current_week}&limit=1"),
                &mut tokens,
            )
            .await;

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

        // Pull remote activity events
        let last_events_cp =
            taurine_core::db::crud::get_sync_checkpoint(conn, "activity_events").unwrap_or(0);
        let rfc3339_events_cp = CloudClient::format_rfc3339_timestamp(last_events_cp);
        let events_pull_res = client
            .postgrest_get(
                &format!("activity_events?updated_at=gt.{rfc3339_events_cp}&order=updated_at.asc"),
                &mut tokens,
            )
            .await;

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
                        let mut max_ts = last_events_cp;
                        for event in &remote_events {
                            let ts = event
                                .updated_at
                                .as_deref()
                                .and_then(|s| {
                                    taurine_core::chrono::DateTime::parse_from_rfc3339(s).ok()
                                })
                                .map(|dt| dt.timestamp())
                                .unwrap_or(if event.timestamp > 1_000_000_000_000 {
                                    event.timestamp / 1000
                                } else {
                                    event.timestamp
                                });
                            if ts > max_ts {
                                max_ts = ts;
                            }
                        }
                        match reconcile_cloud_activity_events(conn, &remote_events) {
                            Ok(count) => {
                                pulled_events += count;
                            }
                            Err(e) => {
                                tracing::warn!("Failed to reconcile activity events: {e}");
                            }
                        }
                        if max_ts > last_events_cp
                            && let Err(e) = taurine_core::db::crud::set_sync_checkpoint(
                                conn,
                                "activity_events",
                                max_ts,
                            )
                        {
                            tracing::warn!("Failed to set activity_events sync checkpoint: {e}");
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

        // Pull remote trigger daily stats
        let last_trigger_stats_cp =
            taurine_core::db::crud::get_sync_checkpoint(conn, "trigger_daily_stats").unwrap_or(0);
        let rfc3339_trigger_stats_cp = CloudClient::format_rfc3339_timestamp(last_trigger_stats_cp);
        let trigger_stats_pull_res = client
            .postgrest_get(
                &format!("trigger_daily_stats?updated_at=gt.{rfc3339_trigger_stats_cp}&order=updated_at.asc"),
                &mut tokens,
            )
            .await;

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
                        let mut max_ts = last_trigger_stats_cp;
                        for stat in &remote_stats {
                            let ts = stat
                                .updated_at
                                .as_deref()
                                .and_then(|s| {
                                    taurine_core::chrono::DateTime::parse_from_rfc3339(s).ok()
                                })
                                .map(|dt| dt.timestamp())
                                .unwrap_or(0);
                            if ts > max_ts {
                                max_ts = ts;
                            }
                        }
                        if let Err(e) = reconcile_cloud_trigger_daily_stats(conn, &remote_stats) {
                            tracing::warn!("Failed to reconcile trigger daily stats: {e}");
                        }
                        if max_ts > last_trigger_stats_cp
                            && let Err(e) = taurine_core::db::crud::set_sync_checkpoint(
                                conn,
                                "trigger_daily_stats",
                                max_ts,
                            )
                        {
                            tracing::warn!(
                                "Failed to set trigger_daily_stats sync checkpoint: {e}"
                            );
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

        // Pull remote global user stats
        let last_user_stats_cp =
            taurine_core::db::crud::get_sync_checkpoint(conn, "user_stats").unwrap_or(0);
        let rfc3339_user_stats_cp = CloudClient::format_rfc3339_timestamp(last_user_stats_cp);
        let user_stats_pull_res = client
            .postgrest_get(
                &format!("user_stats?updated_at=gt.{rfc3339_user_stats_cp}&order=updated_at.asc"),
                &mut tokens,
            )
            .await;

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
                        let mut max_ts = last_user_stats_cp;
                        for stat in &remote_stats {
                            let ts = stat
                                .updated_at
                                .as_deref()
                                .and_then(|s| {
                                    taurine_core::chrono::DateTime::parse_from_rfc3339(s).ok()
                                })
                                .map(|dt| dt.timestamp())
                                .unwrap_or(0);
                            if ts > max_ts {
                                max_ts = ts;
                            }
                        }
                        if let Err(e) = reconcile_cloud_user_stats(conn, &remote_stats) {
                            tracing::warn!("Failed to reconcile user stats: {e}");
                        }
                        if max_ts > last_user_stats_cp
                            && let Err(e) = taurine_core::db::crud::set_sync_checkpoint(
                                conn,
                                "user_stats",
                                max_ts,
                            )
                        {
                            tracing::warn!("Failed to set user_stats sync checkpoint: {e}");
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

        // Pull remote user app stats
        let last_user_app_stats_cp =
            taurine_core::db::crud::get_sync_checkpoint(conn, "user_app_stats").unwrap_or(0);
        let rfc3339_user_app_stats_cp =
            CloudClient::format_rfc3339_timestamp(last_user_app_stats_cp);
        let app_stats_pull_res = client
            .postgrest_get(
                &format!(
                    "user_app_stats?updated_at=gt.{rfc3339_user_app_stats_cp}&order=updated_at.asc"
                ),
                &mut tokens,
            )
            .await;

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
                        let mut max_ts = last_user_app_stats_cp;
                        for stat in &remote_stats {
                            let ts = stat
                                .updated_at
                                .as_deref()
                                .and_then(|s| {
                                    taurine_core::chrono::DateTime::parse_from_rfc3339(s).ok()
                                })
                                .map(|dt| dt.timestamp())
                                .unwrap_or(0);
                            if ts > max_ts {
                                max_ts = ts;
                            }
                        }
                        if let Err(e) = reconcile_cloud_app_stats(conn, &remote_stats) {
                            tracing::warn!("Failed to reconcile app stats: {e}");
                        }
                        if max_ts > last_user_app_stats_cp
                            && let Err(e) = taurine_core::db::crud::set_sync_checkpoint(
                                conn,
                                "user_app_stats",
                                max_ts,
                            )
                        {
                            tracing::warn!("Failed to set user_app_stats sync checkpoint: {e}");
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
