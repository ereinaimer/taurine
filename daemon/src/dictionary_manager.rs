use std::fs::File;
use std::io::Write;
use taurine_core::system::paths::get_data_dir;
use tracing::{debug, error, info};

const DICTIONARY_DB_URL: &str =
    "https://github.com/ereinaimer/taurine/releases/latest/download/dictionary.db";

pub async fn initialize_dictionary_if_enabled() {
    if !taurine_core::settings::get_cached_inline_dictionary_enabled() {
        return;
    }

    let db_path = get_data_dir().join("dictionary.db");
    if db_path.exists() {
        debug!("Dictionary DB already exists at {:?}", db_path);
        return;
    }

    info!("Dictionary DB missing, downloading in the background...");

    match reqwest::get(DICTIONARY_DB_URL).await {
        Ok(response) => {
            if response.status().is_success() {
                match response.bytes().await {
                    Ok(bytes) => {
                        let temp_path = db_path.with_extension("db.tmp");
                        if let Ok(mut file) = File::create(&temp_path)
                            && file.write_all(&bytes).is_ok()
                            && std::fs::rename(&temp_path, &db_path).is_ok()
                        {
                            info!("Successfully downloaded dictionary DB to {:?}", db_path);
                            return;
                        }
                        error!("Failed to write dictionary DB to disk");
                    }
                    Err(e) => error!("Failed to read dictionary DB response bytes: {}", e),
                }
            } else {
                error!(
                    "Failed to download dictionary DB: HTTP {}",
                    response.status()
                );
            }
        }
        Err(e) => error!("Failed to download dictionary DB: {}", e),
    }
}
