pub mod types;

#[cfg(test)]
mod tests;

pub use types::WorkspaceRow;

use rusqlite::Connection;

/// Fetches all non-deleted workspaces, with the default workspace first.
pub fn get_workspaces(conn: &Connection) -> crate::Result<Vec<WorkspaceRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, is_default, created_at, updated_at, version, is_deleted, 0 AS is_synced
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
        "SELECT id, name, is_default, created_at, updated_at, version, is_deleted, 0 AS is_synced
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
        "SELECT id, name, is_default, created_at, updated_at, version, is_deleted, 0 AS is_synced
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
        "INSERT INTO workspaces (id, name, is_default, created_at, updated_at, version, is_deleted)
         VALUES (?1, ?2, 0, ?3, ?3, 1, 0)",
        (&id, trimmed, now),
    )?;

    sync_workspace_map(conn)?;

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

/// Soft-deletes a workspace by ID or name and soft-deletes its associated triggers.
pub fn delete_workspace(conn: &Connection, id_or_name: &str) -> crate::Result<()> {
    let ws = get_workspaces(conn)?
        .into_iter()
        .find(|w| w.id == id_or_name || w.name.eq_ignore_ascii_case(id_or_name))
        .ok_or_else(|| crate::Error::NotFound(format!("Workspace '{id_or_name}' not found")))?;

    if ws.is_default || ws.id == "default" {
        return Err(crate::Error::Config(
            "Cannot delete the default workspace".to_string(),
        ));
    }

    // Collect all active triggers belonging to this workspace to enqueue outbox tombstones
    let mut stmt =
        conn.prepare("SELECT id FROM triggers WHERE workspace_id = ?1 AND is_deleted = 0")?;
    let trigger_ids: Vec<String> = stmt
        .query_map([&ws.id], |row| row.get(0))?
        .filter_map(std::result::Result::ok)
        .collect();

    let now = crate::db::now_unix_secs();
    conn.execute(
        "UPDATE workspaces SET is_deleted = 1, updated_at = ?1, version = version + 1 WHERE id = ?2",
        rusqlite::params![now, ws.id],
    )?;
    conn.execute(
        "UPDATE triggers SET is_deleted = 1, updated_at = ?1, version = version + 1 WHERE workspace_id = ?2",
        rusqlite::params![now, ws.id],
    )?;

    for trigger_id in trigger_ids {
        crate::db::crud::triggers::enqueue_trigger_outbox(conn, &trigger_id, "delete")?;
    }

    sync_workspace_map(conn)?;

    Ok(())
}

/// Serializes active non-default workspaces into the `workspace_map` setting and enqueues sync.
pub fn sync_workspace_map(conn: &Connection) -> crate::Result<()> {
    let workspaces = get_workspaces(conn)?;
    let mut map = serde_json::Map::new();
    for ws in workspaces {
        if !ws.is_default && ws.id != "default" && !ws.is_deleted {
            map.insert(ws.id, serde_json::Value::String(ws.name));
        }
    }
    let payload = serde_json::Value::Object(map);
    crate::db::crud::set_setting(conn, "workspace_map", &payload.to_string())?;
    Ok(())
}

/// Reconciles a remote `workspace_map` JSON payload into local SQLite storage.
pub fn reconcile_workspace_map(conn: &Connection, value: &serde_json::Value) -> crate::Result<()> {
    use std::collections::HashMap;

    let map: HashMap<String, String> = match value {
        serde_json::Value::Object(obj) => obj
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
            .collect(),
        serde_json::Value::String(s) => {
            serde_json::from_str::<HashMap<String, String>>(s).unwrap_or_default()
        }
        _ => HashMap::new(),
    };

    let now = crate::db::now_unix_secs();
    for (id, name) in map {
        if id == "default" || id == "00000000-0000-0000-0000-000000000000" {
            continue;
        }
        conn.execute(
            "INSERT INTO workspaces (id, name, is_default, created_at, updated_at, version, is_deleted)
             VALUES (?1, ?2, 0, ?3, ?3, 1, 0)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 is_deleted = 0
             WHERE workspaces.is_default = 0",
            rusqlite::params![id, name, now],
        )?;
    }
    Ok(())
}
