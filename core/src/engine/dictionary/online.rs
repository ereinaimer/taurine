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

    let response = client.get(&url).send().await.ok()?;

    if response.status().is_success() {
        match response.json::<Vec<DictionaryEntry>>().await {
            Ok(entries) => Some(entries),
            Err(e) => {
                warn!("Failed to parse dictionary API response: {}", e);
                None
            }
        }
    } else {
        None
    }
}
