use taurine_core::engine::dictionary::{lookup_word, online::lookup_online};

#[tokio::test]
async fn test_online_lookup() {
    if let Some(entries) = lookup_online("hello").await {
        assert!(!entries.is_empty());
        assert_eq!(entries[0].word.to_lowercase(), "hello");
    }
}

#[tokio::test]
async fn test_offline_lookup_fallback() {
    let result = lookup_word("world").await;
    assert!(result.is_some());
    let entries = result.unwrap();
    assert!(!entries.is_empty());
    assert_eq!(entries[0].word.to_lowercase(), "world");
}
