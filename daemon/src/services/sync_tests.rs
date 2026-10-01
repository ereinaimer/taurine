use rusqlite::Connection;
use taurine_core::cloud::CloudClient;
use taurine_core::cloud::types::CloudConfig;
use taurine_core::db::crud::triggers::{
    CloudSnippetPayload, InvocationType, ReconcileOutcome, TriggerAliasRow, TriggerRow,
    list_aliases, purge_deleted_tombstones, reconcile_cloud_snippet,
    serialize_trigger_to_cloud_snippet,
};
use taurine_core::db::init::migrate::run_migrations;

use super::sync_worker::{SyncStatus, SyncWorker};

#[test]
fn test_sync_serializes_multi_trigger_aliases() {
    let trigger = TriggerRow {
        id: "trig-123".to_string(),
        workspace_id: "default".to_string(),
        name: "My Snippet".to_string(),
        output: "Hello World".to_string(),
        description: Some("Description here".to_string()),
        action_type: "text".to_string(),
        target_os: "all".to_string(),
        only_apps: Some("slack".to_string()),
        except_apps: None,
        tags: "[\"tag1\",\"tag2\"]".to_string(),
        is_enabled: true,
        auto_case: false,
        version: 3,
        is_deleted: false,
        updated_at: 1730002000,
        created_at: 1730001000,
        ..Default::default()
    };

    let aliases = vec![
        TriggerAliasRow {
            id: "a1".to_string(),
            trigger_id: "trig-123".to_string(),
            invocation: "hw".to_string(),
            invocation_type: InvocationType::Word,
            require_confirmation: false,
            strict_threshold: None,
            created_at: 1730001000,
        },
        TriggerAliasRow {
            id: "a2".to_string(),
            trigger_id: "trig-123".to_string(),
            invocation: "hello world".to_string(),
            invocation_type: InvocationType::Voice,
            require_confirmation: true,
            strict_threshold: Some(0.85),
            created_at: 1730001100,
        },
        TriggerAliasRow {
            id: "a3".to_string(),
            trigger_id: "trig-123".to_string(),
            invocation: "ctrl+shift+h".to_string(),
            invocation_type: InvocationType::Hotkey,
            require_confirmation: false,
            strict_threshold: None,
            created_at: 1730001200,
        },
    ];

    let payload = serialize_trigger_to_cloud_snippet(&trigger, &aliases);

    assert_eq!(payload.id, "trig-123");
    assert_eq!(payload.workspace_id, "default");
    assert_eq!(payload.name, "My Snippet");
    assert_eq!(payload.output, "Hello World");
    assert_eq!(payload.description, Some("Description here".to_string()));
    assert_eq!(payload.action_type, "text");
    assert_eq!(payload.target_os, Some("all".to_string()));
    assert_eq!(payload.only_apps, Some("slack".to_string()));
    assert_eq!(payload.except_apps, None);
    assert_eq!(payload.tags, Some("[\"tag1\",\"tag2\"]".to_string()));
    assert!(payload.is_enabled);
    assert!(!payload.auto_case);
    assert_eq!(payload.version, 3);
    assert!(!payload.is_deleted);
    assert_eq!(payload.updated_at, 1730002000);
    assert_eq!(payload.created_at, 1730001000);
    assert_eq!(payload.aliases.len(), 3);
    assert_eq!(payload.aliases[0].invocation, "hw");
    assert_eq!(payload.aliases[0].invocation_type, InvocationType::Word);
    assert_eq!(payload.aliases[1].invocation, "hello world");
    assert_eq!(payload.aliases[1].invocation_type, InvocationType::Voice);
    assert_eq!(payload.aliases[1].strict_threshold, Some(0.85));
    assert_eq!(payload.aliases[2].invocation, "ctrl+shift+h");
    assert_eq!(payload.aliases[2].invocation_type, InvocationType::Hotkey);

    // Verify JSON serialization round-trip
    let json = serde_json::to_string(&payload).unwrap();
    let deserialized: CloudSnippetPayload = serde_json::from_str(&json).unwrap();
    assert_eq!(payload, deserialized);
}

#[test]
fn test_lww_remote_newer_overwrites_local_and_aliases() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let trigger_id = "test-trig-1";
    // Insert local trigger at version 1
    conn.execute(
        "INSERT INTO triggers (
            id, name, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES (?1, 'Old Name', 'Old Desc', 'old output', 'text', 'all', NULL, NULL,
                  '[]', 1, 0, 1, 0, 1, 1000, 1000)",
        [trigger_id],
    )
    .unwrap();

    // Insert local alias
    conn.execute(
        "INSERT INTO trigger_aliases (
            id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold, created_at
        ) VALUES ('alias-old', ?1, 'old_trigger', 'word', 0, NULL, 1000)",
        [trigger_id],
    )
    .unwrap();

    // Construct remote snippet with version 2, updated_at 2000 (newer)
    let remote_snippet = CloudSnippetPayload {
        id: trigger_id.to_string(),
        workspace_id: "default".to_string(),
        name: "Remote Name".to_string(),
        output: "new remote output".to_string(),
        description: Some("New Desc".to_string()),
        action_type: "text".to_string(),
        target_os: Some("win".to_string()),
        only_apps: Some("notepad".to_string()),
        except_apps: None,
        tags: Some("[\"remote\"]".to_string()),
        is_enabled: true,
        auto_case: true,
        version: 2,
        is_deleted: false,
        updated_at: 2000,
        created_at: 1000,
        aliases: vec![
            TriggerAliasRow {
                id: "alias-new-1".to_string(),
                trigger_id: trigger_id.to_string(),
                invocation: "new_word".to_string(),
                invocation_type: InvocationType::Word,
                require_confirmation: false,
                strict_threshold: None,
                created_at: 2000,
            },
            TriggerAliasRow {
                id: "alias-new-2".to_string(),
                trigger_id: trigger_id.to_string(),
                invocation: "ctrl+j".to_string(),
                invocation_type: InvocationType::Hotkey,
                require_confirmation: false,
                strict_threshold: None,
                created_at: 2000,
            },
        ],
    };

    let outcome = reconcile_cloud_snippet(&conn, &remote_snippet).unwrap();
    assert_eq!(outcome, ReconcileOutcome::UpdatedFromRemote);

    // Verify trigger row updated
    let (name, output, version, updated_at, auto_case): (String, String, i64, i64, bool) = conn
        .query_row(
            "SELECT name, output, version, updated_at, auto_case FROM triggers WHERE id = ?1",
            [trigger_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(name, "Remote Name");
    assert_eq!(output, "new remote output");
    assert_eq!(version, 2);
    assert_eq!(updated_at, 2000);
    assert!(auto_case);

    // Verify old alias deleted and new aliases present
    let aliases = list_aliases(&conn, trigger_id).unwrap();
    assert_eq!(aliases.len(), 2);
    assert!(
        aliases
            .iter()
            .any(|a| a.invocation == "new_word" && a.invocation_type == InvocationType::Word)
    );
    assert!(
        aliases
            .iter()
            .any(|a| a.invocation == "ctrl+j" && a.invocation_type == InvocationType::Hotkey)
    );
    assert!(!aliases.iter().any(|a| a.invocation == "old_trigger"));
}

#[test]
fn test_lww_local_newer_preserved() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let trigger_id = "test-trig-2";
    // Local trigger at version 3, updated_at 3000
    conn.execute(
        "INSERT INTO triggers (
            id, name, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES (?1, 'Local New Name', NULL, 'local output', 'text', 'all', NULL, NULL,
                  '[]', 1, 0, 3, 0, 0, 1000, 3000)",
        [trigger_id],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO trigger_aliases (
            id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold, created_at
        ) VALUES ('alias-local', ?1, 'local_inv', 'word', 0, NULL, 1000)",
        [trigger_id],
    )
    .unwrap();

    // Remote snippet has older version 2, updated_at 2000
    let remote_snippet = CloudSnippetPayload {
        id: trigger_id.to_string(),
        workspace_id: "default".to_string(),
        name: "Remote Stale Name".to_string(),
        output: "stale output".to_string(),
        description: None,
        action_type: "text".to_string(),
        target_os: Some("all".to_string()),
        only_apps: None,
        except_apps: None,
        tags: None,
        is_enabled: true,
        auto_case: false,
        version: 2,
        is_deleted: false,
        updated_at: 2000,
        created_at: 1000,
        aliases: vec![TriggerAliasRow {
            id: "alias-remote".to_string(),
            trigger_id: trigger_id.to_string(),
            invocation: "remote_inv".to_string(),
            invocation_type: InvocationType::Word,
            require_confirmation: false,
            strict_threshold: None,
            created_at: 2000,
        }],
    };

    let outcome = reconcile_cloud_snippet(&conn, &remote_snippet).unwrap();
    assert_eq!(outcome, ReconcileOutcome::LocalNewer);

    // Local trigger preserved
    let (name, output, version): (String, String, i64) = conn
        .query_row(
            "SELECT name, output, version FROM triggers WHERE id = ?1",
            [trigger_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(name, "Local New Name");
    assert_eq!(output, "local output");
    assert_eq!(version, 3);

    // Local alias preserved
    let aliases = list_aliases(&conn, trigger_id).unwrap();
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0].invocation, "local_inv");
}

#[test]
fn test_sync_reconciles_soft_delete_tombstone() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let trigger_id = "test-trig-3";
    // Local active trigger at version 1, updated_at 1000, is_deleted = 0
    conn.execute(
        "INSERT INTO triggers (
            id, name, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES (?1, 'Active Snippet', NULL, 'content', 'text', 'all', NULL, NULL,
                  '[]', 1, 0, 1, 0, 1, 1000, 1000)",
        [trigger_id],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO trigger_aliases (
            id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold, created_at
        ) VALUES ('alias-3', ?1, 'tomb_me', 'word', 0, NULL, 1000)",
        [trigger_id],
    )
    .unwrap();

    // Remote soft-deleted tombstone at version 2, updated_at 2500, is_deleted = true, empty aliases
    let remote_snippet = CloudSnippetPayload {
        id: trigger_id.to_string(),
        workspace_id: "default".to_string(),
        name: "Active Snippet".to_string(),
        output: "content".to_string(),
        description: None,
        action_type: "text".to_string(),
        target_os: Some("all".to_string()),
        only_apps: None,
        except_apps: None,
        tags: None,
        is_enabled: true,
        auto_case: false,
        version: 2,
        is_deleted: true,
        updated_at: 2500,
        created_at: 1000,
        aliases: vec![],
    };

    let outcome = reconcile_cloud_snippet(&conn, &remote_snippet).unwrap();
    assert_eq!(outcome, ReconcileOutcome::UpdatedFromRemote);

    // Verify local row is soft-deleted
    let (is_deleted, version): (bool, i64) = conn
        .query_row(
            "SELECT is_deleted, version FROM triggers WHERE id = ?1",
            [trigger_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(is_deleted);
    assert_eq!(version, 2);

    // Aliases cleared
    let aliases = list_aliases(&conn, trigger_id).unwrap();
    assert_eq!(aliases.len(), 0);
}

#[test]
fn test_tombstone_gc_purges_old_deleted_rows() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let old_epoch = now - (40 * 86400); // 40 days ago
    let recent_epoch = now - (5 * 86400); // 5 days ago

    // 1. Soft-deleted row older than 30 days
    conn.execute(
        "INSERT INTO triggers (
            id, name, output, action_type, target_os, tags,
            is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('tomb-old', 'Old Deleted', '', 'text', 'all', '[]',
                  1, 0, 2, 1, 1, ?1, ?1)",
        [old_epoch],
    )
    .unwrap();

    // 2. Soft-deleted row recent (5 days ago)
    conn.execute(
        "INSERT INTO triggers (
            id, name, output, action_type, target_os, tags,
            is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('tomb-recent', 'Recent Deleted', '', 'text', 'all', '[]',
                  1, 0, 2, 1, 1, ?1, ?1)",
        [recent_epoch],
    )
    .unwrap();

    // 3. Active row old (40 days ago) - should NOT be purged
    conn.execute(
        "INSERT INTO triggers (
            id, name, output, action_type, target_os, tags,
            is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('active-old', 'Active Old', 'out', 'text', 'all', '[]',
                  1, 0, 1, 0, 1, ?1, ?1)",
        [old_epoch],
    )
    .unwrap();

    // Purge rows older than 30 days
    let purged = purge_deleted_tombstones(&conn, 30 * 86400).unwrap();
    assert_eq!(purged, 1);

    // Check presence
    let old_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM triggers WHERE id = 'tomb-old')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!old_exists, "tomb-old should be permanently purged");

    let recent_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM triggers WHERE id = 'tomb-recent')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(recent_exists, "tomb-recent should be kept");

    let active_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM triggers WHERE id = 'active-old')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(active_exists, "active-old should be kept");
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_sync_worker_skips_when_unauthenticated() {
    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    // Test with None client
    let status_no_client = SyncWorker::run_sync_cycle(&conn, None).await;
    assert_eq!(status_no_client, SyncStatus::NotAuthenticated);

    // Test with client when keystore has no tokens
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::hook::tests::mock_keystore::use_mock_keystore();
    let _ = taurine_core::cloud::clear_tokens();

    let config = CloudConfig {
        supabase_url: "https://mock.supabase.co".to_string(),
        anon_key: "mock-key".to_string(),
    };
    let client = CloudClient::new(config);

    let status_no_tokens = SyncWorker::run_sync_cycle(&conn, Some(&client)).await;
    assert_eq!(status_no_tokens, SyncStatus::NotAuthenticated);
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_sync_worker_authenticated_two_way_cycle() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::hook::tests::mock_keystore::use_mock_keystore();

    let tokens = taurine_core::cloud::AuthTokens {
        access_token: "mock-token-xyz".to_string(),
        refresh_token: "mock-refresh".to_string(),
        user_id: "user-1".to_string(),
        expires_at: Some(2000000000),
    };
    taurine_core::cloud::store_tokens(&tokens).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();

    // Insert an unsynced local trigger (is_synced = 0)
    conn.execute(
        "INSERT INTO triggers (
            id, name, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('local-push-1', 'Push Me', NULL, 'pushed output', 'text', 'all', NULL, NULL,
                  '[]', 1, 0, 1, 0, 0, 1000, 1000)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO trigger_aliases (
            id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold, created_at
        ) VALUES ('alias-p1', 'local-push-1', 'pushme', 'word', 0, NULL, 1000)",
        [],
    )
    .unwrap();

    // Start mock HTTP server
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            use std::io::{Read, Write};
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.starts_with("GET /rest/v1/snippets") {
                let remote_snippet = CloudSnippetPayload {
                    id: "pulled-from-cloud".to_string(),
                    workspace_id: "default".to_string(),
                    name: "Pulled Cloud Trigger".to_string(),
                    output: "cloud output".to_string(),
                    description: None,
                    action_type: "text".to_string(),
                    target_os: Some("all".to_string()),
                    only_apps: None,
                    except_apps: None,
                    tags: None,
                    is_enabled: true,
                    auto_case: false,
                    version: 1,
                    is_deleted: false,
                    updated_at: 1500,
                    created_at: 1500,
                    aliases: vec![TriggerAliasRow {
                        id: "alias-cloud-1".to_string(),
                        trigger_id: "pulled-from-cloud".to_string(),
                        invocation: "fromcloud".to_string(),
                        invocation_type: InvocationType::Word,
                        require_confirmation: false,
                        strict_threshold: None,
                        created_at: 1500,
                    }],
                };
                let body = serde_json::to_string(&vec![remote_snippet]).unwrap();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(resp.as_bytes()).unwrap();
            } else if req.starts_with("POST /rest/v1/snippets") {
                assert!(req.contains("local-push-1"));
                let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                stream.write_all(resp.as_bytes()).unwrap();
            }
        }
    });

    let config = CloudConfig {
        supabase_url: format!("http://127.0.0.1:{port}"),
        anon_key: "test-anon-key".to_string(),
    };
    let client = CloudClient::new(config);

    let status = SyncWorker::run_sync_cycle(&conn, Some(&client)).await;
    assert_eq!(
        status,
        SyncStatus::Success {
            pulled_snippets: 1,
            pushed_snippets: 1,
        }
    );

    // Verify pulled snippet was inserted locally
    let pulled_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM triggers WHERE id = 'pulled-from-cloud')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(pulled_exists);

    // Verify pushed local trigger is now marked synced (is_synced = 1)
    let is_synced: bool = conn
        .query_row(
            "SELECT is_synced FROM triggers WHERE id = 'local-push-1'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(is_synced);

    server_handle.join().unwrap();
    taurine_core::cloud::clear_tokens().unwrap();
}

#[tokio::test]
async fn test_sync_worker_flushes_quota_to_db() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::hook::tests::mock_keystore::use_mock_keystore();

    let conn = Connection::open_in_memory().unwrap();
    run_migrations(&conn).unwrap();
    let key = [42u8; 32];
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    taurine_core::db::crud::quota::get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap();

    let guard = crate::engine::quota_guard::QuotaGuard::global();
    guard.set_balance(100.0);
    assert!(guard.deplete(crate::engine::quota_guard::ExpansionType::Text));
    assert!(guard.unflushed_delta() > 0.0);

    // Flush directly using the guard to verify flush works with connection
    guard
        .flush_to_db(&conn, &key)
        .expect("flush should succeed");

    assert_eq!(guard.unflushed_delta(), 0.0);
    let row =
        taurine_core::db::crud::quota::get_or_init_quota_ledger(&conn, &key, now_epoch).unwrap();
    assert!((row.remaining_percentage - 99.70).abs() < 1e-6);
}
