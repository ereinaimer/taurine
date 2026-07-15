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
}
