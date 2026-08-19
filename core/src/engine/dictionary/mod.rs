pub mod offline;
pub mod online;
pub mod types;

use regex::Regex;
use std::sync::LazyLock;
use tracing::debug;
pub use types::*;

pub static DICTIONARY_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    // Matches "meaning of <word>", "synonym of <word>", "synonyms of <word>", "antonyms of <word>" at the end of the buffer
    Regex::new(r"(?i)\b(meaning|synonyms?|antonyms?) of ([a-zA-Z]+)$")
        .expect("Valid dictionary regex")
});

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
