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

    // Fast exit: nothing to do on the vast majority of startups.
    if version >= CURRENT_SCHEMA_VERSION {
        debug!(current_schema_version = version, "Schema is up to date");
        return Ok(());
    }

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
                    keystrokes_saved INTEGER DEFAULT 0,
                    version          INTEGER DEFAULT 1,
                    updated_at       INTEGER NOT NULL
                );

                -- Partial index: hot-path trigger lookup, tombstoned rows excluded.
                CREATE INDEX IF NOT EXISTS idx_active_triggers
                    ON automations(trigger) WHERE is_deleted = 0;

                -- Sync index: version is the LWW arbiter; updated_at breaks clock-drift ties.
                CREATE INDEX IF NOT EXISTS idx_sync_queue
                    ON automations(version, updated_at, is_synced);

                -- UI index: fuzzy-finder sorts by most-used first.
                CREATE INDEX IF NOT EXISTS idx_automations_usage_count
                    ON automations(usage_count DESC);

                -- Metrics sync: same LWW ordering as automations.
                CREATE INDEX IF NOT EXISTS idx_metrics_sync
                    ON metrics(version, updated_at);

                PRAGMA user_version = 1;",
                )
                .map_err(|e| {
                    error!(error=%e, "Schema migration v0 -> v1 failed");
                    e
                })?,

            // ----------------------------------------------------------------
            // Template for the next migration — copy, fill in, bump version.
            // ----------------------------------------------------------------
            // 1 => conn.execute_batch(
            //     "ALTER TABLE automations ADD COLUMN shortcut TEXT;
            //      PRAGMA user_version = 3;",
            // )?,
            _ => unreachable!("Unhandled migration version {v}"),
        }
    }

    Ok(())
}
