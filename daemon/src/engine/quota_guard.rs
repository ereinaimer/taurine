use std::sync::{Mutex, OnceLock};

/// Quota cost deducted per transcribed word in voice dictation (-0.004%).
pub const VOICE_COST_PER_WORD: f64 = 0.004;

/// Type of expansion that consumes weekly countdown quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionType {
    /// Basic static text expansion (-0.30%).
    Text,
    /// Dynamic expansion containing scripts, calculations, or inline execution (-0.60%).
    Dynamic,
    /// Voice trigger expansion (-0.90%).
    VoiceTrigger,
    /// Voice dictation speech-to-text (free & unlimited: 0.00%).
    VoiceDictation,
}

impl ExpansionType {
    /// Returns the quota percentage cost for this expansion type.
    pub fn cost(&self) -> f64 {
        match self {
            Self::Text => 0.30,
            Self::Dynamic => 0.60,
            Self::VoiceTrigger => 0.90,
            Self::VoiceDictation => 0.00,
        }
    }
}

#[derive(Debug, Clone)]
struct QuotaInner {
    balance: f64,
    unflushed_delta: f64,
    is_unlimited: bool,
    is_authenticated: bool,
}

/// In-memory fast-path guard that monitors and depletes weekly countdown quota.
///
/// Starting each Monday at 00:00 UTC at 100.0%, quota decreases per expansion.
/// Expansions are halted once quota hits 0.0%.
#[allow(dead_code)]
pub struct QuotaGuard {
    inner: Mutex<QuotaInner>,
}

static GLOBAL_GUARD: OnceLock<QuotaGuard> = OnceLock::new();

impl Default for QuotaGuard {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl QuotaGuard {
    /// Creates a new QuotaGuard initialized to the default 100.0% balance.
    pub fn new() -> Self {
        Self::new_with_balance(100.0)
    }

    /// Creates a new QuotaGuard with the specified initial balance percentage.
    pub fn new_with_balance(initial_balance: f64) -> Self {
        Self {
            inner: Mutex::new(QuotaInner {
                balance: initial_balance.clamp(0.0, 100.0),
                unflushed_delta: 0.0,
                is_unlimited: false,
                is_authenticated: true,
            }),
        }
    }

    /// Returns a reference to the process-global singleton QuotaGuard.
    pub fn global() -> &'static QuotaGuard {
        GLOBAL_GUARD.get_or_init(QuotaGuard::new)
    }

    /// Replaces the global singleton's state with the provided guard's state.
    pub fn set_global(guard: QuotaGuard) {
        let global = Self::global();
        let other_inner = guard
            .inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        *global.inner.lock().unwrap_or_else(|e| e.into_inner()) = other_inner;
    }

    /// Initializes or resets the global singleton's balance.
    pub fn init_global(initial_balance: f64) {
        Self::global().set_balance(initial_balance);
    }

    /// Sets whether the user is authenticated with Taurine Cloud.
    pub fn set_authenticated(&self, authenticated: bool) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.is_authenticated = authenticated;
    }

    /// Returns whether the user is authenticated with Taurine Cloud.
    pub fn is_authenticated(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_authenticated
    }

    /// Sets whether the quota guard operates in unlimited mode (bypassing quota depletion).
    pub fn set_unlimited(&self, unlimited: bool) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.is_unlimited = unlimited;
    }

    /// Returns whether the quota guard operates in unlimited mode.
    pub fn is_unlimited(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_unlimited
    }

    /// Checks if expansions are permitted (< 0.1ms hot-path check).
    pub fn can_expand(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.is_authenticated && (inner.is_unlimited || inner.balance > 0.0)
    }

    /// Checks if quota is fully depleted (<= 0.0%).
    pub fn is_depleted(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        !inner.is_unlimited && inner.balance <= 0.0
    }

    /// Checks if quota is in warning zone (<= 10.0%).
    pub fn is_warning(&self) -> bool {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        !inner.is_unlimited && inner.balance <= 10.0
    }

    /// Returns the current balance percentage (0.0 to 100.0).
    pub fn balance(&self) -> f64 {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).balance
    }

    /// Returns the accumulated unflushed quota delta percentage.
    pub fn unflushed_delta(&self) -> f64 {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unflushed_delta
    }

    /// Depletes quota for given expansion type.
    ///
    /// Returns true if permitted and burned, false if blocked due to depletion or unauthenticated state.
    /// Voice dictation is free and unlimited, so it returns true as long as the user is authenticated.
    pub fn deplete(&self, exp_type: ExpansionType) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.is_authenticated {
            return false;
        }

        if exp_type == ExpansionType::VoiceDictation {
            return true;
        }

        if inner.is_unlimited {
            return true;
        }

        if inner.balance <= 0.0 {
            return false;
        }

        let cost = exp_type.cost();
        inner.balance = (inner.balance - cost).max(0.0);
        inner.unflushed_delta += cost;
        true
    }

    /// Depletes quota for voice dictation based on spoken word count.
    ///
    /// Deducts 0.004% per word from remaining quota balance and tracks
    /// unflushed delta for database persistence.
    pub fn deplete_words(&self, word_count: usize) -> bool {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !inner.is_authenticated {
            return false;
        }

        if inner.is_unlimited {
            return true;
        }

        if inner.balance <= 0.0 {
            return false;
        }

        let cost = word_count as f64 * VOICE_COST_PER_WORD;
        inner.balance = (inner.balance - cost).max(0.0);
        inner.unflushed_delta += cost;
        true
    }

    /// Resets or sets the balance from cloud sync or week rollover.
    pub fn set_balance(&self, new_balance: f64) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.balance = new_balance.clamp(0.0, 100.0);
        inner.unflushed_delta = 0.0;
    }

    /// Flushes unpersisted accumulated delta to local SQLite database.
    pub fn flush_to_db(
        &self,
        conn: &rusqlite::Connection,
        hmac_key: &[u8; 32],
    ) -> taurine_core::Result<()> {
        let delta = {
            let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            if inner.unflushed_delta <= 0.0 {
                return Ok(());
            }
            let delta = inner.unflushed_delta;
            inner.unflushed_delta = 0.0;
            delta
        };

        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        match taurine_core::db::crud::quota::record_quota_depletion(
            conn, hmac_key, delta, now_epoch,
        ) {
            Ok(row) => {
                let week_epoch = row.week_start_epoch;
                let mutation_id = uuid::Uuid::new_v4().to_string();
                let payload = serde_json::json!({
                    "week_epoch": week_epoch,
                    "delta": delta,
                    "mutation_id": mutation_id
                });
                let _ = taurine_core::db::crud::enqueue_outbox(
                    conn,
                    "quota",
                    &week_epoch.to_string(),
                    "burn",
                    payload,
                );
                Ok(())
            }
            Err(err) => {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                inner.unflushed_delta += delta;
                Err(err)
            }
        }
    }
}
