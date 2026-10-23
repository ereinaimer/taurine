pub mod types;

#[cfg(test)]
mod tests;

pub use types::UserTier;

use crate::Result;
use crate::db::crud::settings::{get_setting_value, upsert_setting};
use rusqlite::Connection;

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
///
/// Notifies the daemon to hot-reload tier and quota status if the service is running.
pub fn set_user_tier(conn: &Connection, tier: UserTier) -> Result<()> {
    let value_json = serde_json::to_string(&tier)?;
    upsert_setting(conn, SETTING_KEY_USER_TIER, &value_json)?;
    crate::system::rpc::notify_daemon_reload();
    Ok(())
}

/// Toggles a trigger's enabled status.
///
/// Notifies the daemon to hot-reload if the service is running.
pub fn set_trigger_enabled(conn: &Connection, trigger_id: &str, enabled: bool) -> Result<()> {
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
