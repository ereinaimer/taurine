pub mod offline;
pub mod online;
pub mod types;

use tracing::debug;
pub use types::*;

pub async fn lookup_word(word: &str) -> Option<Vec<DictionaryEntry>> {
    // Attempt offline lookup first (fast, local)
    if let Some(entries) = offline::lookup_offline(word) {
        debug!("Dictionary cache hit for '{}'", word);
        return Some(entries);
    }

    // Fall back to online API
    debug!(
        "Dictionary cache miss for '{}', falling back to online API",
        word
    );
    online::lookup_online(word).await
}
