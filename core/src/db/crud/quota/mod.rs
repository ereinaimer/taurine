pub mod hmac;
pub mod types;

#[cfg(test)]
mod tests;

pub use hmac::{compute_quota_hmac, verify_quota_hmac};
pub use types::QuotaLedgerRow;

use chrono::{DateTime, Datelike, NaiveTime};
use rusqlite::Connection;

/// Calculates the Monday 00:00:00 UTC epoch timestamp for any given Unix epoch timestamp.
pub fn calculate_week_start_epoch(epoch_seconds: i64) -> i64 {
    if let Some(dt) = DateTime::from_timestamp(epoch_seconds, 0) {
        let naive_date = dt.date_naive();
        let days_from_monday = dt.weekday().num_days_from_monday() as i64;
        let monday = naive_date - chrono::Duration::days(days_from_monday);
        return monday.and_time(NaiveTime::MIN).and_utc().timestamp();
    }
    // Fallback: Thursday 1970-01-01 was 3 days after Monday 1969-12-29 (-259_200s).
    (epoch_seconds + 259_200).div_euclid(604_800) * 604_800 - 259_200
}

/// Internal helper to retrieve a quota ledger row by its week start epoch.
fn query_quota_row(conn: &Connection, week_epoch: i64) -> crate::Result<Option<QuotaLedgerRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT week_start_epoch, remaining_percentage, last_expansion_at, hmac_signature, is_synced, updated_at
         FROM quota_ledger
         WHERE week_start_epoch = ?1",
    )?;
    let result = stmt.query_row([week_epoch], |row| {
        Ok(QuotaLedgerRow {
            week_start_epoch: row.get(0)?,
            remaining_percentage: row.get(1)?,
            last_expansion_at: row.get(2)?,
            hmac_signature: row.get(3)?,
            is_synced: row.get(4)?,
            updated_at: row.get(5)?,
        })
    });
    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(crate::Error::Database(e)),
    }
}

/// Retrieves the current week's quota ledger row, seeding it at 100.0% if missing or rolled over.
/// Validates the HMAC signature and fails closed if corrupted or tampered with.
pub fn get_or_init_quota_ledger(
    conn: &Connection,
    key: &[u8; 32],
    now_epoch: i64,
) -> crate::Result<QuotaLedgerRow> {
    let current_week = calculate_week_start_epoch(now_epoch);

    if let Some(row) = query_quota_row(conn, current_week)? {
        if !verify_quota_hmac(
            key,
            row.week_start_epoch,
            row.remaining_percentage,
            &row.hmac_signature,
        ) {
            return Err(crate::Error::Engine(
                "HMAC signature verification failed for quota ledger: possible tampering detected"
                    .to_string(),
            ));
        }
        return Ok(row);
    }

    // Check for clock tampering / rollback: reject if a quota ledger exists for a future week.
    let mut future_stmt = conn.prepare_cached(
        "SELECT week_start_epoch FROM quota_ledger WHERE week_start_epoch > ?1 LIMIT 1",
    )?;
    let has_future: bool = future_stmt
        .query_row([current_week], |_| Ok(true))
        .unwrap_or(false);
    if has_future {
        return Err(crate::Error::Engine(
            "System clock rollback detected: quota ledger exists for a future week".to_string(),
        ));
    }

    // Seed new week row with 100.0%
    let initial_pct = 100.0;
    let signature = compute_quota_hmac(key, current_week, initial_pct);
    let rows_affected = conn.execute(
        "INSERT OR IGNORE INTO quota_ledger (
             week_start_epoch,
             remaining_percentage,
             last_expansion_at,
             hmac_signature,
             is_synced,
             updated_at
         ) VALUES (?1, ?2, NULL, ?3, 0, ?4)",
        (current_week, initial_pct, &signature, now_epoch),
    )?;
    if rows_affected == 0 {
        let row = query_quota_row(conn, current_week)?
            .ok_or_else(|| crate::Error::Database(rusqlite::Error::QueryReturnedNoRows))?;
        if !verify_quota_hmac(
            key,
            row.week_start_epoch,
            row.remaining_percentage,
            &row.hmac_signature,
        ) {
            return Err(crate::Error::Engine(
                "HMAC signature verification failed for quota ledger: possible tampering detected"
                    .to_string(),
            ));
        }
        return Ok(row);
    }

    Ok(QuotaLedgerRow {
        week_start_epoch: current_week,
        remaining_percentage: initial_pct,
        last_expansion_at: None,
        hmac_signature: signature,
        is_synced: false,
        updated_at: now_epoch,
    })
}

/// Atomically records a quota depletion, updating remaining percentage, timestamp, and HMAC signature.
pub fn record_quota_depletion(
    conn: &Connection,
    key: &[u8; 32],
    delta_pct: f64,
    now_epoch: i64,
) -> crate::Result<QuotaLedgerRow> {
    let current = get_or_init_quota_ledger(conn, key, now_epoch)?;
    let new_pct = (current.remaining_percentage - delta_pct).max(0.0);
    let new_sig = compute_quota_hmac(key, current.week_start_epoch, new_pct);

    conn.execute(
        "UPDATE quota_ledger
         SET remaining_percentage = ?1,
             last_expansion_at = ?2,
             hmac_signature = ?3,
             is_synced = 0,
             updated_at = ?4
         WHERE week_start_epoch = ?5",
        (
            new_pct,
            now_epoch,
            &new_sig,
            now_epoch,
            current.week_start_epoch,
        ),
    )?;

    Ok(QuotaLedgerRow {
        week_start_epoch: current.week_start_epoch,
        remaining_percentage: new_pct,
        last_expansion_at: Some(now_epoch),
        hmac_signature: new_sig,
        is_synced: false,
        updated_at: now_epoch,
    })
}

/// Sets or updates the quota balance from cloud sync, marking it as synced with updated HMAC signature.
pub fn set_quota_balance_from_cloud(
    conn: &Connection,
    key: &[u8; 32],
    week_epoch: i64,
    remaining_pct: f64,
    now_epoch: i64,
) -> crate::Result<QuotaLedgerRow> {
    let week_start = calculate_week_start_epoch(week_epoch);
    let clamped_pct = remaining_pct.clamp(0.0, 100.0);
    let sig = compute_quota_hmac(key, week_start, clamped_pct);

    conn.execute(
        "INSERT INTO quota_ledger (
             week_start_epoch,
             remaining_percentage,
             last_expansion_at,
             hmac_signature,
             is_synced,
             updated_at
         ) VALUES (?1, ?2, NULL, ?3, 1, ?4)
         ON CONFLICT(week_start_epoch) DO UPDATE SET
             remaining_percentage = excluded.remaining_percentage,
             hmac_signature = excluded.hmac_signature,
             is_synced = 1,
             updated_at = excluded.updated_at",
        (week_start, clamped_pct, &sig, now_epoch),
    )?;

    let row = query_quota_row(conn, week_start)?
        .ok_or_else(|| crate::Error::Database(rusqlite::Error::QueryReturnedNoRows))?;
    Ok(row)
}
