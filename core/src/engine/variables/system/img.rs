use crate::engine::variables::types::ExpansionStep;

fn get_mime_type(bytes: &[u8], path_str: &str) -> String {
    if let Ok(format) = image::guess_format(bytes) {
        match format {
            image::ImageFormat::Png => "image/png".to_string(),
            image::ImageFormat::Jpeg => "image/jpeg".to_string(),
            image::ImageFormat::Gif => "image/gif".to_string(),
            image::ImageFormat::Bmp => "image/bmp".to_string(),
            _ => "application/octet-stream".to_string(),
        }
    } else {
        let path = std::path::Path::new(path_str);
        match path.extension().and_then(|ext| ext.to_str()) {
            Some(ext) => match ext.to_lowercase().as_str() {
                "png" => "image/png".to_string(),
                "jpg" | "jpeg" => "image/jpeg".to_string(),
                "gif" => "image/gif".to_string(),
                "bmp" => "image/bmp".to_string(),
                _ => "application/octet-stream".to_string(),
            },
            None => "application/octet-stream".to_string(),
        }
    }
}

fn load_asset_image(hash: &str) -> Result<(Vec<u8>, String), String> {
    let conn = crate::db::get_conn().map_err(|e| e.to_string())?;
    let (mime_type, compressed): (String, Vec<u8>) = conn
        .query_row(
            "SELECT mime_type, compressed_content FROM assets WHERE id = ?1",
            [hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("Asset not found in database: {}", e))?;

    let decompressed = crate::engine::shell::decompress_bytes(&compressed)
        .map_err(|e| format!("Failed to decompress image asset: {}", e))?;

    Ok((decompressed, mime_type))
}

fn load_file_image(path_str: &str) -> Result<(Vec<u8>, String), String> {
    let path = crate::engine::variables::system::file::expand_path(path_str)
        .ok_or_else(|| "Invalid path".to_string())?;

    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    let bytes = std::fs::read(&path).map_err(|e| format!("Failed to read image file: {}", e))?;

    image::guess_format(&bytes).map_err(|_| {
        format!(
            "'{}' is not a supported image file (PNG or JPEG required)",
            path.display()
        )
    })?;

    let mime_type = get_mime_type(&bytes, path_str);
    Ok((bytes, mime_type))
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
            Ok((bytes, mime_type)) => Some(ExpansionStep::Image(bytes, mime_type)),
            Err(e) => Some(ExpansionStep::Text(format!("[Error: {}]", e))),
        }
    } else {
        None
    }
}
