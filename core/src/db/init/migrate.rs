use rusqlite::{Connection, Result};
use tracing::{debug, error};

/// Runs all pending schema migrations against an open connection.
///
/// # How it works
///
/// `PRAGMA user_version` is a free integer baked into every SQLite file's
/// header. We use it as a schema version counter:
///
/// - `0` → brand-new or pre-migration database
/// - `N` → all migrations up to N have been applied
///
/// On each startup this function reads that integer, then runs every
/// migration whose number is higher, in order. Already-applied migrations
/// are skipped instantly — zero cost on the happy path.
///
/// The `PRAGMA user_version = N` statement that stamps the new version is
/// included inside the same `execute_batch` as the DDL, so a failed
/// migration never leaves a partial stamp behind.
///
/// # How to add a new migration
///
/// 1. Add a new `match` arm for the next version number.
/// 2. Write idempotent DDL (`ALTER TABLE`, `CREATE INDEX IF NOT EXISTS`, …).
/// 3. End the batch with `PRAGMA user_version = <new_version>;`.
/// 4. Bump `CURRENT_SCHEMA_VERSION` by one.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    // Bump this whenever you add a new match arm below.
    const CURRENT_SCHEMA_VERSION: u32 = 1;

    // Read the stamp baked into the file header (0 for a fresh database).
    let version: u32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| {
            error!(error=%e, "Failed to read schema version (PRAGMA user_version)");
            e
        })?;

    if version < CURRENT_SCHEMA_VERSION {
        debug!(
            from_schema_version = version,
            to_schema_version = CURRENT_SCHEMA_VERSION,
            "Running database migrations"
        );

        // Walk every missing migration in order.
        for v in version..CURRENT_SCHEMA_VERSION {
            match v {
                // ----------------------------------------------------------------
                // v0 → v1 : Initial production schema
                // ----------------------------------------------------------------
                // settings     — domain-keyed JSON config store
                // triggers     — trigger rules with sync/tombstone metadata
                // stats        — daily usage counters
                // ----------------------------------------------------------------
                0 => conn
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS settings (
                    key        TEXT    PRIMARY KEY,
                    value      JSON    NOT NULL,
                    version    INTEGER DEFAULT 1,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS triggers (
                    id           TEXT    PRIMARY KEY,
                    name         TEXT    NOT NULL,
                    description  TEXT,
                    trigger_type TEXT    NOT NULL DEFAULT 'word',
                    trigger      TEXT    NOT NULL,
                    output       TEXT    NOT NULL,
                    action_type  TEXT    DEFAULT 'text',
                    is_enabled   BOOLEAN DEFAULT 1,
                    auto_case    BOOLEAN DEFAULT 0,
                    target_os    TEXT    DEFAULT 'all',
                    only_apps    TEXT,
                    except_apps  TEXT,
                    tags         JSON    DEFAULT '[]',
                    usage_count  INTEGER DEFAULT 0,
                    last_used_at INTEGER,
                    created_at   INTEGER NOT NULL,
                    updated_at   INTEGER NOT NULL,
                    version      INTEGER DEFAULT 1,
                    is_deleted   BOOLEAN DEFAULT 0,
                    is_synced    BOOLEAN DEFAULT 1
                );

                CREATE TABLE IF NOT EXISTS stats (
                    date             TEXT    PRIMARY KEY,
                    executions       INTEGER DEFAULT 0,
                    ai_executions    INTEGER DEFAULT 0,
                    keystrokes_saved INTEGER DEFAULT 0,
                    time_saved_ms    INTEGER DEFAULT 0,
                    version          INTEGER DEFAULT 1,
                    updated_at       INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS scripts (
                    trigger_id         TEXT    PRIMARY KEY,
                    interpreter        TEXT    NOT NULL,
                    behavior           TEXT    NOT NULL,
                    compressed_content BLOB    NOT NULL,
                    version            INTEGER DEFAULT 1,
                    updated_at         INTEGER NOT NULL,
                    FOREIGN KEY(trigger_id) REFERENCES triggers(id) ON DELETE CASCADE
                );

                CREATE TABLE IF NOT EXISTS assets (
                    id                 TEXT    PRIMARY KEY,
                    trigger_id         TEXT    NOT NULL,
                    mime_type          TEXT    NOT NULL,
                    compressed_content BLOB    NOT NULL,
                    updated_at         INTEGER NOT NULL,
                    FOREIGN KEY(trigger_id) REFERENCES triggers(id) ON DELETE CASCADE
                );

                -- Partial index: hot-path word-trigger lookup, tombstoned rows excluded.
                CREATE INDEX IF NOT EXISTS idx_active_triggers
                    ON triggers(trigger_type, trigger)
                 WHERE is_deleted = 0 AND is_enabled = 1;

                -- Sync index: version is the LWW arbiter; updated_at breaks clock-drift ties.
                CREATE INDEX IF NOT EXISTS idx_sync_queue
                    ON triggers(version, updated_at, is_synced);

                -- UI index: fuzzy-finder sorts by most-used first.
                CREATE INDEX IF NOT EXISTS idx_triggers_usage_count
                    ON triggers(usage_count DESC);

                -- Stats sync: same LWW ordering as triggers.
                CREATE INDEX IF NOT EXISTS idx_stats_sync
                    ON stats(version, updated_at);

                CREATE TABLE IF NOT EXISTS ai_presets (
                    name   TEXT PRIMARY KEY,
                    prompt TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS app_stats (
                    app_key          TEXT    NOT NULL,
                    date             TEXT    NOT NULL,
                    executions       INTEGER NOT NULL DEFAULT 0,
                    keystrokes_saved INTEGER NOT NULL DEFAULT 0,
                    time_saved_ms    INTEGER NOT NULL DEFAULT 0,
                    version          INTEGER NOT NULL DEFAULT 1,
                    updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                    PRIMARY KEY (app_key, date)
                );

                DROP INDEX IF EXISTS idx_active_trigger_uniqueness;

                CREATE UNIQUE INDEX IF NOT EXISTS idx_active_trigger_uniqueness
                    ON triggers(trigger_type, trigger, target_os, COALESCE(only_apps, ''), COALESCE(except_apps, ''))
                 WHERE is_deleted = 0;

                PRAGMA user_version = 1;",
                    )
                    .map_err(|e| {
                        error!(error=%e, "Schema migration v0 -> v1 failed");
                        e
                    })?,

                _ => {
                    error!(version = v, "Unhandled schema migration version");
                    return Err(rusqlite::Error::InvalidQuery);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_stats_table_created() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let table_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='app_stats')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(
            table_exists,
            "app_stats table should be created by run_migrations"
        );
    }
}
