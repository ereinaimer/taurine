use std::fs::{self, File};
use std::io::{self, Write};
use taurine_core::settings::{
    InlineDictionaryMode, get_cached_inline_dictionary_enabled, get_cached_inline_dictionary_mode,
};
use taurine_core::system::paths::get_data_dir;
use tracing::{debug, error, info};

pub async fn check_and_update_dictionary() {
    if !get_cached_inline_dictionary_enabled() {
        return;
    }

    let mode = get_cached_inline_dictionary_mode();
    let (file_name, url) = match mode {
        InlineDictionaryMode::Lite => (
            "dictionary_lite.db",
            "https://raw.githubusercontent.com/ereinaimer/taurine/dict/dictionary_lite.db.zst",
        ),
        InlineDictionaryMode::Full => (
            "dictionary_full.db",
            "https://raw.githubusercontent.com/ereinaimer/taurine/dict/dictionary_full.db.zst",
        ),
    };

    let dict_dir = get_data_dir().join("dict");

    if let Ok(entries) = fs::read_dir(&dict_dir) {
        for entry in entries.flatten() {
            if let Some(ext) = entry.path().extension()
                && ext == "tmp"
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let db_path = dict_dir.join(file_name);

    if db_path.exists() {
        debug!(
            "Dictionary DB for mode {:?} already exists at {:?}",
            mode, db_path
        );
        return;
    }

    info!(
        "Dictionary DB for mode {:?} missing, starting download...",
        mode
    );

    let mode_name = match mode {
        InlineDictionaryMode::Lite => "Lite",
        InlineDictionaryMode::Full => "Full",
    };
    crate::services::notify::notify_dictionary_download_started(mode_name);

    if let Err(e) = fs::create_dir_all(&dict_dir) {
        error!("Failed to create dictionary directory: {}", e);
        return;
    }

    let temp_path = db_path.with_extension("db.tmp");

    match download_and_decompress(url, &temp_path).await {
        Ok(()) => {
            taurine_core::engine::dictionary::offline::close_cached_connection();

            if let Err(e) = fs::rename(&temp_path, &db_path) {
                error!("Failed to replace dictionary file: {}", e);
                let _ = fs::remove_file(&temp_path);
                return;
            }

            info!("Successfully installed offline {:?} dictionary", mode);
            crate::services::notify::notify_dictionary_installed(mode_name);
        }
        Err(e) => {
            error!("Failed to download and decompress dictionary: {}", e);
            let _ = fs::remove_file(&temp_path);
        }
    }
}

async fn download_and_decompress(url: &str, target_path: &std::path::Path) -> io::Result<()> {
    let response = reqwest::get(url).await.map_err(io::Error::other)?;

    if !response.status().is_success() {
        return Err(io::Error::other(format!(
            "HTTP failure: {}",
            response.status()
        )));
    }

    let bytes = response.bytes().await.map_err(io::Error::other)?;

    let mut decoder = zstd::Decoder::new(io::Cursor::new(&bytes))?;
    let mut file = File::create(target_path)?;

    io::copy(&mut decoder, &mut file)?;
    file.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::settings::{
        InlineDictionaryMode, set_cached_inline_dictionary_enabled,
        set_cached_inline_dictionary_mode,
    };
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_check_and_update_dictionary_integration() {
        let temp = tempdir().unwrap();
        // SAFETY: Setting environment variable for download manager tests.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path().to_str().unwrap()) };

        // 1. Test disabled does not download
        set_cached_inline_dictionary_enabled(false);
        set_cached_inline_dictionary_mode(InlineDictionaryMode::Lite);
        check_and_update_dictionary().await;

        let dict_dir = temp.path().join("dict");
        assert!(!dict_dir.exists());

        // 2. Test enabled downloads and decompresses Lite DB successfully
        set_cached_inline_dictionary_enabled(true);
        check_and_update_dictionary().await;

        let db_path = dict_dir.join("dictionary_lite.db");
        assert!(db_path.exists(), "dictionary_lite.db should be created");

        // Verify the created database is a valid SQLite DB and is queryable
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let mut stmt = conn.prepare("SELECT word FROM dictionary LIMIT 1").unwrap();
        let word: String = stmt.query_row([], |row| row.get(0)).unwrap();
        assert!(
            !word.is_empty(),
            "Database should have valid dictionary entries"
        );

        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
    }
}
