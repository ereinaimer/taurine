//! Database migration compatibility bridge.
//!
//! Provides `run_migrations` as a compatibility wrapper around `schema::ensure_schema`
//! so existing test fixtures and callers remain fully functional.

use rusqlite::{Connection, Result};

/// Compatibility wrapper forwarding to `schema::ensure_schema`.
#[inline]
pub fn run_migrations(conn: &Connection) -> Result<()> {
    super::schema::ensure_schema(conn)
}
