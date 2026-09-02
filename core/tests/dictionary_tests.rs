use rusqlite::Connection;
use taurine_core::engine::dictionary::lookup_word;
use taurine_core::settings::{InlineDictionaryMode, set_cached_inline_dictionary_mode};
use tempfile::tempdir;

#[tokio::test]
#[allow(clippy::await_holding_lock)] // TEST_LOCK must be held for the full test to serialize env var mutation
async fn test_offline_lookup_with_isolated_fixture() {
    let _lock = taurine_core::testing::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());

    let temp = tempdir().unwrap();
    // SAFETY: Setting environment variable for test directory isolation under TEST_LOCK.
    unsafe { std::env::set_var("TAURINE_DATA_DIR", temp.path().to_str().unwrap()) };

    let dict_dir = temp.path().join("dict");
    std::fs::create_dir_all(&dict_dir).unwrap();

    let db_path = dict_dir.join("dictionary_lite.db");
    let conn = Connection::open(&db_path).unwrap();
    conn.execute(
        "CREATE TABLE dictionary (word TEXT PRIMARY KEY, data TEXT)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO dictionary (word, data) VALUES ('world', '[{\"word\":\"world\",\"meanings\":[]}]')",
        [],
    )
    .unwrap();
    drop(conn);

    set_cached_inline_dictionary_mode(InlineDictionaryMode::Lite);
    taurine_core::engine::dictionary::offline::close_cached_connection();

    let result = lookup_word("world").await;
    assert!(result.is_some());
    let entries = result.unwrap();
    assert_eq!(entries[0].word.to_lowercase(), "world");

    taurine_core::engine::dictionary::offline::close_cached_connection();
    // SAFETY: Cleaning up environment variable under TEST_LOCK.
    unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
}
