use rusqlite::{Connection, Result};
use tracing::error;

/// Creates the full database schema against an open connection.
///
/// There is no versioning and no upgrade path: every `CREATE` is
/// `IF NOT EXISTS`, so calling this on an existing database is a
/// cheap no-op. Schema changes during alpha mean deleting the local
/// database file and letting it recreate from scratch.
pub fn ensure_schema(conn: &Connection) -> Result<()> {
    migrate_legacy_columns(conn)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
            key        TEXT    PRIMARY KEY,
            value      JSON    NOT NULL,
            version    INTEGER DEFAULT 1,
            updated_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workspaces (
            id           TEXT    PRIMARY KEY,
            name         TEXT    NOT NULL,
            is_default   BOOLEAN NOT NULL DEFAULT 0,
            created_at   INTEGER NOT NULL,
            updated_at   INTEGER NOT NULL,
            version      INTEGER NOT NULL DEFAULT 1,
            is_deleted   BOOLEAN NOT NULL DEFAULT 0
        );

        INSERT OR IGNORE INTO workspaces (id, name, is_default, created_at, updated_at, version, is_deleted)
        VALUES ('default', 'Personal', 1, unixepoch(), unixepoch(), 1, 0);

        CREATE TABLE IF NOT EXISTS quota_ledger (
            week_start_epoch     INTEGER PRIMARY KEY,
            remaining_percentage REAL    NOT NULL DEFAULT 100.0,
            last_expansion_at    INTEGER,
            hmac_signature       TEXT    NOT NULL DEFAULT '',
            updated_at           INTEGER NOT NULL
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
            workspace_id TEXT    NOT NULL DEFAULT 'default' REFERENCES workspaces(id)
        );

        CREATE TABLE IF NOT EXISTS stats (
            device_id        TEXT    NOT NULL DEFAULT '',
            date             TEXT    NOT NULL,
            executions       INTEGER DEFAULT 0,
            ai_executions    INTEGER DEFAULT 0,
            voice_executions INTEGER DEFAULT 0,
            words_spoken     INTEGER DEFAULT 0,
            words_expanded   INTEGER DEFAULT 0,
            time_saved_ms    INTEGER DEFAULT 0,
            version          INTEGER DEFAULT 1,
            updated_at       INTEGER NOT NULL,
            PRIMARY KEY (device_id, date)
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
            ON triggers(version, updated_at);

        -- UI index: fuzzy-finder sorts by most-used first.
        CREATE INDEX IF NOT EXISTS idx_triggers_usage_count
            ON triggers(usage_count DESC);

        CREATE INDEX IF NOT EXISTS idx_triggers_workspace
            ON triggers(workspace_id);

        -- Stats sync: same LWW ordering as triggers.
        CREATE INDEX IF NOT EXISTS idx_stats_sync
            ON stats(version, updated_at);

        CREATE TABLE IF NOT EXISTS ai_presets (
            name   TEXT PRIMARY KEY,
            prompt TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS app_stats (
            device_id        TEXT    NOT NULL DEFAULT '',
            app_key          TEXT    NOT NULL,
            date             TEXT    NOT NULL,
            executions       INTEGER NOT NULL DEFAULT 0,
            words_spoken     INTEGER NOT NULL DEFAULT 0,
            words_expanded   INTEGER NOT NULL DEFAULT 0,
            time_saved_ms    INTEGER NOT NULL DEFAULT 0,
            version          INTEGER NOT NULL DEFAULT 1,
            updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (device_id, app_key, date)
        );

        CREATE TABLE IF NOT EXISTS trigger_daily_stats (
            device_id        TEXT    NOT NULL DEFAULT '',
            trigger_id       TEXT    NOT NULL,
            date             TEXT    NOT NULL,
            app_key          TEXT    NOT NULL DEFAULT '',
            executions       INTEGER NOT NULL DEFAULT 0,
            words_spoken     INTEGER NOT NULL DEFAULT 0,
            words_expanded   INTEGER NOT NULL DEFAULT 0,
            time_saved_ms    INTEGER NOT NULL DEFAULT 0,
            version          INTEGER NOT NULL DEFAULT 1,
            updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),
            PRIMARY KEY (device_id, trigger_id, date, app_key)
        );

        CREATE INDEX IF NOT EXISTS idx_trigger_daily_stats_sync
            ON trigger_daily_stats(version, updated_at);

        CREATE TABLE IF NOT EXISTS activity_events (
            id            TEXT    PRIMARY KEY,
            trigger_id    TEXT,
            timestamp     INTEGER NOT NULL,
            date          TEXT    NOT NULL,
            category      TEXT    NOT NULL,
            app_key       TEXT    NOT NULL DEFAULT '',
            word_count    INTEGER NOT NULL DEFAULT 0,
            time_saved_ms INTEGER NOT NULL DEFAULT 0,
            content       TEXT,
            version       INTEGER NOT NULL DEFAULT 1,
            created_at    INTEGER NOT NULL DEFAULT (unixepoch())
        );

        CREATE INDEX IF NOT EXISTS idx_activity_events_date
            ON activity_events(date);

        CREATE INDEX IF NOT EXISTS idx_activity_events_timestamp
            ON activity_events(timestamp DESC);

        CREATE INDEX IF NOT EXISTS idx_activity_events_sync
            ON activity_events(version);

        CREATE INDEX IF NOT EXISTS idx_activity_events_trigger
            ON activity_events(trigger_id);

        CREATE TABLE IF NOT EXISTS sync_outbox (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            entity_type TEXT    NOT NULL,
            entity_id   TEXT    NOT NULL,
            operation   TEXT    NOT NULL,
            payload     JSON    NOT NULL,
            created_at  INTEGER NOT NULL DEFAULT (unixepoch()),
            retry_count INTEGER NOT NULL DEFAULT 0,
            last_error  TEXT
        );

        CREATE TABLE IF NOT EXISTS sync_outbox_quarantine (
            id             INTEGER PRIMARY KEY,
            entity_type    TEXT    NOT NULL,
            entity_id      TEXT    NOT NULL,
            operation      TEXT    NOT NULL,
            payload        JSON    NOT NULL,
            retry_count    INTEGER NOT NULL,
            last_error     TEXT,
            quarantined_at INTEGER NOT NULL DEFAULT (unixepoch())
        );

        CREATE INDEX IF NOT EXISTS idx_sync_outbox_order
            ON sync_outbox(retry_count ASC, id ASC);

        CREATE TABLE IF NOT EXISTS sync_checkpoints (
            entity_type    TEXT PRIMARY KEY,
            last_synced_at INTEGER NOT NULL DEFAULT 0
        );",
    )
    .map_err(|e| {
        if super::is_rusqlite_corrupt(&e) {
            error!(error=%e, "Corrupted SQLite database detected during schema setup");
        } else {
            error!(error=%e, "Schema setup failed");
        }
        e
    })?;

    migrate_legacy_columns(conn)?;

    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1)"),
        [column],
        |row| row.get(0),
    )
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get(0),
    )
}

/// Inspects existing tables for newly introduced columns and adds them if absent.
pub fn migrate_legacy_columns(conn: &Connection) -> Result<()> {
    if table_exists(conn, "activity_events")? && !column_exists(conn, "activity_events", "content")?
    {
        conn.execute("ALTER TABLE activity_events ADD COLUMN content TEXT;", [])?;
    }

    if table_exists(conn, "trigger_daily_stats")?
        && !column_exists(conn, "trigger_daily_stats", "words_spoken")?
    {
        conn.execute(
            "ALTER TABLE trigger_daily_stats ADD COLUMN words_spoken INTEGER NOT NULL DEFAULT 0;",
            [],
        )?;
    }

    if table_exists(conn, "triggers")? && !column_exists(conn, "triggers", "workspace_id")? {
        conn.execute(
            "ALTER TABLE triggers ADD COLUMN workspace_id TEXT NOT NULL DEFAULT 'default';",
            [],
        )?;
    }

    if table_exists(conn, "stats")? && !column_exists(conn, "stats", "device_id")? {
        let device_id = crate::cloud::get_device_hardware_id();
        conn.execute_batch(
            "CREATE TABLE stats_migrating (
                device_id        TEXT    NOT NULL DEFAULT '',
                date             TEXT    NOT NULL,
                executions       INTEGER DEFAULT 0,
                ai_executions    INTEGER DEFAULT 0,
                voice_executions INTEGER DEFAULT 0,
                words_spoken     INTEGER DEFAULT 0,
                words_expanded   INTEGER DEFAULT 0,
                time_saved_ms    INTEGER DEFAULT 0,
                version          INTEGER DEFAULT 1,
                updated_at       INTEGER NOT NULL,
                PRIMARY KEY (device_id, date)
            );",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO stats_migrating (
                device_id, date, executions, ai_executions, voice_executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
            )
            SELECT ?1 AS device_id, date, executions, ai_executions, voice_executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
            FROM stats;",
            rusqlite::params![device_id],
        )?;
        conn.execute_batch(
            "DROP TABLE stats;
             ALTER TABLE stats_migrating RENAME TO stats;
             CREATE INDEX IF NOT EXISTS idx_stats_sync ON stats(version, updated_at);",
        )?;
    }

    if table_exists(conn, "app_stats")? && !column_exists(conn, "app_stats", "device_id")? {
        let device_id = crate::cloud::get_device_hardware_id();
        conn.execute_batch(
            "CREATE TABLE app_stats_migrating (
                device_id        TEXT    NOT NULL DEFAULT '',
                app_key          TEXT    NOT NULL,
                date             TEXT    NOT NULL,
                executions       INTEGER NOT NULL DEFAULT 0,
                words_spoken     INTEGER NOT NULL DEFAULT 0,
                words_expanded   INTEGER NOT NULL DEFAULT 0,
                time_saved_ms    INTEGER NOT NULL DEFAULT 0,
                version          INTEGER NOT NULL DEFAULT 1,
                updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (device_id, app_key, date)
            );",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO app_stats_migrating (
                device_id, app_key, date, executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
            )
            SELECT ?1 AS device_id, app_key, date, executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
            FROM app_stats;",
            rusqlite::params![device_id],
        )?;
        conn.execute_batch(
            "DROP TABLE app_stats;
             ALTER TABLE app_stats_migrating RENAME TO app_stats;",
        )?;
    }

    if table_exists(conn, "trigger_daily_stats")?
        && !column_exists(conn, "trigger_daily_stats", "device_id")?
    {
        let device_id = crate::cloud::get_device_hardware_id();
        conn.execute_batch(
            "CREATE TABLE trigger_daily_stats_migrating (
                device_id        TEXT    NOT NULL DEFAULT '',
                trigger_id       TEXT    NOT NULL,
                date             TEXT    NOT NULL,
                app_key          TEXT    NOT NULL DEFAULT '',
                executions       INTEGER NOT NULL DEFAULT 0,
                words_spoken     INTEGER NOT NULL DEFAULT 0,
                words_expanded   INTEGER NOT NULL DEFAULT 0,
                time_saved_ms    INTEGER NOT NULL DEFAULT 0,
                version          INTEGER NOT NULL DEFAULT 1,
                updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (device_id, trigger_id, date, app_key)
            );",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO trigger_daily_stats_migrating (
                device_id, trigger_id, date, app_key, executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
            )
            SELECT ?1 AS device_id, trigger_id, date, app_key, executions, words_spoken, words_expanded, time_saved_ms, version, updated_at
            FROM trigger_daily_stats;",
            rusqlite::params![device_id],
        )?;
        conn.execute_batch(
            "DROP TABLE trigger_daily_stats;
             ALTER TABLE trigger_daily_stats_migrating RENAME TO trigger_daily_stats;
             CREATE INDEX IF NOT EXISTS idx_trigger_daily_stats_sync ON trigger_daily_stats(version, updated_at);",
        )?;
    }

    if table_exists(conn, "sync_outbox")? && !column_exists(conn, "sync_outbox", "retry_count")? {
        conn.execute(
            "ALTER TABLE sync_outbox ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;",
            [],
        )?;
    }

    if table_exists(conn, "sync_outbox")? && !column_exists(conn, "sync_outbox", "last_error")? {
        conn.execute("ALTER TABLE sync_outbox ADD COLUMN last_error TEXT;", [])?;
    }

    Ok(())
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
    fn test_activity_events_table_created() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let table_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='activity_events')",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(
            table_exists,
            "activity_events table should be created by ensure_schema"
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('activity_events') WHERE name IN ('id', 'trigger_id', 'timestamp', 'date', 'category', 'app_key', 'word_count', 'time_saved_ms', 'content', 'version', 'created_at')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 11, "activity_events must contain all 11 columns");
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
    fn test_sync_outbox_and_pruned_schema() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();

        let trigger_daily_stats_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='trigger_daily_stats')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            trigger_daily_stats_exists,
            "trigger_daily_stats table must be created"
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('trigger_daily_stats') WHERE name IN ('device_id', 'trigger_id', 'date', 'app_key', 'executions', 'words_spoken', 'words_expanded', 'time_saved_ms', 'version', 'updated_at')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 10,
            "trigger_daily_stats must contain all 10 expected columns"
        );

        let outbox_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sync_outbox')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(outbox_exists, "sync_outbox table must exist");

        let checkpoints_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sync_checkpoints')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(checkpoints_exists, "sync_checkpoints table must exist");

        for table in &[
            "settings",
            "workspaces",
            "quota_ledger",
            "triggers",
            "stats",
            "app_stats",
            "trigger_daily_stats",
            "activity_events",
        ] {
            let has_is_synced: bool = conn
                .query_row(
                    &format!(
                        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name='is_synced')"
                    ),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                !has_is_synced,
                "table {table} must NOT have is_synced column"
            );
        }
    }

    #[test]
    fn test_schema_setup_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap();
    }

    #[test]
    fn test_schema_auto_migrates_missing_columns() {
        let conn = Connection::open_in_memory().unwrap();

        // Create tables without the newly introduced columns:
        // - activity_events without content
        // - trigger_daily_stats without words_spoken and without device_id
        // - stats without device_id
        // - app_stats without device_id
        // - triggers without workspace_id
        conn.execute_batch(
            "CREATE TABLE stats (
                date             TEXT    PRIMARY KEY,
                executions       INTEGER DEFAULT 0,
                ai_executions    INTEGER DEFAULT 0,
                voice_executions INTEGER DEFAULT 0,
                words_spoken     INTEGER DEFAULT 0,
                words_expanded   INTEGER DEFAULT 0,
                time_saved_ms    INTEGER DEFAULT 0,
                version          INTEGER DEFAULT 1,
                updated_at       INTEGER NOT NULL
            );

            CREATE TABLE app_stats (
                app_key          TEXT    NOT NULL,
                date             TEXT    NOT NULL,
                executions       INTEGER NOT NULL DEFAULT 0,
                words_spoken     INTEGER NOT NULL DEFAULT 0,
                words_expanded   INTEGER NOT NULL DEFAULT 0,
                time_saved_ms    INTEGER NOT NULL DEFAULT 0,
                version          INTEGER NOT NULL DEFAULT 1,
                updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (app_key, date)
            );

            CREATE TABLE activity_events (
                id            TEXT    PRIMARY KEY,
                trigger_id    TEXT,
                timestamp     INTEGER NOT NULL,
                date          TEXT    NOT NULL,
                category      TEXT    NOT NULL,
                app_key       TEXT    NOT NULL DEFAULT '',
                word_count    INTEGER NOT NULL DEFAULT 0,
                time_saved_ms INTEGER NOT NULL DEFAULT 0,
                version       INTEGER NOT NULL DEFAULT 1,
                created_at    INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE trigger_daily_stats (
                trigger_id       TEXT    NOT NULL,
                date             TEXT    NOT NULL,
                app_key          TEXT    NOT NULL DEFAULT '',
                executions       INTEGER NOT NULL DEFAULT 0,
                words_expanded   INTEGER NOT NULL DEFAULT 0,
                time_saved_ms    INTEGER NOT NULL DEFAULT 0,
                version          INTEGER NOT NULL DEFAULT 1,
                updated_at       INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (trigger_id, date, app_key)
            );

            CREATE TABLE triggers (
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
                is_deleted   BOOLEAN DEFAULT 0
            );",
        )
        .unwrap();

        let has_content_before: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('activity_events') WHERE name='content')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!has_content_before);

        let has_words_spoken_before: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('trigger_daily_stats') WHERE name='words_spoken')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!has_words_spoken_before);

        let has_workspace_id_before: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('triggers') WHERE name='workspace_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!has_workspace_id_before);

        // Run ensure_schema to heal and auto-migrate missing columns
        ensure_schema(&conn).unwrap();

        let has_content_after: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('activity_events') WHERE name='content')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            has_content_after,
            "content column should be migrated onto activity_events"
        );

        let has_words_spoken_after: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('trigger_daily_stats') WHERE name='words_spoken')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            has_words_spoken_after,
            "words_spoken column should be migrated onto trigger_daily_stats"
        );

        let has_workspace_id_after: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('triggers') WHERE name='workspace_id')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            has_workspace_id_after,
            "workspace_id column should be migrated onto triggers"
        );

        assert!(
            column_exists(&conn, "stats", "device_id").unwrap(),
            "device_id column should be migrated onto stats"
        );
        assert!(
            column_exists(&conn, "app_stats", "device_id").unwrap(),
            "device_id column should be migrated onto app_stats"
        );
        assert!(
            column_exists(&conn, "trigger_daily_stats", "device_id").unwrap(),
            "device_id column should be migrated onto trigger_daily_stats"
        );

        // Verify default values and subsequent operations work correctly
        conn.execute(
            "INSERT INTO triggers (id, output, created_at, updated_at) VALUES ('t1', 'out', 0, 0)",
            [],
        )
        .unwrap();
        let ws: String = conn
            .query_row(
                "SELECT workspace_id FROM triggers WHERE id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ws, "default");

        conn.execute(
            "INSERT INTO trigger_daily_stats (trigger_id, date) VALUES ('t1', '2026-10-31')",
            [],
        )
        .unwrap();
        let spoken: i64 = conn
            .query_row(
                "SELECT words_spoken FROM trigger_daily_stats WHERE trigger_id = 't1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(spoken, 0);

        // Verify idempotence
        ensure_schema(&conn).unwrap();
    }
}
