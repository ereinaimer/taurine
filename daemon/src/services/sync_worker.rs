use rusqlite::OptionalExtension;
use std::time::Duration;
use taurine_core::cloud::CloudClient;
use taurine_core::db::crud::triggers::{
    CloudSnippetPayload, ReconcileOutcome, purge_deleted_tombstones, reconcile_cloud_snippet,
    serialize_trigger_to_cloud_snippet,
};
use taurine_core::db::crud::{
    CloudActivityEventPayload, CloudSettingPayload, CloudTriggerDailyStatPayload,
    CloudUserAppStatPayload, CloudUserStatPayload, SettingReconcileOutcome,
    get_unsynced_activity_events, get_unsynced_app_stats, get_unsynced_settings,
    get_unsynced_stats, get_unsynced_trigger_daily_stats, mark_activity_events_synced,
    mark_app_stats_synced, mark_settings_synced, mark_stats_synced,
    mark_trigger_daily_stats_synced, reconcile_cloud_activity_events, reconcile_cloud_app_stats,
    reconcile_cloud_setting, reconcile_cloud_trigger_daily_stats, reconcile_cloud_user_stats,
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

        // 1. Pull remote quota ledger for the current week
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
                let local_quota: Option<(i64, f64, Option<i64>, bool, i64)> = conn
                    .query_row(
                        "SELECT week_start_epoch, remaining_percentage, last_expansion_at, is_synced, updated_at
                         FROM quota_ledger WHERE week_start_epoch = ?1",
                        [current_week],
                        |row| {
                            Ok((
                                row.get(0)?,
                                row.get(1)?,
                                row.get(2)?,
                                row.get(3)?,
                                row.get(4)?,
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
                        Some((_, local_pct, local_last_exp, local_is_synced, local_updated_at)) => {
                            if local_is_synced {
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
                            } else if local_last_exp.is_none() {
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
                            } else if remote.remaining_percentage < local_pct {
                                let merged_pct = remote.remaining_percentage.min(local_pct);
                                let sig = taurine_core::db::crud::quota::compute_quota_hmac(
                                    &key,
                                    current_week,
                                    merged_pct,
                                );
                                let _ = conn.execute(
                                    "UPDATE quota_ledger SET remaining_percentage = ?1, hmac_signature = ?2, is_synced = 0, updated_at = ?3 WHERE week_start_epoch = ?4",
                                    rusqlite::params![merged_pct, sig, now_epoch, current_week],
                                );
                                crate::engine::quota_guard::QuotaGuard::global()
                                    .set_balance(merged_pct);
                            }
                        }
                    }
                }
            }
        }

        // Push pending quota ledger deltas if any
        let unsynced_quota: Option<taurine_core::db::crud::quota::QuotaLedgerRow> = conn
            .query_row(
                "SELECT week_start_epoch, remaining_percentage, last_expansion_at, hmac_signature, is_synced, updated_at
                 FROM quota_ledger WHERE is_synced = 0 LIMIT 1",
                [],
                |row| {
                    Ok(taurine_core::db::crud::quota::QuotaLedgerRow {
                        week_start_epoch: row.get(0)?,
                        remaining_percentage: row.get(1)?,
                        last_expansion_at: row.get(2)?,
                        hmac_signature: row.get(3)?,
                        is_synced: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                },
            )
            .optional()
            .unwrap_or(None);

        if let Some(quota) = unsynced_quota {
            let quota_url = format!(
                "{}/rest/v1/quota_ledger",
                client.config().supabase_url.trim_end_matches('/')
            );
            let payload = serde_json::json!({
                "user_id": tokens.user_id,
                "week_start_epoch": quota.week_start_epoch,
                "remaining_percentage": quota.remaining_percentage,
                "last_expansion_at": quota.last_expansion_at,
                "updated_at": quota.updated_at,
            });

            let mut quota_res = http_client
                .post(&quota_url)
                .header("apikey", &client.config().anon_key)
                .header("Authorization", format!("Bearer {}", tokens.access_token))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payload)
                .send()
                .await;

            if let Ok(ref r) = quota_res
                && r.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                tracing::debug!("Received 401 pushing quota ledger; refreshing session...");
                if Self::refresh_session_tokens(client, &mut tokens)
                    .await
                    .is_ok()
                {
                    quota_res = http_client
                        .post(&quota_url)
                        .header("apikey", &client.config().anon_key)
                        .header("Authorization", format!("Bearer {}", tokens.access_token))
                        .header("Content-Type", "application/json")
                        .header("Prefer", "resolution=merge-duplicates")
                        .json(&payload)
                        .send()
                        .await;
                }
            }

            if let Ok(r) = quota_res
                && r.status().is_success()
            {
                let _ = conn.execute(
                    "UPDATE quota_ledger SET is_synced = 1 WHERE week_start_epoch = ?1",
                    [quota.week_start_epoch],
                );
            }
        }

        // Pull remote snippets
        let snippets_url = format!(
            "{}/rest/v1/snippets?select=*&order=version.asc,updated_at.asc",
            client.config().supabase_url.trim_end_matches('/')
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

        let mut pulled_snippets = 0;
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
                        for snippet in &remote_snippets {
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

        // Push local modifications
        let unsynced = match taurine_core::db::crud::triggers::get_unsynced_triggers(conn) {
            Ok(rows) => rows,
            Err(e) => return SyncStatus::Error(format!("Failed to query unsynced triggers: {e}")),
        };

        let mut pushed_snippets = 0;
        if !unsynced.is_empty() {
            let mut payloads = Vec::with_capacity(unsynced.len());
            for trigger in &unsynced {
                let mut snippet = serialize_trigger_to_cloud_snippet(trigger, &trigger.invocations);
                snippet.user_id = Some(tokens.user_id.clone());
                payloads.push(snippet);
            }

            let push_url = format!(
                "{}/rest/v1/snippets",
                client.config().supabase_url.trim_end_matches('/')
            );

            let mut push_res = http_client
                .post(&push_url)
                .header("apikey", &client.config().anon_key)
                .header("Authorization", format!("Bearer {}", tokens.access_token))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payloads)
                .send()
                .await;

            if let Ok(ref r) = push_res
                && r.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                tracing::debug!("Received 401 pushing snippets; refreshing session...");
                if Self::refresh_session_tokens(client, &mut tokens)
                    .await
                    .is_ok()
                {
                    push_res = http_client
                        .post(&push_url)
                        .header("apikey", &client.config().anon_key)
                        .header("Authorization", format!("Bearer {}", tokens.access_token))
                        .header("Content-Type", "application/json")
                        .header("Prefer", "resolution=merge-duplicates")
                        .json(&payloads)
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
                            "Failed to push snippets ({status}): {text}"
                        ));
                    }
                    for p in &payloads {
                        let target_id = p.local_id.as_deref().unwrap_or(&p.id);
                        if let Err(e) =
                            taurine_core::db::crud::triggers::mark_trigger_synced(conn, target_id)
                        {
                            tracing::warn!("Failed to mark trigger {} synced: {e}", target_id);
                        } else {
                            pushed_snippets += 1;
                        }
                    }
                }
                Err(e) => {
                    return SyncStatus::Error(format!("Network error pushing snippets: {e}"));
                }
            }
        }

        // Pull remote settings
        let settings_url = format!(
            "{}/rest/v1/user_settings?select=*&order=version.asc,updated_at.asc",
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

        let mut pulled_settings = 0;
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

        // Push local unsynced settings
        let unsynced_settings = match get_unsynced_settings(conn) {
            Ok(rows) => rows,
            Err(e) => return SyncStatus::Error(format!("Failed to query unsynced settings: {e}")),
        };

        let mut pushed_settings = 0;
        if !unsynced_settings.is_empty() {
            let payloads: Vec<CloudSettingPayload> = unsynced_settings
                .iter()
                .map(|s| {
                    let value: serde_json::Value = serde_json::from_str(&s.value)
                        .unwrap_or_else(|_| serde_json::Value::String(s.value.clone()));
                    let updated_at = Some(Self::unix_secs_to_rfc3339(s.updated_at));
                    CloudSettingPayload {
                        user_id: Some(tokens.user_id.clone()),
                        key: s.key.clone(),
                        value,
                        version: s.version,
                        updated_at,
                    }
                })
                .collect();

            let push_settings_url = format!(
                "{}/rest/v1/user_settings",
                client.config().supabase_url.trim_end_matches('/')
            );

            let mut push_res = http_client
                .post(&push_settings_url)
                .header("apikey", &client.config().anon_key)
                .header("Authorization", format!("Bearer {}", tokens.access_token))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payloads)
                .send()
                .await;

            if let Ok(ref r) = push_res
                && r.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                tracing::debug!("Received 401 pushing settings; refreshing session...");
                if Self::refresh_session_tokens(client, &mut tokens)
                    .await
                    .is_ok()
                {
                    push_res = http_client
                        .post(&push_settings_url)
                        .header("apikey", &client.config().anon_key)
                        .header("Authorization", format!("Bearer {}", tokens.access_token))
                        .header("Content-Type", "application/json")
                        .header("Prefer", "resolution=merge-duplicates")
                        .json(&payloads)
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
                            "Failed to push settings ({status}): {text}"
                        ));
                    }
                    let keys: Vec<&str> =
                        unsynced_settings.iter().map(|s| s.key.as_str()).collect();
                    if let Err(e) = mark_settings_synced(conn, &keys) {
                        tracing::warn!("Failed to mark settings synced: {e}");
                    } else {
                        pushed_settings += keys.len();
                    }
                }
                Err(e) => {
                    return SyncStatus::Error(format!("Network error pushing settings: {e}"));
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

        // Upload unsynced trigger daily stats
        let unsynced_trigger_stats = match get_unsynced_trigger_daily_stats(conn, 100) {
            Ok(rows) => rows,
            Err(e) => {
                return SyncStatus::Error(format!(
                    "Failed to query unsynced trigger daily stats: {e}"
                ));
            }
        };

        let mut pushed_stats = 0;
        if !unsynced_trigger_stats.is_empty() {
            let payloads: Vec<CloudTriggerDailyStatPayload> = unsynced_trigger_stats
                .iter()
                .map(|s| {
                    let mut payload = CloudTriggerDailyStatPayload::from(s);
                    payload.user_id = Some(tokens.user_id.clone());
                    payload
                })
                .collect();

            let trigger_stats_url = format!(
                "{}/rest/v1/trigger_daily_stats",
                client.config().supabase_url.trim_end_matches('/')
            );

            let mut trigger_stats_res = http_client
                .post(&trigger_stats_url)
                .header("apikey", &client.config().anon_key)
                .header("Authorization", format!("Bearer {}", tokens.access_token))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payloads)
                .send()
                .await;

            if let Ok(ref r) = trigger_stats_res
                && r.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                tracing::debug!("Received 401 pushing trigger daily stats; refreshing session...");
                if Self::refresh_session_tokens(client, &mut tokens)
                    .await
                    .is_ok()
                {
                    trigger_stats_res = http_client
                        .post(&trigger_stats_url)
                        .header("apikey", &client.config().anon_key)
                        .header("Authorization", format!("Bearer {}", tokens.access_token))
                        .header("Content-Type", "application/json")
                        .header("Prefer", "resolution=merge-duplicates")
                        .json(&payloads)
                        .send()
                        .await;
                }
            }

            match trigger_stats_res {
                Ok(res) => {
                    if !res.status().is_success() {
                        let status = res.status();
                        let text = res.text().await.unwrap_or_default();
                        return SyncStatus::Error(format!(
                            "Failed to push trigger daily stats ({status}): {text}"
                        ));
                    }
                    let items: Vec<(&str, &str, &str)> = unsynced_trigger_stats
                        .iter()
                        .map(|s| (s.trigger_id.as_str(), s.date.as_str(), s.app_key.as_str()))
                        .collect();
                    if let Err(e) = mark_trigger_daily_stats_synced(conn, &items) {
                        tracing::warn!("Failed to mark trigger daily stats synced: {e}");
                    } else {
                        pushed_stats += items.len();
                    }
                }
                Err(e) => {
                    return SyncStatus::Error(format!(
                        "Network error pushing trigger daily stats: {e}"
                    ));
                }
            }
        }

        // Upload unsynced global user stats
        let unsynced_global_stats = match get_unsynced_stats(conn, 100) {
            Ok(rows) => rows,
            Err(e) => {
                return SyncStatus::Error(format!("Failed to query unsynced global stats: {e}"));
            }
        };

        if !unsynced_global_stats.is_empty() {
            let payloads: Vec<CloudUserStatPayload> = unsynced_global_stats
                .iter()
                .map(|s| {
                    let mut payload = CloudUserStatPayload::from(s);
                    payload.user_id = Some(tokens.user_id.clone());
                    payload
                })
                .collect();

            let user_stats_url = format!(
                "{}/rest/v1/user_stats",
                client.config().supabase_url.trim_end_matches('/')
            );

            let mut user_stats_res = http_client
                .post(&user_stats_url)
                .header("apikey", &client.config().anon_key)
                .header("Authorization", format!("Bearer {}", tokens.access_token))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payloads)
                .send()
                .await;

            if let Ok(ref r) = user_stats_res
                && r.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                tracing::debug!("Received 401 pushing user stats; refreshing session...");
                if Self::refresh_session_tokens(client, &mut tokens)
                    .await
                    .is_ok()
                {
                    user_stats_res = http_client
                        .post(&user_stats_url)
                        .header("apikey", &client.config().anon_key)
                        .header("Authorization", format!("Bearer {}", tokens.access_token))
                        .header("Content-Type", "application/json")
                        .header("Prefer", "resolution=merge-duplicates")
                        .json(&payloads)
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
                    let dates: Vec<&str> = unsynced_global_stats
                        .iter()
                        .map(|s| s.date.as_str())
                        .collect();
                    if let Err(e) = mark_stats_synced(conn, &dates) {
                        tracing::warn!("Failed to mark global stats synced: {e}");
                    } else {
                        pushed_stats += dates.len();
                    }
                }
                Err(e) => {
                    return SyncStatus::Error(format!("Network error pushing user stats: {e}"));
                }
            }
        }

        // Upload unsynced user app stats
        let unsynced_app_stats = match get_unsynced_app_stats(conn, 100) {
            Ok(rows) => rows,
            Err(e) => return SyncStatus::Error(format!("Failed to query unsynced app stats: {e}")),
        };

        if !unsynced_app_stats.is_empty() {
            let payloads: Vec<CloudUserAppStatPayload> = unsynced_app_stats
                .iter()
                .map(|s| {
                    let mut payload = CloudUserAppStatPayload::from(s);
                    payload.user_id = Some(tokens.user_id.clone());
                    payload
                })
                .collect();

            let user_app_stats_url = format!(
                "{}/rest/v1/user_app_stats",
                client.config().supabase_url.trim_end_matches('/')
            );

            let mut app_stats_res = http_client
                .post(&user_app_stats_url)
                .header("apikey", &client.config().anon_key)
                .header("Authorization", format!("Bearer {}", tokens.access_token))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payloads)
                .send()
                .await;

            if let Ok(ref r) = app_stats_res
                && r.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                tracing::debug!("Received 401 pushing app stats; refreshing session...");
                if Self::refresh_session_tokens(client, &mut tokens)
                    .await
                    .is_ok()
                {
                    app_stats_res = http_client
                        .post(&user_app_stats_url)
                        .header("apikey", &client.config().anon_key)
                        .header("Authorization", format!("Bearer {}", tokens.access_token))
                        .header("Content-Type", "application/json")
                        .header("Prefer", "resolution=merge-duplicates")
                        .json(&payloads)
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
                    let keys: Vec<(&str, &str)> = unsynced_app_stats
                        .iter()
                        .map(|s| (s.app_key.as_str(), s.date.as_str()))
                        .collect();
                    if let Err(e) = mark_app_stats_synced(conn, &keys) {
                        tracing::warn!("Failed to mark app stats synced: {e}");
                    } else {
                        pushed_stats += keys.len();
                    }
                }
                Err(e) => {
                    return SyncStatus::Error(format!("Network error pushing app stats: {e}"));
                }
            }
        }

        // Upload unsynced activity events
        let unsynced_events = match get_unsynced_activity_events(conn, 100) {
            Ok(rows) => rows,
            Err(e) => {
                return SyncStatus::Error(format!("Failed to query unsynced activity events: {e}"));
            }
        };

        if !unsynced_events.is_empty() {
            let payloads: Vec<CloudActivityEventPayload> = unsynced_events
                .iter()
                .map(|s| {
                    let mut payload = CloudActivityEventPayload::from(s);
                    payload.user_id = Some(tokens.user_id.clone());
                    payload
                })
                .collect();

            let events_url = format!(
                "{}/rest/v1/activity_events",
                client.config().supabase_url.trim_end_matches('/')
            );

            let mut events_res = http_client
                .post(&events_url)
                .header("apikey", &client.config().anon_key)
                .header("Authorization", format!("Bearer {}", tokens.access_token))
                .header("Content-Type", "application/json")
                .header("Prefer", "resolution=merge-duplicates")
                .json(&payloads)
                .send()
                .await;

            if let Ok(ref r) = events_res
                && r.status() == reqwest::StatusCode::UNAUTHORIZED
            {
                tracing::debug!("Received 401 pushing activity events; refreshing session...");
                if Self::refresh_session_tokens(client, &mut tokens)
                    .await
                    .is_ok()
                {
                    events_res = http_client
                        .post(&events_url)
                        .header("apikey", &client.config().anon_key)
                        .header("Authorization", format!("Bearer {}", tokens.access_token))
                        .header("Content-Type", "application/json")
                        .header("Prefer", "resolution=merge-duplicates")
                        .json(&payloads)
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
                            "Failed to push activity events ({status}): {text}"
                        ));
                    }
                    let ids: Vec<&str> = unsynced_events.iter().map(|s| s.id.as_str()).collect();
                    if let Err(e) = mark_activity_events_synced(conn, &ids) {
                        tracing::warn!("Failed to mark activity events synced: {e}");
                    } else {
                        pushed_stats += ids.len();
                    }
                }
                Err(e) => {
                    return SyncStatus::Error(format!(
                        "Network error pushing activity events: {e}"
                    ));
                }
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
