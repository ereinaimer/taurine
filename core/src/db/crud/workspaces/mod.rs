pub mod types;

#[cfg(test)]
mod tests;

pub use types::WorkspaceRow;

use rusqlite::Connection;

/// Fetches all non-deleted workspaces, with the default workspace first.
pub fn get_workspaces(conn: &Connection) -> crate::Result<Vec<WorkspaceRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, is_default, created_at, updated_at, version, is_deleted, is_synced
         FROM workspaces
         WHERE is_deleted = 0
         ORDER BY is_default DESC, created_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WorkspaceRow {
            id: row.get(0)?,
            name: row.get(1)?,
            is_default: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            version: row.get(5)?,
            is_deleted: row.get(6)?,
            is_synced: row.get(7)?,
        })
    })?;
    let mut list = Vec::new();
    for r in rows {
        list.push(r?);
    }
    Ok(list)
}

/// Fetches the primary default workspace.
pub fn get_default_workspace(conn: &Connection) -> crate::Result<WorkspaceRow> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, is_default, created_at, updated_at, version, is_deleted, is_synced
         FROM workspaces
         WHERE is_default = 1 AND is_deleted = 0
         LIMIT 1",
    )?;
    stmt.query_row([], |row| {
        Ok(WorkspaceRow {
            id: row.get(0)?,
            name: row.get(1)?,
            is_default: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            version: row.get(5)?,
            is_deleted: row.get(6)?,
            is_synced: row.get(7)?,
        })
    })
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => {
            crate::Error::NotFound("Default workspace not found".to_string())
        }
        other => crate::Error::Database(other),
    })
}

/// Retrieves a specific active workspace by its unique identifier.
pub fn get_workspace_by_id(conn: &Connection, id: &str) -> crate::Result<Option<WorkspaceRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, is_default, created_at, updated_at, version, is_deleted, is_synced
         FROM workspaces
         WHERE id = ?1 AND is_deleted = 0",
    )?;
    let result = stmt.query_row([id], |row| {
        Ok(WorkspaceRow {
            id: row.get(0)?,
            name: row.get(1)?,
            is_default: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            version: row.get(5)?,
            is_deleted: row.get(6)?,
            is_synced: row.get(7)?,
        })
    });
    match result {
        Ok(ws) => Ok(Some(ws)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(crate::Error::Database(e)),
    }
}

/// Creates a new non-default workspace.
pub fn create_workspace(conn: &Connection, name: &str) -> crate::Result<WorkspaceRow> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(crate::Error::Config(
            "Workspace name cannot be empty".to_string(),
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let now = crate::db::now_unix_secs();
    conn.execute(
        "INSERT INTO workspaces (id, name, is_default, created_at, updated_at, version, is_deleted, is_synced)
         VALUES (?1, ?2, 0, ?3, ?3, 1, 0, 0)",
        (&id, trimmed, now),
    )?;
    Ok(WorkspaceRow {
        id,
        name: trimmed.to_string(),
        is_default: false,
        created_at: now,
        updated_at: now,
        version: 1,
        is_deleted: false,
        is_synced: false,
    })
}
