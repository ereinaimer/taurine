use super::*;
use crate::db::crud::triggers::overlap::app_filters_overlap;
use crate::db::crud::triggers::validate::{
    check_limits_recursive, count_ai_calls_in_template, get_referenced_triggers,
    validate_trigger_limits,
};
use rusqlite::Connection;

#[test]
fn test_create_trigger_name_exceeds_max_length() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    let long_name = "a".repeat(201);
    let new_trigger = NewTrigger {
        name: Some(&long_name),
        description: None,
        trigger_type: TriggerType::Word,
        trigger: "test_trigger",
        content: "test output",
        action_type: "text",
        target_os: "all",
        tags_json: "[]",
        auto_case: false,
        interpreter: None,
        behavior: None,
    };
    let result = create_trigger(&mut conn, new_trigger);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn test_create_trigger_description_exceeds_max_length() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    let long_desc = "a".repeat(1001);
    let new_trigger = NewTrigger {
        name: Some("short name"),
        description: Some(&long_desc),
        trigger_type: TriggerType::Word,
        trigger: "test_trigger2",
        content: "test output",
        action_type: "text",
        target_os: "all",
        tags_json: "[]",
        auto_case: false,
        interpreter: None,
        behavior: None,
    };
    let result = create_trigger(&mut conn, new_trigger);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn add_trigger_rejects_invalid_regex() {
    let (_dir, conn) = crate::testing::open_test_db();
    let result = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Regex,
        "[invalid",
        "output",
        "all",
        None,
        None,
        None,
        None,
        None,
        false,
    );
    assert!(result.is_err(), "invalid regex should be rejected");
}

#[test]
fn add_trigger_accepts_valid_regex() {
    let (_dir, conn) = crate::testing::open_test_db();
    let result = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Regex,
        r"^foo\d+bar$",
        "output",
        "all",
        None,
        None,
        None,
        None,
        None,
        false,
    );
    assert!(result.is_ok(), "valid regex should be accepted: {result:?}");
}

#[test]
fn test_create_trigger_short_name_succeeds() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    let new_trigger = NewTrigger {
        name: Some("short name"),
        description: Some("short description"),
        trigger_type: TriggerType::Word,
        trigger: "test_trigger3",
        content: "test output",
        action_type: "text",
        target_os: "all",
        tags_json: "[]",
        auto_case: false,
        interpreter: None,
        behavior: None,
    };
    let result = create_trigger(&mut conn, new_trigger);
    assert!(result.is_ok());
}

#[test]
fn test_update_trigger_name_exceeds_max_length() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    let id = create_trigger(
        &mut conn,
        NewTrigger {
            name: Some("original"),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "update_test_trigger",
            content: "test output",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            interpreter: None,
            behavior: None,
        },
    )
    .unwrap();

    let long_name = "a".repeat(201);
    let result = update_existing_trigger(
        &mut conn,
        ExistingTriggerUpdate {
            id: &id,
            name: &long_name,
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "update_test_trigger",
            content: "test output",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            usage_count: 0,
            last_used_at: None,
            interpreter: None,
            behavior: None,
        },
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn test_update_trigger_description_exceeds_max_length() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    let id = create_trigger(
        &mut conn,
        NewTrigger {
            name: Some("original"),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "update_desc_test",
            content: "test output",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            interpreter: None,
            behavior: None,
        },
    )
    .unwrap();

    let long_desc = "a".repeat(1001);
    let result = update_existing_trigger(
        &mut conn,
        ExistingTriggerUpdate {
            id: &id,
            name: "original",
            description: Some(&long_desc),
            trigger_type: TriggerType::Word,
            trigger: "update_desc_test",
            content: "test output",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            usage_count: 1,
            last_used_at: None,
            interpreter: None,
            behavior: None,
        },
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn test_prepare_trigger_exceeds_max_length() {
    let long_trigger = "a".repeat(201);
    let result = prepare_trigger_with_type(&long_trigger, TriggerType::Word, "all");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn test_prepare_trigger_at_max_length() {
    let max_trigger = "a".repeat(200);
    let result = prepare_trigger_with_type(&max_trigger, TriggerType::Word, "all");
    assert!(result.is_ok());
}

#[test]
fn test_prepare_trigger_with_spaces_succeeds() {
    let result = prepare_trigger_with_type("my email address", TriggerType::Word, "all");
    assert!(result.is_ok());
}

#[test]
fn test_prepare_trigger_with_newlines_rejected() {
    let result = prepare_trigger_with_type("my\nemail", TriggerType::Word, "all");
    assert!(result.is_err());
}

#[test]
fn test_count_ai_calls_in_template() {
    assert_eq!(count_ai_calls_in_template("Hello world"), 0);
    assert_eq!(count_ai_calls_in_template("Hello [clip | upper]"), 0);
    assert_eq!(count_ai_calls_in_template("[clip | ai(summarize)]"), 1);
    assert_eq!(
        count_ai_calls_in_template("[clip | ai(a) | upper | ai(b)] and [date | ai(c)]"),
        3
    );
}

#[test]
fn test_get_referenced_triggers() {
    assert!(get_referenced_triggers("Hello world").is_empty());
    assert_eq!(get_referenced_triggers("Hello [use(\"foo\")]"), vec!["foo"]);
    assert_eq!(
        get_referenced_triggers("[use('bar')] [use(baz)]"),
        vec!["bar", "baz"]
    );
}

#[test]
fn test_check_limits_recursive_detects_cycles() {
    let mut catalog = std::collections::HashMap::new();
    catalog.insert("A".to_string(), "calls [use(\"B\")]".to_string());
    catalog.insert("B".to_string(), "calls [use(\"A\")]".to_string());

    let mut visited = std::collections::HashSet::new();
    let mut max_depth = 0;
    let mut ai_count = 0;
    let mut cursor_count = 0;
    let mut has_key_or_delay = false;

    let result = check_limits_recursive(
        &catalog,
        "A",
        &mut visited,
        1,
        &mut max_depth,
        &mut ai_count,
        &mut cursor_count,
        &mut has_key_or_delay,
    );
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Circular reference")
    );
}

#[test]
fn test_check_limits_recursive_enforces_depth() {
    let mut catalog = std::collections::HashMap::new();
    catalog.insert("1".to_string(), "[use(\"2\")]".to_string());
    catalog.insert("2".to_string(), "[use(\"3\")]".to_string());
    catalog.insert("3".to_string(), "[use(\"4\")]".to_string());
    catalog.insert("4".to_string(), "[use(\"5\")]".to_string());
    catalog.insert("5".to_string(), "[use(\"6\")]".to_string());
    catalog.insert("6".to_string(), "done".to_string());

    let mut visited = std::collections::HashSet::new();
    let mut max_depth = 0;
    let mut ai_count = 0;
    let mut cursor_count = 0;
    let mut has_key_or_delay = false;

    let result = check_limits_recursive(
        &catalog,
        "1",
        &mut visited,
        1,
        &mut max_depth,
        &mut ai_count,
        &mut cursor_count,
        &mut has_key_or_delay,
    );
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("depth limit (5) exceeded")
    );
}

#[test]
fn test_check_limits_recursive_enforces_ai_count() {
    let mut catalog = std::collections::HashMap::new();
    catalog.insert("A".to_string(), "[clip | ai(1)] [use(\"B\")]".to_string());
    catalog.insert("B".to_string(), "[clip | ai(2)] [use(\"C\")]".to_string());
    catalog.insert("C".to_string(), "[clip | ai(3)] [clip | ai(4)]".to_string());

    let mut visited = std::collections::HashSet::new();
    let mut max_depth = 0;
    let mut ai_count = 0;
    let mut cursor_count = 0;
    let mut has_key_or_delay = false;

    let result = check_limits_recursive(
        &catalog,
        "A",
        &mut visited,
        1,
        &mut max_depth,
        &mut ai_count,
        &mut cursor_count,
        &mut has_key_or_delay,
    );
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("AI call limit (3) exceeded")
    );
}

#[test]
fn test_validate_dead_use_reference() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let now = crate::db::now_unix_secs();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
            "INSERT INTO triggers (id, name, trigger_type, trigger, output, action_type, target_os, is_deleted, created_at, updated_at)
             VALUES (?1, 'existing', 'word', 'existing', 'hello', 'text', 'all', 0, ?2, ?2)",
            rusqlite::params![id, now],
        ).unwrap();

    let result = validate_trigger_limits(
        &conn,
        "new_trigger",
        "greeting [use(\"nonexistent\")]",
        "text",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not exist"));
}

#[test]
fn test_validate_live_reference_passes() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let now = crate::db::now_unix_secs();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
            "INSERT INTO triggers (id, name, trigger_type, trigger, output, action_type, target_os, is_deleted, created_at, updated_at)
             VALUES (?1, 'other', 'word', 'other', 'world', 'text', 'all', 0, ?2, ?2)",
            rusqlite::params![id, now],
        ).unwrap();

    let result = validate_trigger_limits(&conn, "greeting", "hello [use(\"other\")]", "text");
    assert!(result.is_ok());
}

#[test]
fn test_validate_self_reference_is_error() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    // Self-reference is a circular reference — correctly caught by
    // check_limits_recursive before the dead-reference scan runs.
    let result = validate_trigger_limits(&conn, "greeting", "hello [use(\"greeting\")]", "text");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Circular reference")
    );
}

#[test]
fn test_validate_no_references_passes() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let result = validate_trigger_limits(&conn, "simple", "just plain text no refs", "text");
    assert!(result.is_ok());
}

#[test]
fn test_update_app_filters_trims_whitespace() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let now = crate::db::now_unix_secs();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
            "INSERT INTO triggers (id, name, trigger_type, trigger, output, action_type, target_os, is_deleted, created_at, updated_at)
             VALUES (?1, 'test', 'word', 'test', 'out', 'text', 'all', 0, ?2, ?2)",
            rusqlite::params![id, now],
        ).unwrap();

    update_trigger_app_filters(&conn, &id, Some("chrome, ".to_string()), None).unwrap();

    let stored: String = conn
        .query_row("SELECT only_apps FROM triggers WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, "chrome");
}

#[test]
fn test_update_app_filters_removes_empty() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let now = crate::db::now_unix_secs();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
            "INSERT INTO triggers (id, name, trigger_type, trigger, output, action_type, target_os, is_deleted, created_at, updated_at)
             VALUES (?1, 'test', 'word', 'test', 'out', 'text', 'all', 0, ?2, ?2)",
            rusqlite::params![id, now],
        ).unwrap();

    update_trigger_app_filters(&conn, &id, Some("chrome,,".to_string()), None).unwrap();

    let stored: String = conn
        .query_row("SELECT only_apps FROM triggers WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, "chrome");
}

#[test]
fn test_update_app_filters_rejects_unknown_prefix() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let result = update_trigger_app_filters(&conn, "fake-id", Some("foo:bar".to_string()), None);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("unknown app filter prefix")
    );
}

#[test]
fn test_update_app_filters_accepts_valid_prefixes() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let now = crate::db::now_unix_secs();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
            "INSERT INTO triggers (id, name, trigger_type, trigger, output, action_type, target_os, is_deleted, created_at, updated_at)
             VALUES (?1, 'test', 'word', 'test', 'out', 'text', 'all', 0, ?2, ?2)",
            rusqlite::params![id, now],
        ).unwrap();

    update_trigger_app_filters(
        &conn,
        &id,
        Some("exe:code, class:Chrome_WidgetWin_1".to_string()),
        None,
    )
    .unwrap();

    let stored: String = conn
        .query_row("SELECT only_apps FROM triggers WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, "exe:code,class:Chrome_WidgetWin_1");
}

#[test]
fn test_update_app_filters_none_stays_none() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let now = crate::db::now_unix_secs();
    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
            "INSERT INTO triggers (id, name, trigger_type, trigger, output, action_type, target_os, is_deleted, created_at, updated_at)
             VALUES (?1, 'test', 'word', 'test', 'out', 'text', 'all', 0, ?2, ?2)",
            rusqlite::params![id, now],
        ).unwrap();

    update_trigger_app_filters(&conn, &id, None, None).unwrap();

    let stored: Option<String> = conn
        .query_row("SELECT only_apps FROM triggers WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, None);
}

#[test]
fn test_normalize_tags_trims_and_lowercases() {
    let result = normalize_tags(r#"["  WORK ", "HeLLo"]"#).unwrap();
    assert_eq!(result, r#"["work","hello"]"#);
}

#[test]
fn test_normalize_tags_deduplicates() {
    let result = normalize_tags(r#"["a", "A", "a"]"#).unwrap();
    assert_eq!(result, r#"["a"]"#);
}

#[test]
fn test_normalize_tags_removes_empty() {
    let result = normalize_tags(r#"["a", "", "b"]"#).unwrap();
    assert_eq!(result, r#"["a","b"]"#);
}

#[test]
fn test_normalize_tags_rejects_long_tag() {
    let long_tag = "a".repeat(51);
    let json = serde_json::to_string(&vec![long_tag]).unwrap();
    let result = normalize_tags(&json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn test_normalize_tags_rejects_excessive_count() {
    let tags: Vec<String> = (0..21).map(|i| format!("tag{}", i)).collect();
    let json = serde_json::to_string(&tags).unwrap();
    let result = normalize_tags(&json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("exceeds limit of"));
}

#[test]
fn test_normalize_tags_identity_for_clean() {
    let result = normalize_tags(r#"["work","hello"]"#).unwrap();
    assert_eq!(result, r#"["work","hello"]"#);
}

#[test]
fn test_normalize_trigger_nfc() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    let nfd_e = "e\u{301}";
    let nfc_e = "\u{e9}";
    let id = create_trigger(
        &mut conn,
        NewTrigger {
            name: Some("accent"),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: nfd_e,
            content: nfc_e,
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            interpreter: None,
            behavior: None,
        },
    )
    .unwrap();

    let (stored_trigger, stored_output): (String, String) = conn
        .query_row(
            "SELECT trigger, output FROM triggers WHERE id = ?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_trigger, nfc_e, "trigger should be NFC-normalized");
    assert_eq!(stored_output, nfc_e, "output should be NFC-normalized");
}

#[test]
fn test_add_trigger_by_type_normalizes_nfc() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let nfd_e = "e\u{301}";
    let nfc_e = "\u{e9}";
    add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        nfd_e,
        nfc_e,
        "all",
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();

    let (stored_trigger, stored_output): (String, String) = conn
        .query_row(
            "SELECT trigger, output FROM triggers WHERE trigger = ?1",
            [nfc_e],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_trigger, nfc_e, "trigger should be NFC-normalized");
    assert_eq!(stored_output, nfc_e, "output should be NFC-normalized");
}

#[test]
fn test_add_trigger_by_type_rejects_dead_ref() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let result = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "hello",
        "greeting [use(\"nonexistent\")]",
        "all",
        None,
        None,
        None,
        None,
        None,
        false,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not exist"));
}

#[test]
fn test_add_trigger_with_name_and_description() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "greeting",
        "hello",
        "all",
        None,
        None,
        None,
        Some("My Greeting"),
        Some("A friendly salutation"),
        false,
    )
    .unwrap();

    let (stored_name, stored_desc): (String, Option<String>) = conn
        .query_row(
            "SELECT name, description FROM triggers WHERE trigger = 'greeting'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored_name, "My Greeting");
    assert_eq!(stored_desc.as_deref(), Some("A friendly salutation"));
}

#[test]
fn test_update_name_and_description_on_re_add() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    // First add — no custom name/description
    add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "greeting2",
        "hello",
        "all",
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();

    let (name1, desc1): (String, Option<String>) = conn
        .query_row(
            "SELECT name, description FROM triggers WHERE trigger = 'greeting2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name1, "greeting2"); // Defaults to trigger
    assert_eq!(desc1, None);

    // Re-add with same output but custom name/description
    let outcome = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "greeting2",
        "hello",
        "all",
        None,
        None,
        None,
        Some("Updated Name"),
        Some("Now has a description"),
        false,
    )
    .unwrap();
    assert_eq!(outcome, AddOutcome::Updated);

    let (name2, desc2): (String, Option<String>) = conn
        .query_row(
            "SELECT name, description FROM triggers WHERE trigger = 'greeting2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name2, "Updated Name");
    assert_eq!(desc2.as_deref(), Some("Now has a description"));

    // Re-add with different output + name override
    let outcome2 = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "greeting2",
        "bonjour",
        "all",
        None,
        None,
        None,
        Some("French Greeting"),
        None,
        false,
    )
    .unwrap();
    assert_eq!(outcome2, AddOutcome::Updated);

    let (name3, desc3, output3): (String, Option<String>, String) = conn
        .query_row(
            "SELECT name, description, output FROM triggers WHERE trigger = 'greeting2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(name3, "French Greeting");
    assert_eq!(desc3.as_deref(), Some("Now has a description")); // Preserved from before
    assert_eq!(output3, "bonjour");
}

#[test]
fn test_add_trigger_by_type_with_case_rejects_long_name() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let long_name = "a".repeat(201);
    let result = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "len_test",
        "out",
        "all",
        None,
        None,
        None,
        Some(&long_name),
        None,
        false,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn test_add_trigger_by_type_with_case_rejects_long_description() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    let long_desc = "a".repeat(1001);
    let result = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "len_test2",
        "out",
        "all",
        None,
        None,
        None,
        Some("ok"),
        Some(&long_desc),
        false,
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("character limit"));
}

#[test]
fn test_re_add_same_output_no_name_returns_already_exists() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();

    add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "foo",
        "bar",
        "all",
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();

    let outcome = add_trigger_by_type_with_case(
        &conn,
        TriggerType::Word,
        "foo",
        "bar",
        "all",
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .unwrap();
    assert_eq!(outcome, AddOutcome::AlreadyExists);
}

#[test]
fn test_duplicate_name_warns() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    create_trigger(
        &mut conn,
        NewTrigger {
            name: Some("Duplicate"),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "first",
            content: "hello",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            interpreter: None,
            behavior: None,
        },
    )
    .unwrap();

    // Second trigger with same name should succeed (warning, not error)
    let id = create_trigger(
        &mut conn,
        NewTrigger {
            name: Some("Duplicate"),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "second",
            content: "world",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            interpreter: None,
            behavior: None,
        },
    )
    .unwrap();

    assert!(!id.is_empty());
}

#[test]
fn test_duplicate_name_warn_update_excludes_self() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, mut conn) = crate::testing::open_test_db();

    let id = create_trigger(
        &mut conn,
        NewTrigger {
            name: Some("Unique"),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "uniq",
            content: "hello",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            interpreter: None,
            behavior: None,
        },
    )
    .unwrap();

    // Updating the same trigger to keep its own name should not warn
    update_existing_trigger(
        &mut conn,
        ExistingTriggerUpdate {
            id: &id,
            name: "Unique",
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "uniq",
            content: "world",
            action_type: "text",
            target_os: "all",
            tags_json: "[]",
            auto_case: false,
            usage_count: 0,
            last_used_at: None,
            interpreter: None,
            behavior: None,
        },
    )
    .unwrap();
}

fn create_test_trigger(conn: &Connection) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let now = crate::db::now_unix_secs();
    conn.execute(
            "INSERT INTO triggers (id, name, trigger, output, action_type, trigger_type, target_os, is_deleted, created_at, updated_at)
             VALUES (?1, 'test', 't', 'o', 'text', 'word', 'all', 0, ?2, ?2)",
            rusqlite::params![id, now],
        ).unwrap();
    id
}

#[test]
fn app_filter_comma_in_title_value_is_preserved() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();
    let id = create_test_trigger(&conn);

    update_trigger_app_filters(
        &conn,
        &id,
        Some(String::from(r"title:Hello\, World,exe:notepad.exe")),
        None,
    )
    .unwrap();

    let only: String = conn
        .query_row(
            "SELECT only_apps FROM triggers WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        only, "title:Hello, World,exe:notepad.exe",
        "comma in title value should be preserved in stored value"
    );
}

#[test]
fn app_filter_trailing_backslash_is_preserved() {
    let _guard = crate::testing::TEST_LOCK.lock().unwrap();
    let (_dir, conn) = crate::testing::open_test_db();
    let id = create_test_trigger(&conn);

    update_trigger_app_filters(
        &conn,
        &id,
        Some(String::from(r"exe:test\,path\,with\,commas")),
        None,
    )
    .unwrap();

    let only: String = conn
        .query_row(
            "SELECT only_apps FROM triggers WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(only, "exe:test,path,with,commas");
}

#[test]
fn test_app_filters_overlap_logic() {
    // No filters -> overlaps
    assert!(app_filters_overlap(None, None, None, None));

    // One only_apps vs no filters -> overlaps
    assert!(app_filters_overlap(Some("exe:code"), None, None, None));
    assert!(app_filters_overlap(None, None, Some("exe:notepad"), None));

    // Non-intersecting only_apps -> no overlap
    assert!(!app_filters_overlap(
        Some("exe:code"),
        None,
        Some("exe:notepad"),
        None
    ));

    // Intersecting only_apps -> overlaps
    assert!(app_filters_overlap(
        Some("exe:code,exe:notepad"),
        None,
        Some("exe:notepad"),
        None
    ));

    // Non-intersecting only_apps vs except_apps -> overlaps
    assert!(app_filters_overlap(
        Some("exe:code"),
        None,
        None,
        Some("exe:notepad")
    ));

    // Intersecting only_apps vs except_apps -> no overlap (if only_a is in except_b)
    assert!(!app_filters_overlap(
        Some("exe:code"),
        None,
        None,
        Some("exe:code")
    ));
}
