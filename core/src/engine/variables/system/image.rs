use super::strip_argument_quotes;
use crate::engine::variables::types::ExpansionStep;

/// Images paste as raw bytes, so quality is never touched: oversized files
/// are refused before reading instead of being resized or recompressed.
pub(crate) const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024; // 10MB limit

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
        .map_err(|e| format!("asset not in DB: {}", e))?;

    let decompressed = crate::engine::shell::decompress_bytes(&compressed)
        .map_err(|e| format!("decompress failed: {}", e))?;

    Ok((decompressed, mime_type))
}

fn load_file_image(path_str: &str) -> Result<(Vec<u8>, String), String> {
    let path = crate::engine::variables::system::file::expand_path(path_str)
        .ok_or_else(|| "Invalid path".to_string())?;

    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    if path.metadata().map(|m| m.len()).unwrap_or(0) > MAX_IMAGE_BYTES as u64 {
        return Err(format!("Image exceeds 10MB limit: {}", path.display()));
    }

    let bytes = std::fs::read(&path).map_err(|e| format!("read failed: {}", e))?;

    image::guess_format(&bytes)
        .map_err(|_| format!("'{}' not a supported image (PNG/JPEG only)", path.display()))?;

    let mime_type = get_mime_type(&bytes, path_str);
    Ok((bytes, mime_type))
}

pub fn parse_img_directive(inner: &str) -> Option<ExpansionStep> {
    if let Some(rest) = inner.strip_prefix("image(")
        && rest.ends_with(')')
    {
        let path = strip_argument_quotes(rest[..rest.len() - 1].trim());
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
            Err(e) => {
                tracing::warn!("Failed to load image '{}': {}", path, e);
                None
            }
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal valid 1x1 PNG.
    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn quoted_image_path_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("logo.png");
        std::fs::write(&path, TINY_PNG).unwrap();
        let key = format!("image(\"{}\")", path.display());
        assert!(parse_img_directive(&key).is_some());
    }

    #[test]
    fn oversize_image_refused_before_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.png");
        let mut bytes = TINY_PNG.to_vec();
        bytes.resize(10 * 1024 * 1024 + 1, 0);
        std::fs::write(&path, &bytes).unwrap();
        let key = format!("image({})", path.display());
        assert!(parse_img_directive(&key).is_none());
    }
}
