use rusqlite::{Connection, Result};
use tracing::error;

/// Creates the full database schema against an open connection.
///
/// There is no versioning and no upgrade path: every `CREATE` is
/// `IF NOT EXISTS`, so calling this on an existing database is a
/// cheap no-op. Schema changes during alpha mean deleting the local
/// database file and letting it recreate from scratch.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key        TEXT    PRIMARY KEY,
            value      JSON    NOT NULL,
            version    INTEGER DEFAULT 1,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS triggers (
            id           TEXT    PRIMARY KEY,
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
        );",
    )
    .map_err(|e| {
        if super::is_rusqlite_corrupt(&e) {
            error!(error=%e, "Corrupted SQLite database detected during schema setup");
        } else {
            error!(error=%e, "Schema setup failed");
        }
        e
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_stats_table_created() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let table_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='app_stats')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(
            table_exists,
            "app_stats table should be created by ensure_schema"
        );
    }

    #[test]
    fn test_alias_schema_shape() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
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
        assert!(!cols.contains(&"name".to_string()));
    }

    #[test]
    fn test_schema_setup_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();
    }
}
