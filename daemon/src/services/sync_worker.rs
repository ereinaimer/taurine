use rusqlite::OptionalExtension;
use std::time::Duration;
use taurine_core::cloud::CloudClient;
use taurine_core::db::crud::triggers::{
    CloudSnippetPayload, ReconcileOutcome, purge_deleted_tombstones, reconcile_cloud_snippet,
    serialize_trigger_to_cloud_snippet,
};

/// The execution outcome of a sync cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncStatus {
    NotAuthenticated,
    Success {
        pulled_snippets: usize,
        pushed_snippets: usize,
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
                                match status {
                                    SyncStatus::Success {
                                        pulled_snippets,
                                        pushed_snippets,
                                    } => {
                                        if pulled_snippets > 0 || pushed_snippets > 0 {
                                            tracing::info!(
                                                pulled = pulled_snippets,
                                                pushed = pushed_snippets,
                                                "Cloud sync cycle succeeded"
                                            );
                                        } else {
                                            tracing::debug!(
                                                "Cloud sync cycle succeeded (no changes)"
                                            );
                                        }
                                    }
                                    SyncStatus::Error(err) => {
                                        tracing::warn!("Cloud sync cycle failed: {err}");
                                    }
                                    SyncStatus::NotAuthenticated => {
                                        tracing::debug!(
                                            "Cloud sync skipped: user not authenticated"
                                        );
                                    }
                                }
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
                payloads.push(serialize_trigger_to_cloud_snippet(
                    trigger,
                    &trigger.invocations,
                ));
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
                        if let Err(e) =
                            taurine_core::db::crud::triggers::mark_trigger_synced(conn, &p.id)
                        {
                            tracing::warn!("Failed to mark trigger {} synced: {e}", p.id);
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

        SyncStatus::Success {
            pulled_snippets,
            pushed_snippets,
        }
    }
}
