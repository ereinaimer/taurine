use crate::Result;
use crate::db::now_unix_secs;
use crate::engine::variables::tags::*;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

pub(crate) fn compile_and_save_assets(
    conn: &Connection,
    trigger_id: &str,
    output: &str,
) -> Result<String> {
    let mut processed = String::new();
    let mut ptr = 0;
    let mut active_hashes = std::collections::HashSet::new();

    while let Some(tag) = find_next_tag(output, ptr) {
        processed.push_str(&output[ptr..tag.start]);

        let inner = trim_slice(&output[tag.start + 1..tag.end]);
        let mut rewritten_tag = None;

        if let Some(rest) = inner.strip_prefix("img(")
            && rest.ends_with(')')
        {
            let path = trim_slice(&rest[..rest.len() - 1]);
            if path.starts_with("asset(") && path.ends_with(')') {
                let hash = trim_slice(&path[6..path.len() - 1]);
                active_hashes.insert(hash.to_string());
                rewritten_tag = Some(format!("[img(asset({}))]", hash));
            } else if !path.is_empty()
                && let Some(path_buf) = crate::engine::variables::system::file::expand_path(path)
            {
                let bytes = std::fs::read(&path_buf).map_err(|_| {
                    crate::Error::Config(format!("img: file not found: {}", path_buf.display()))
                })?;

                // Validate the bytes are a recognized image format.
                image::guess_format(&bytes).map_err(|_| {
                    crate::Error::Config(format!(
                        "img: '{}' is not a supported image file (PNG or JPEG required)",
                        path_buf.display()
                    ))
                })?;

                let compressed = zstd::bulk::compress(&bytes, 3).map_err(|e| {
                    crate::Error::Service(format!("zstd compression failed: {}", e))
                })?;
                let mut hasher = Sha256::new();
                hasher.update(&compressed);
                let hash = hex::encode(hasher.finalize());

                let mime_type = match path_buf.extension().and_then(|ext| ext.to_str()) {
                    Some(ext) => match ext.to_lowercase().as_str() {
                        "png" => "image/png",
                        "jpg" | "jpeg" => "image/jpeg",
                        "gif" => "image/gif",
                        "bmp" => "image/bmp",
                        _ => "application/octet-stream",
                    },
                    None => "application/octet-stream",
                };

                let now = now_unix_secs();
                conn.execute(
                    "INSERT OR REPLACE INTO assets (id, trigger_id, mime_type, compressed_content, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    (
                        &hash,
                        trigger_id,
                        mime_type,
                        &compressed,
                        now,
                    ),
                )?;

                active_hashes.insert(hash.clone());
                rewritten_tag = Some(format!("[img(asset({}))]", hash));
            }
        } else if inner.starts_with("exec.")
            && inner.contains(".file(")
            && let Ok(invocation) = crate::engine::variables::system::exec::parse_invocation(inner)
            && invocation.file
        {
            let path = invocation.subject.trim();
            if path.starts_with("asset(") && path.ends_with(')') {
                let hash = trim_slice(&path[6..path.len() - 1]);
                active_hashes.insert(hash.to_string());
                rewritten_tag = Some(format!("[{}]", inner));
            } else if !path.is_empty()
                && let Some(path_buf) = crate::engine::variables::system::file::expand_path(path)
                && let Ok(bytes) = std::fs::read(&path_buf)
            {
                let compressed = zstd::bulk::compress(&bytes, 3).map_err(|e| {
                    crate::Error::Service(format!("zstd compression failed: {}", e))
                })?;
                let mut hasher = Sha256::new();
                hasher.update(&compressed);
                let hash = hex::encode(hasher.finalize());

                let mime_type = match path_buf.extension().and_then(|ext| ext.to_str()) {
                    Some(ext) => match ext.to_lowercase().as_str() {
                        "sh" | "bash" => "text/x-shellscript",
                        "py" => "text/x-python",
                        "js" => "text/javascript",
                        "ps1" => "text/x-powershell",
                        _ => "application/octet-stream",
                    },
                    None => "application/octet-stream",
                };

                let now = now_unix_secs();
                conn.execute(
                    "INSERT OR REPLACE INTO assets (id, trigger_id, mime_type, compressed_content, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    (
                        &hash,
                        trigger_id,
                        mime_type,
                        &compressed,
                        now,
                    ),
                )?;

                active_hashes.insert(hash.clone());
                let file_pattern = format!("file({})", invocation.subject);
                let replacement = format!("file(asset({}))", hash);
                let new_inner = inner.replace(&file_pattern, &replacement);
                rewritten_tag = Some(format!("[{}]", new_inner));
            }
        }

        if let Some(rewritten) = rewritten_tag {
            processed.push_str(&rewritten);
        } else {
            processed.push_str(&output[tag.start..tag.end + 1]);
        }

        ptr = tag.end + 1;
    }
    processed.push_str(&output[ptr..]);

    if active_hashes.is_empty() {
        conn.execute("DELETE FROM assets WHERE trigger_id = ?1", [trigger_id])?;
    } else {
        let placeholders: Vec<String> = active_hashes.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "DELETE FROM assets WHERE trigger_id = ?1 AND id NOT IN ({})",
            placeholders.join(",")
        );
        let mut params: Vec<&dyn rusqlite::ToSql> = vec![&trigger_id];
        for h in &active_hashes {
            params.push(h);
        }
        conn.execute(&query, rusqlite::params_from_iter(params))?;
    }

    Ok(processed)
}
