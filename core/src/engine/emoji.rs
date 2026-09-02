pub fn search_emoji_shortcodes(query: &str) -> Vec<String> {
    if query.is_empty() {
        return Vec::new();
    }
    let normalized = query.replace('_', "-");
    let mut matches = Vec::new();
    for emoji in emojis::iter() {
        for shortcode in emoji.shortcodes() {
            let hyphen_shortcode = shortcode.replace('_', "-");
            if hyphen_shortcode.starts_with(&normalized) {
                matches.push(hyphen_shortcode);
            }
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

pub fn lookup_emoji(shortcode: &str) -> Option<String> {
    let normalized = shortcode.replace('-', "_");
    emojis::get_by_shortcode(&normalized).map(|e| e.as_str().to_string())
}

const NL_EMOJI_ALIASES: &[(&str, &[&str])] = &[
    // Smileys & Emotion
    (
        "😊",
        &[
            "happy",
            "happy face",
            "smile",
            "smiling",
            "cheerful",
            "grin",
            "glad",
            "joy",
        ],
    ),
    (
        "😄",
        &[
            "happy", "smile", "smiling", "cheerful", "grin", "glad", "joy",
        ],
    ),
    (
        "😀",
        &[
            "happy", "smile", "smiling", "cheerful", "grin", "glad", "joy",
        ],
    ),
    (
        "😂",
        &[
            "laugh",
            "laughing",
            "funny",
            "lol",
            "rofl",
            "tears of joy",
            "haha",
            "xd",
        ],
    ),
    (
        "🤣",
        &[
            "laugh",
            "laughing",
            "funny",
            "lol",
            "rofl",
            "tears of joy",
            "haha",
            "xd",
        ],
    ),
    ("😉", &["wink", "winking", "wink face"]),
    ("😍", &["love face", "heart eyes", "love eyes", "crush"]),
    ("😘", &["blow kiss", "kiss face", "kissing", "love kiss"]),
    ("😋", &["yum", "yummy", "delicious", "hungry", "tasty"]),
    ("😎", &["cool", "sunglasses", "smart", "swag", "glasses"]),
    (
        "😢",
        &[
            "sad",
            "crying",
            "tear",
            "sad face",
            "sob",
            "depressed",
            "upset",
        ],
    ),
    (
        "😭",
        &["sad", "crying", "tear", "sob", "depressed", "upset"],
    ),
    ("😱", &["scream", "scared", "shocked", "gasp", "fear"]),
    ("😡", &["angry", "mad", "rage", "pissed", "furious"]),
    ("😠", &["angry", "mad", "rage", "pissed", "furious"]),
    ("🤔", &["think", "thinking", "hmm", "ponder", "curious"]),
    ("🤫", &["shush", "quiet", "silent", "whisper", "shh"]),
    (
        "😴",
        &["sleep", "sleeping", "tired", "zzz", "snore", "bedtime"],
    ),
    ("💤", &["sleep", "sleeping", "tired", "zzz", "snore"]),
    // Hand Gestures
    (
        "👍",
        &["thumbs up", "like", "yes", "agree", "ok", "good", "correct"],
    ),
    ("👎", &["thumbs down", "dislike", "no", "bad", "incorrect"]),
    ("👌", &["ok", "okay", "perfect", "deal"]),
    ("✌️", &["peace", "victory", "two"]),
    ("🤞", &["fingers crossed", "hope", "luck", "wishing"]),
    (
        "🙏",
        &["pray", "please", "thank you", "thanks", "hope", "grateful"],
    ),
    ("👏", &["clap", "clapping", "applause", "bravo"]),
    ("🙌", &["raise hands", "hooray", "celebrate", "hands"]),
    ("🫶", &["heart hands", "love hands", "heart", "love"]),
    ("🤝", &["handshake", "deal", "agree", "partnership"]),
    ("👊", &["fist", "punch", "brofist", "fist bump"]),
    ("👋", &["wave", "waving", "hello", "hi", "bye", "goodbye"]),
    // Hearts & Love
    ("❤️", &["red heart", "love", "heart", "like"]),
    ("💔", &["broken heart", "sad heart", "heartbreak"]),
    ("💖", &["sparkling heart", "love", "heart"]),
    // Symbols & Indicators
    (
        "✨",
        &["sparkles", "shine", "magic", "clean", "new", "glitter"],
    ),
    ("🔥", &["fire", "lit", "hot", "burn", "cool"]),
    (
        "🎉",
        &["party", "celebrate", "congrats", "hooray", "birthday"],
    ),
    ("💯", &["100", "hundred", "perfect", "truth", "correct"]),
    ("⭐", &["star", "gold star", "favorite"]),
    ("🌟", &["star", "gold star", "favorite"]),
    (
        "✅",
        &["check mark", "check", "done", "correct", "success", "ok"],
    ),
    ("❌", &["cross mark", "cross", "x", "wrong", "no", "error"]),
    ("⚠️", &["warning", "caution", "alert"]),
    ("ℹ️", &["info", "information"]),
    ("❓", &["question", "help", "ask"]),
    ("💡", &["idea", "light bulb", "think", "brainstorm"]),
    ("🔍", &["search", "find", "magnifying glass"]),
    ("📌", &["pin", "pushpin", "save"]),
    // Objects & Media
    ("🚀", &["rocket", "spaceship", "launch", "fast", "speed"]),
    ("💵", &["money", "cash", "dollar", "rich", "pay"]),
    ("💰", &["money", "cash", "dollar", "rich", "wealth"]),
    ("⏰", &["time", "clock", "watch", "alarm"]),
    ("⌚", &["time", "clock", "watch"]),
    ("💻", &["computer", "laptop", "pc", "screen", "developer"]),
    ("🖥️", &["computer", "laptop", "pc", "screen"]),
    ("📱", &["phone", "mobile", "smartphone"]),
    ("✉️", &["mail", "email", "letter"]),
    ("🎁", &["gift", "present", "birthday", "surprise"]),
    ("🎈", &["balloon", "party", "celebrate"]),
    // Food & Drinks
    ("🍕", &["pizza", "food", "slice", "italian"]),
    ("🍺", &["beer", "beers", "cheers", "drink", "pub"]),
    ("🍻", &["beer", "beers", "cheers", "drink", "party"]),
    ("☕", &["coffee", "tea", "cup", "morning", "cafe"]),
    // Nature & Animals
    ("☀️", &["sun", "sunny", "weather", "summer", "hot", "day"]),
    ("🌧️", &["rain", "rainy", "weather", "wet", "shower"]),
    ("❄️", &["snow", "snowflake", "cold", "winter"]),
    ("🌈", &["rainbow", "pride", "color"]),
    ("🐱", &["cat", "kitten", "kitty", "meow"]),
    ("🐶", &["dog", "puppy", "woof", "bark"]),
    // Vehicles & Travel
    ("✈️", &["plane", "airplane", "flight", "travel", "vacation"]),
    ("🚗", &["car", "drive", "vehicle", "trip"]),
];

static EXACT_NL_EMOJI_CACHE: std::sync::LazyLock<std::collections::HashMap<String, Vec<String>>> =
    std::sync::LazyLock::new(|| {
        let mut map = std::collections::HashMap::new();
        for &(_emoji_char, aliases) in NL_EMOJI_ALIASES {
            for &alias in aliases {
                let key = alias.to_lowercase();
                map.entry(key.clone())
                    .or_insert_with(|| search_natural_language_emojis_uncached(&key));
            }
        }
        for emoji in emojis::iter() {
            let key = emoji.name().to_lowercase();
            map.entry(key.clone())
                .or_insert_with(|| search_natural_language_emojis_uncached(&key));
        }
        map
    });

pub fn search_natural_language_emojis(query: &str) -> Vec<String> {
    let normalized_query = query.to_lowercase().trim().to_string();
    if normalized_query.is_empty() {
        return Vec::new();
    }

    if let Some(cached) = EXACT_NL_EMOJI_CACHE.get(&normalized_query) {
        return cached.clone();
    }

    search_natural_language_emojis_uncached(&normalized_query)
}

fn search_natural_language_emojis_uncached(normalized_query: &str) -> Vec<String> {
    let query_words: Vec<&str> = normalized_query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();

    if query_words.is_empty() {
        return Vec::new();
    }

    let mut matches: Vec<(String, i32, usize)> = Vec::new();

    let check_match = |text: &str, query_words: &[&str]| -> bool {
        let text_words: Vec<&str> = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| !s.is_empty())
            .collect();
        query_words.iter().all(|qw| text_words.contains(qw))
    };

    // 1. Check curated aliases
    for &(emoji_char, aliases) in NL_EMOJI_ALIASES {
        for alias in aliases {
            if check_match(alias, &query_words) {
                let is_exact = alias.to_lowercase() == normalized_query;
                let score = 1000 + if is_exact { 500 } else { 0 };
                matches.push((emoji_char.to_string(), score, alias.len()));
            }
        }
    }

    // 2. Check emojis crate names (NOT shortcodes)
    for emoji in emojis::iter() {
        let name = emoji.name().to_lowercase();
        if check_match(&name, &query_words) {
            let is_exact = name == normalized_query;
            let score = if is_exact { 500 } else { 0 };
            matches.push((emoji.as_str().to_string(), score, name.len()));
        }
    }

    // Deduplicate keeping highest score
    let mut unique_matches: std::collections::HashMap<String, (i32, usize)> =
        std::collections::HashMap::new();
    for (emoji, score, len) in matches {
        let entry = unique_matches
            .entry(emoji)
            .or_insert((i32::MIN, usize::MAX));
        if score > entry.0 || (score == entry.0 && len < entry.1) {
            *entry = (score, len);
        }
    }

    let mut result: Vec<(String, i32, usize)> = unique_matches
        .into_iter()
        .map(|(k, (s, l))| (k, s, l))
        .collect();

    // Sort by score desc, then by alias length asc, then alphabetically
    result.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.2.cmp(&b.2))
            .then_with(|| a.0.cmp(&b.0))
    });

    result.into_iter().map(|(k, _, _)| k).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_and_lookup() {
        let results = search_emoji_shortcodes("rocke");
        assert!(results.contains(&"rocket".to_string()));
        assert_eq!(lookup_emoji("rocket"), Some("🚀".to_string()));
        assert_eq!(lookup_emoji("invalid-emoji-name"), None);

        // Test that both hyphens and underscores work for searches (returning hyphenated suggestions)
        let results_with_underscore = search_emoji_shortcodes("heart_ey");
        assert!(results_with_underscore.contains(&"heart-eyes".to_string()));

        let results_with_hyphen = search_emoji_shortcodes("heart-ey");
        assert!(results_with_hyphen.contains(&"heart-eyes".to_string()));

        // Test that both hyphens and underscores work for lookups
        assert_eq!(lookup_emoji("heart_eyes"), Some("😍".to_string()));
        assert_eq!(lookup_emoji("heart-eyes"), Some("😍".to_string()));
    }

    #[test]
    fn test_natural_language_emoji_search() {
        let happy_matches = search_natural_language_emojis("happy");
        assert!(happy_matches.contains(&"😊".to_string()));

        let happy_face_matches = search_natural_language_emojis("happy face");
        assert_eq!(happy_face_matches.first(), Some(&"😊".to_string()));

        let rocket_matches = search_natural_language_emojis("rocket");
        assert_eq!(rocket_matches, vec!["🚀".to_string()]);

        let heart_matches = search_natural_language_emojis("heart");
        assert!(heart_matches.contains(&"❤️".to_string()));
        assert!(heart_matches.contains(&"🫶".to_string()));

        // Exact word matching: partial prefix should return empty
        let partial = search_natural_language_emojis("he");
        assert!(partial.is_empty());

        // Single char should return empty
        let single = search_natural_language_emojis("h");
        assert!(single.is_empty());

        // Shortcode word "pi" in man_with_gua_pi_mao should NOT match
        let pi = search_natural_language_emojis("pi");
        assert!(pi.is_empty());

        // Digit-only aliases should match
        let hundred = search_natural_language_emojis("100");
        assert_eq!(hundred, vec!["💯".to_string()]);
    }
}
