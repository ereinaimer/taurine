use crate::engine::variables::types::ExpansionStep;

fn decode_rgba_image(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let img =
        image::load_from_memory(bytes).map_err(|e| format!("Failed to decode image: {}", e))?;
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok((rgba.into_raw(), width, height))
}

fn load_asset_image(hash: &str) -> Result<(Vec<u8>, u32, u32), String> {
    let conn = crate::db::get_conn().map_err(|e| e.to_string())?;
    let compressed: Vec<u8> = conn
        .query_row(
            "SELECT compressed_content FROM assets WHERE id = ?1",
            [hash],
            |row| row.get(0),
        )
        .map_err(|e| format!("Asset not found in database: {}", e))?;

    let decompressed = crate::engine::shell::decompress_bytes(&compressed)
        .map_err(|e| format!("Failed to decompress image asset: {}", e))?;

    decode_rgba_image(&decompressed)
}

fn load_file_image(path_str: &str) -> Result<(Vec<u8>, u32, u32), String> {
    let path = crate::engine::variables::system::file::expand_path(path_str)
        .ok_or_else(|| "Invalid path".to_string())?;

    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    let bytes = std::fs::read(&path).map_err(|e| format!("Failed to read image file: {}", e))?;

    decode_rgba_image(&bytes)
}

pub fn parse_img_directive(inner: &str) -> Option<ExpansionStep> {
    if let Some(rest) = inner.strip_prefix("img(")
        && rest.ends_with(')')
    {
        let path = rest[..rest.len() - 1].trim();
        if path.is_empty() {
            return None;
        }

        let res = if path.starts_with("asset(") && path.ends_with(')') {
            let hash = path[6..path.len() - 1].trim();
            load_asset_image(hash)
        } else {
            load_file_image(path)
        };

        match res {
            Ok((rgba, w, h)) => Some(ExpansionStep::Image(rgba, w, h)),
            Err(e) => {
                // If it fails, output error text inside the expansion
                Some(ExpansionStep::Text(format!("[Error: {}]", e)))
            }
        }
    } else {
        None
    }
}
