use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptInterpreter {
    Bash,
    PowerShell,
    Python,
    Node,
    Cmd,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptBehavior {
    Inline,
    Silent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptMetadata {
    pub interpreter: ScriptInterpreter,
    pub behavior: ScriptBehavior,
    pub compressed_content: Vec<u8>,
}

/// Compresses a script string using zstd for efficient storage.
pub fn compress(content: &str) -> crate::Result<Vec<u8>> {
    zstd::bulk::compress(content.as_bytes(), 3)
        .map_err(|e| crate::Error::Service(format!("zstd compression failed: {}", e)))
}

/// Decompresses binary script content back into a String.
///
/// Implements a 1MB safety limit for decompression to prevent zip-bomb style attacks.
pub fn decompress(compressed: &[u8]) -> crate::Result<String> {
    const MAX_SCRIPT_SIZE: usize = 1024 * 1024; // 1MB

    let decompressed = zstd::bulk::decompress(compressed, MAX_SCRIPT_SIZE)
        .map_err(|e| crate::Error::Service(format!("zstd decompression failed: {}", e)))?;

    String::from_utf8(decompressed).map_err(|e| {
        crate::Error::Service(format!("UTF-8 decoding failed after decompression: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_round_trip() {
        let original = "echo 'Hello, World!'";
        let compressed = compress(original).unwrap();
        let decompressed = decompress(&compressed).unwrap();
        assert_eq!(original, decompressed);
    }

    #[test]
    fn test_compression_efficiency() {
        let long_script = "echo 'line'\n".repeat(100);
        let compressed = compress(&long_script).unwrap();
        assert!(compressed.len() < long_script.len());
    }
}
