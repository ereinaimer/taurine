use rusqlite::{Connection, Result};
use tracing::{debug, error, info};

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
        info!(
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
                // automations  — trigger rules with sync/tombstone metadata
                // metrics      — daily usage counters
                // ----------------------------------------------------------------
                0 => conn
                    .execute_batch(
                        "CREATE TABLE IF NOT EXISTS settings (
                    key        TEXT    PRIMARY KEY,
                    value      JSON    NOT NULL,
                    version    INTEGER DEFAULT 1,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS automations (
                    id           TEXT    PRIMARY KEY,
                    name         TEXT    NOT NULL,
                    description  TEXT,
                    trigger_type TEXT    NOT NULL DEFAULT 'word',
                    trigger      TEXT    NOT NULL,
                    output       TEXT    NOT NULL,
                    action_type  TEXT    DEFAULT 'text',
                    is_enabled   BOOLEAN DEFAULT 1,
                    target_os    TEXT    DEFAULT 'all',
                    tags         JSON    DEFAULT '[]',
                    usage_count  INTEGER DEFAULT 0,
                    last_used_at INTEGER,
                    created_at   INTEGER NOT NULL,
                    updated_at   INTEGER NOT NULL,
                    version      INTEGER DEFAULT 1,
                    is_deleted   BOOLEAN DEFAULT 0,
                    is_synced    BOOLEAN DEFAULT 1
                );

                CREATE TABLE IF NOT EXISTS metrics (
                    date             TEXT    PRIMARY KEY,
                    executions       INTEGER DEFAULT 0,
                    ai_executions    INTEGER DEFAULT 0,
                    keystrokes_saved INTEGER DEFAULT 0,
                    time_saved_ms    INTEGER DEFAULT 0,
                    version          INTEGER DEFAULT 1,
                    updated_at       INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS scripts (
                    automation_id      TEXT    PRIMARY KEY,
                    interpreter        TEXT    NOT NULL,
                    behavior           TEXT    NOT NULL,
                    compressed_content BLOB    NOT NULL,
                    version            INTEGER DEFAULT 1,
                    updated_at         INTEGER NOT NULL,
                    FOREIGN KEY(automation_id) REFERENCES automations(id) ON DELETE CASCADE
                );

                -- Partial index: hot-path word-trigger lookup, tombstoned rows excluded.
                CREATE INDEX IF NOT EXISTS idx_active_triggers
                    ON automations(trigger_type, trigger)
                 WHERE is_deleted = 0 AND is_enabled = 1;

                -- Exact duplicate guard for active rows only.
                CREATE UNIQUE INDEX IF NOT EXISTS idx_active_trigger_uniqueness
                    ON automations(trigger_type, trigger, target_os)
                 WHERE is_deleted = 0;

                -- Sync index: version is the LWW arbiter; updated_at breaks clock-drift ties.
                CREATE INDEX IF NOT EXISTS idx_sync_queue
                    ON automations(version, updated_at, is_synced);

                -- UI index: fuzzy-finder sorts by most-used first.
                CREATE INDEX IF NOT EXISTS idx_automations_usage_count
                    ON automations(usage_count DESC);

                -- Metrics sync: same LWW ordering as automations.
                CREATE INDEX IF NOT EXISTS idx_metrics_sync
                    ON metrics(version, updated_at);

                CREATE TABLE IF NOT EXISTS ai_presets (
                    name   TEXT PRIMARY KEY,
                    prompt TEXT NOT NULL
                );

                PRAGMA user_version = 1;",
                    )
                    .map_err(|e| {
                        error!(error=%e, "Schema migration v0 -> v1 failed");
                        e
                    })?,

                // ----------------------------------------------------------------
                // Template for the next migration — copy, fill in, bump version.
                // ----------------------------------------------------------------
                // 2 => conn.execute_batch(
                //     \"ALTER TABLE automations ADD COLUMN shortcut TEXT;
                //      PRAGMA user_version = 3;\",
                // )?,
                _ => unreachable!("Unhandled migration version {v}"),
            }
        }
    }

    reconcile_schema_v1(conn)?;
    debug!(
        current_schema_version = CURRENT_SCHEMA_VERSION,
        "Schema is up to date"
    );

    Ok(())
}

fn reconcile_schema_v1(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "automations", "trigger_type")? {
        conn.execute_batch(
            "ALTER TABLE automations
                 ADD COLUMN trigger_type TEXT NOT NULL DEFAULT 'word';",
        )
        .map_err(|e| {
            error!(error=%e, "Failed to reconcile automations.trigger_type into schema v1");
            e
        })?;
    }

    if !column_exists(conn, "metrics", "ai_executions")? {
        conn.execute_batch(
            "ALTER TABLE metrics
                 ADD COLUMN ai_executions INTEGER DEFAULT 0;",
        )
        .map_err(|e| {
            error!(error=%e, "Failed to reconcile metrics.ai_executions into schema v1");
            e
        })?;
    }

    if !column_exists(conn, "metrics", "time_saved_ms")? {
        conn.execute_batch(
            "ALTER TABLE metrics
                 ADD COLUMN time_saved_ms INTEGER DEFAULT 0;",
        )
        .map_err(|e| {
            error!(error=%e, "Failed to reconcile metrics.time_saved_ms into schema v1");
            e
        })?;
    }

    validate_active_trigger_type_values(conn)?;
    validate_no_exact_active_duplicates(conn)?;

    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_active_triggers;
         CREATE INDEX IF NOT EXISTS idx_active_triggers
             ON automations(trigger_type, trigger)
          WHERE is_deleted = 0 AND is_enabled = 1;
         CREATE UNIQUE INDEX IF NOT EXISTS idx_active_trigger_uniqueness
             ON automations(trigger_type, trigger, target_os)
          WHERE is_deleted = 0;",
    )
    .map_err(|e| {
        error!(error=%e, "Failed to reconcile schema v1 indexes for trigger types");
        e
    })?;

    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&pragma)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;

    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }

    Ok(false)
}

fn validate_active_trigger_type_values(conn: &Connection) -> Result<()> {
    let invalid: Option<String> = conn
        .query_row(
            "SELECT trigger_type
             FROM automations
             WHERE trigger_type NOT IN ('word', 'hotkey')
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    if let Some(trigger_type) = invalid {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "Invalid trigger_type found in automations table: {trigger_type}"
        )));
    }

    Ok(())
}

fn validate_no_exact_active_duplicates(conn: &Connection) -> Result<()> {
    let duplicate: Option<(String, String, String, i64)> = conn
        .query_row(
            "SELECT trigger_type, trigger, target_os, COUNT(*)
             FROM automations
             WHERE is_deleted = 0
             GROUP BY trigger_type, trigger, target_os
             HAVING COUNT(*) > 1
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .ok();

    if let Some((trigger_type, trigger, target_os, count)) = duplicate {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "Cannot reconcile schema v1 with {count} active duplicate automation(s) for {trigger_type}:{trigger}@{target_os}"
        )));
    }

    Ok(())
}
