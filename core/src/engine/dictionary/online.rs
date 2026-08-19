use super::types::DictionaryEntry;
use reqwest::Client;
use std::time::Duration;
use tracing::warn;

pub async fn lookup_online(word: &str) -> Option<Vec<DictionaryEntry>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;

    let url = format!("https://api.dictionaryapi.dev/api/v2/entries/en/{}", word);

    let mut attempts = 0;
    while attempts < 3 {
        attempts += 1;
        match client.get(&url).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<Vec<DictionaryEntry>>().await {
                        Ok(entries) => return Some(entries),
                        Err(e) => {
                            warn!("Failed to parse dictionary API response: {}", e);
                            return None;
                        }
                    }
                } else if response.status().is_server_error()
                    || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
                {
                    warn!(
                        "Dictionary API server error ({}). Attempt {}/3",
                        response.status(),
                        attempts
                    );
                } else {
                    // Client errors like 404 Not Found, retrying won't help
                    return None;
                }
            }
            Err(e) => {
                warn!(
                    "Dictionary API network error: {}. Attempt {}/3",
                    e, attempts
                );
            }
        }

        if attempts < 3 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    None
}
