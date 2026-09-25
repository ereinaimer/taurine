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
            if super::is_rusqlite_corrupt(&e) {
                error!(error=%e, "Corrupted SQLite database detected during schema version check (PRAGMA user_version)");
            } else {
                error!(error=%e, "Failed to read schema version (PRAGMA user_version)");
            }
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
                    voice_executions INTEGER DEFAULT 0,
                    words_dictated   INTEGER DEFAULT 0,
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

                CREATE TABLE IF NOT EXISTS trigger_aliases (
                    id                   TEXT PRIMARY KEY,
                    trigger_id           TEXT NOT NULL REFERENCES triggers(id) ON DELETE CASCADE,
                    invocation           TEXT NOT NULL,
                    invocation_type      TEXT NOT NULL DEFAULT 'word',
                    require_confirmation INTEGER NOT NULL DEFAULT 0,
                    strict_threshold     REAL,
                    created_at           INTEGER NOT NULL DEFAULT (unixepoch())
                );

                DROP INDEX IF EXISTS idx_alias_uniqueness;

                CREATE INDEX IF NOT EXISTS idx_alias_lookup
                    ON trigger_aliases(invocation_type, invocation);

                CREATE INDEX IF NOT EXISTS idx_alias_by_trigger
                    ON trigger_aliases(trigger_id);

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

                PRAGMA user_version = 1;",
                    )
                    .map_err(|e| {
                        if super::is_rusqlite_corrupt(&e) {
                            error!(
                                error = %e,
                                "Corrupted SQLite database detected during schema migration v0 -> v1"
                            );
                        } else {
                            error!(error = %e, "Schema migration v0 -> v1 failed");
                        }
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

    #[test]
    fn test_alias_schema_shape() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let has: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='trigger_aliases')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(has, "trigger_aliases table should exist");
        let gone: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='voice_triggers')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!gone, "voice_triggers table should be gone");
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(triggers)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!(!cols.contains(&"trigger".to_string()));
        assert!(!cols.contains(&"trigger_type".to_string()));
    }
}
