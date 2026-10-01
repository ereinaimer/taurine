use std::sync::{Mutex, OnceLock};

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

    /// Checks if expansions are permitted (< 0.1ms hot-path check).
    pub fn can_expand(&self) -> bool {
        self.balance() > 0.0
    }

    /// Checks if quota is fully depleted (<= 0.0%).
    pub fn is_depleted(&self) -> bool {
        self.balance() <= 0.0
    }

    /// Checks if quota is in warning zone (<= 10.0%).
    pub fn is_warning(&self) -> bool {
        self.balance() <= 10.0
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
    /// Returns true if permitted and burned, false if blocked due to depletion.
    /// Voice dictation is free and unlimited, so it always returns true.
    pub fn deplete(&self, exp_type: ExpansionType) -> bool {
        if exp_type == ExpansionType::VoiceDictation {
            return true;
        }

        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.balance <= 0.0 {
            return false;
        }

        let cost = exp_type.cost();
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
            Ok(_) => Ok(()),
            Err(err) => {
                let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
                inner.unflushed_delta += delta;
                Err(err)
            }
        }
    }
}
