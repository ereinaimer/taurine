pub mod types;

#[cfg(test)]
mod tests;

pub use types::UserTier;

use crate::Result;
use crate::db::crud::settings::{get_setting_value, upsert_setting};
use rusqlite::Connection;

pub const FREE_TIER_MAX_SNIPPETS: usize = 30;
pub const FREE_TIER_MAX_WORKSPACES: usize = 1;

const SETTING_KEY_USER_TIER: &str = "user_tier";

/// Retrieves the current user tier from the local database settings.
/// Defaults to `UserTier::Free` if no tier is explicitly configured or if the value cannot be parsed.
pub fn get_user_tier(conn: &Connection) -> UserTier {
    let Ok(Some(raw)) = get_setting_value(conn, SETTING_KEY_USER_TIER) else {
        return UserTier::Free;
    };

    serde_json::from_str::<UserTier>(&raw)
        .or_else(|_| {
            let trimmed = raw.trim().trim_matches('"');
            match trimmed.to_lowercase().as_str() {
                "free" => Ok(UserTier::Free),
                "pro" => Ok(UserTier::Pro),
                "max" => Ok(UserTier::Max),
                "team" => Ok(UserTier::Team),
                _ => Err(()),
            }
        })
        .unwrap_or_default()
}

/// Sets the user tier in the local database settings.
/// If setting to Free tier, automatically enforces the 30-snippet cap on active snippets.
pub fn set_user_tier(conn: &Connection, tier: UserTier) -> Result<()> {
    let value_json = serde_json::to_string(&tier)?;
    upsert_setting(conn, SETTING_KEY_USER_TIER, &value_json)?;
    if tier.is_free() {
        enforce_free_tier_snippet_cap(conn)?;
    }
    Ok(())
}

/// Checks whether creating a new snippet is allowed under the current tier limit.
///
/// On Free tier, returns `Error::QuotaExceeded` if there are already 30 or more active enabled
/// (`is_deleted = 0 AND is_enabled = 1`) snippets.
/// On Pro, Max, and Team tiers, snippet creation is unlimited.
pub fn check_snippet_creation_allowed(conn: &Connection) -> Result<()> {
    let tier = get_user_tier(conn);
    if tier.is_unlimited() {
        return Ok(());
    }

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0 AND is_enabled = 1",
        [],
        |row| row.get(0),
    )?;

    if count as usize >= FREE_TIER_MAX_SNIPPETS {
        return Err(crate::Error::QuotaExceeded(format!(
            "Free tier is limited to {FREE_TIER_MAX_SNIPPETS} snippets"
        )));
    }

    Ok(())
}

/// Checks whether enabling a specific snippet is allowed under the Free tier limit.
///
/// Counts currently enabled snippets excluding `trigger_id`. If already at or above 30,
/// returns `Error::QuotaExceeded`.
pub fn check_snippet_enable_allowed(conn: &Connection, trigger_id: &str) -> Result<()> {
    let tier = get_user_tier(conn);
    if tier.is_unlimited() {
        return Ok(());
    }

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0 AND is_enabled = 1 AND id != ?1",
        [trigger_id],
        |row| row.get(0),
    )?;

    if count as usize >= FREE_TIER_MAX_SNIPPETS {
        return Err(crate::Error::QuotaExceeded(format!(
            "Free tier is limited to {FREE_TIER_MAX_SNIPPETS} active snippets. Disable another snippet first or upgrade to Taurine Pro."
        )));
    }

    Ok(())
}

/// Enforces the Free tier maximum enabled snippet limit (30) by automatically pausing
/// excess snippets.
///
/// Keeps the top 30 most-used snippets enabled (`is_enabled = 1`) sorted by `usage_count DESC, created_at ASC, id ASC`.
/// Any excess snippets beyond 30 are marked `is_enabled = 0`.
///
/// This acts as a one-way valve: it only pauses excess snippets if currently enabled > 30.
/// It NEVER auto-enables any snippet that was already disabled.
/// Returns the number of snippets that were paused.
pub fn enforce_free_tier_snippet_cap(conn: &Connection) -> Result<usize> {
    let tier = get_user_tier(conn);
    if tier.is_unlimited() {
        return Ok(0);
    }

    let mut stmt = conn.prepare(
        "SELECT id FROM triggers 
         WHERE is_deleted = 0 AND is_enabled = 1 
         ORDER BY usage_count DESC, created_at ASC, id ASC",
    )?;

    let enabled_ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;

    if enabled_ids.len() <= FREE_TIER_MAX_SNIPPETS {
        return Ok(0);
    }

    let excess_ids = &enabled_ids[FREE_TIER_MAX_SNIPPETS..];
    let paused_count = excess_ids.len();

    for id in excess_ids {
        conn.execute(
            "UPDATE triggers SET is_enabled = 0, is_synced = 0, updated_at = unixepoch() WHERE id = ?1",
            [id],
        )?;
    }

    tracing::info!(
        paused_count,
        "Enforced Free tier limit: paused excess snippets"
    );

    Ok(paused_count)
}

/// Toggles a trigger's enabled status.
///
/// If `enabled` is true, verifies that the user tier allows enabling the snippet
/// (enforcing the 30 active snippets limit on Free tier).
/// Notifies the daemon to hot-reload if the service is running.
pub fn set_trigger_enabled(conn: &Connection, trigger_id: &str, enabled: bool) -> Result<()> {
    if enabled {
        check_snippet_enable_allowed(conn, trigger_id)?;
    }

    let updated = conn.execute(
        "UPDATE triggers 
         SET is_enabled = ?1, is_synced = 0, updated_at = unixepoch() 
         WHERE id = ?2 AND is_deleted = 0",
        rusqlite::params![if enabled { 1 } else { 0 }, trigger_id],
    )?;

    if updated == 0 {
        return Err(crate::Error::NotFound(format!(
            "Trigger '{trigger_id}' not found"
        )));
    }

    crate::system::rpc::notify_daemon_reload();
    Ok(())
}

/// Checks whether creating a new workspace is allowed under the current tier limit.
///
/// On Free tier, returns `Error::QuotaExceeded` if there is already 1 or more active
/// (`is_deleted = 0`) workspace.
/// On Pro, Max, and Team tiers, workspace creation is unlimited.
pub fn check_workspace_creation_allowed(conn: &Connection) -> Result<()> {
    let tier = get_user_tier(conn);
    if tier.is_unlimited() {
        return Ok(());
    }

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workspaces WHERE is_deleted = 0",
        [],
        |row| row.get(0),
    )?;

    if count as usize >= FREE_TIER_MAX_WORKSPACES {
        return Err(crate::Error::QuotaExceeded(format!(
            "Free tier is limited to {FREE_TIER_MAX_WORKSPACES} workspace"
        )));
    }

    Ok(())
}
