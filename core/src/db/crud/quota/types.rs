use serde::{Deserialize, Serialize};

/// Represents a persistent snapshot of the user's weekly quota ledger.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaLedgerRow {
    pub week_start_epoch: i64,
    pub remaining_percentage: f64,
    pub last_expansion_at: Option<i64>,
    pub hmac_signature: String,
    pub is_synced: bool,
    pub updated_at: i64,
}
