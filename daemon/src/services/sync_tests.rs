use rusqlite::Connection;
use taurine_core::cloud::CloudClient;
use taurine_core::cloud::types::CloudConfig;
use taurine_core::db::crud::triggers::{
    CloudSnippetPayload, InvocationType, ReconcileOutcome, TriggerAliasRow, TriggerRow,
    list_aliases, purge_deleted_tombstones, reconcile_cloud_snippet,
    serialize_trigger_to_cloud_snippet,
};
use taurine_core::db::crud::{
    CloudActivityEventPayload, CloudTriggerDailyStatPayload, CloudUserAppStatPayload,
    CloudUserStatPayload,
};
use taurine_core::db::init::ensure_schema;

use super::sync_worker::{SyncStatus, SyncWorker};

#[test]
fn test_sync_serializes_multi_trigger_aliases() {
    let trigger = TriggerRow {
        id: "trig-123".to_string(),
        workspace_id: "default".to_string(),
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
    assert_eq!(payload.name, "Description here");
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
    ensure_schema(&conn).unwrap();

    let trigger_id = "test-trig-1";
    // Insert local trigger at version 1
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES (?1, 'Old Desc', 'old output', 'text', 'all', NULL, NULL,
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
    let (description, output, version, updated_at, auto_case): (Option<String>, String, i64, i64, bool) = conn
        .query_row(
            "SELECT description, output, version, updated_at, auto_case FROM triggers WHERE id = ?1",
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

    assert_eq!(description.as_deref(), Some("New Desc"));
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
    ensure_schema(&conn).unwrap();

    let trigger_id = "test-trig-2";
    // Local trigger at version 3, updated_at 3000
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES (?1, 'Local New Name', 'local output', 'text', 'all', NULL, NULL,
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
    let (description, output, version): (Option<String>, String, i64) = conn
        .query_row(
            "SELECT description, output, version FROM triggers WHERE id = ?1",
            [trigger_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(description.as_deref(), Some("Local New Name"));
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
    ensure_schema(&conn).unwrap();

    let trigger_id = "test-trig-3";
    // Local active trigger at version 1, updated_at 1000, is_deleted = 0
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES (?1, 'Active Snippet', 'content', 'text', 'all', NULL, NULL,
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
    ensure_schema(&conn).unwrap();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let old_epoch = now - (40 * 86400); // 40 days ago
    let recent_epoch = now - (5 * 86400); // 5 days ago

    // 1. Soft-deleted row older than 30 days
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, tags,
            is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('tomb-old', 'Old Deleted', '', 'text', 'all', '[]',
                  1, 0, 2, 1, 1, ?1, ?1)",
        [old_epoch],
    )
    .unwrap();

    // 2. Soft-deleted row recent (5 days ago)
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, tags,
            is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('tomb-recent', 'Recent Deleted', '', 'text', 'all', '[]',
                  1, 0, 2, 1, 1, ?1, ?1)",
        [recent_epoch],
    )
    .unwrap();

    // 3. Active row old (40 days ago) - should NOT be purged
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, tags,
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
    ensure_schema(&conn).unwrap();

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
    ensure_schema(&conn).unwrap();

    // Insert an unsynced local trigger (is_synced = 0)
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('local-push-1', 'Push Me', 'pushed output', 'text', 'all', NULL, NULL,
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
        for _ in 0..7 {
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
            } else if req.starts_with("GET /rest/v1/") {
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
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
            pulled_settings: 0,
            pushed_settings: 0,
            pulled_events: 0,
            pushed_stats: 0,
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
    ensure_schema(&conn).unwrap();
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

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_sync_worker_proactive_token_refresh() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::hook::tests::mock_keystore::use_mock_keystore();

    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let initial_tokens = taurine_core::cloud::AuthTokens {
        access_token: "expired-access-token".to_string(),
        refresh_token: "valid-refresh-token".to_string(),
        user_id: "user-refresh-1".to_string(),
        expires_at: Some(now_epoch - 100),
    };
    taurine_core::cloud::store_tokens(&initial_tokens).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        for _ in 0..7 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.starts_with("POST /auth/v1/token?grant_type=refresh_token") {
                assert!(req.contains("valid-refresh-token"));
                let body = serde_json::json!({
                    "access_token": "fresh-access-proactive",
                    "refresh_token": "next-refresh-proactive",
                    "expires_at": now_epoch + 3600,
                    "user": {
                        "id": "user-refresh-1"
                    }
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(resp.as_bytes()).unwrap();
            } else if req.starts_with("GET /rest/v1/snippets") {
                assert!(req.contains("Bearer fresh-access-proactive"));
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
                stream.write_all(resp.as_bytes()).unwrap();
            } else if req.starts_with("GET /rest/v1/") {
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
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
            pulled_snippets: 0,
            pushed_snippets: 0,
            pulled_settings: 0,
            pushed_settings: 0,
            pulled_events: 0,
            pushed_stats: 0,
        }
    );

    let saved_tokens = taurine_core::cloud::get_tokens().unwrap().unwrap();
    assert_eq!(saved_tokens.access_token, "fresh-access-proactive");
    assert_eq!(saved_tokens.refresh_token, "next-refresh-proactive");

    server_handle.join().unwrap();
    taurine_core::cloud::clear_tokens().unwrap();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_sync_worker_reactive_refresh_on_401() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::hook::tests::mock_keystore::use_mock_keystore();

    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let initial_tokens = taurine_core::cloud::AuthTokens {
        access_token: "invalid-or-revoked-token".to_string(),
        refresh_token: "valid-reactive-refresh".to_string(),
        user_id: "user-reactive-1".to_string(),
        expires_at: Some(now_epoch + 3600),
    };
    taurine_core::cloud::store_tokens(&initial_tokens).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        for _ in 0..8 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.starts_with("GET /rest/v1/snippets") && req.contains("invalid-or-revoked-token")
            {
                let err_body = r#"{"code":"PGRST303","message":"JWT expired"}"#;
                let resp = format!(
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    err_body.len(),
                    err_body
                );
                stream.write_all(resp.as_bytes()).unwrap();
            } else if req.starts_with("POST /auth/v1/token?grant_type=refresh_token") {
                assert!(req.contains("valid-reactive-refresh"));
                let body = serde_json::json!({
                    "access_token": "fresh-access-reactive",
                    "refresh_token": "next-refresh-reactive",
                    "expires_at": now_epoch + 3600,
                    "user": {
                        "id": "user-reactive-1"
                    }
                })
                .to_string();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(resp.as_bytes()).unwrap();
            } else if (req.starts_with("GET /rest/v1/snippets")
                && req.contains("fresh-access-reactive"))
                || req.starts_with("GET /rest/v1/")
            {
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
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
            pulled_snippets: 0,
            pushed_snippets: 0,
            pulled_settings: 0,
            pushed_settings: 0,
            pulled_events: 0,
            pushed_stats: 0,
        }
    );

    let saved_tokens = taurine_core::cloud::get_tokens().unwrap().unwrap();
    assert_eq!(saved_tokens.access_token, "fresh-access-reactive");
    assert_eq!(saved_tokens.refresh_token, "next-refresh-reactive");

    server_handle.join().unwrap();
    taurine_core::cloud::clear_tokens().unwrap();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_sync_settings_with_strict_allowlist_ignores_sensitive_and_hardware_keys() {
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
    ensure_schema(&conn).unwrap();

    // Inserts local wpm (syncable), voice_input_device (hardware), and scripts_enabled (security)
    taurine_core::db::crud::upsert_setting(&conn, "wpm", "120").unwrap();
    taurine_core::db::crud::upsert_setting(&conn, "voice_input_device", "\"Realtek Mic\"").unwrap();
    taurine_core::db::crud::upsert_setting(&conn, "scripts_enabled", "true").unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let start = std::time::Instant::now();
        let mut handled = 0;
        while handled < 7 && start.elapsed() < std::time::Duration::from_secs(3) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut buf = [0u8; 4096];
                    let n = match stream.read(&mut buf) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /rest/v1/snippets") {
                        let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("GET /rest/v1/user_settings") {
                        // Remote response has one syncable setting and two forbidden hardware/security settings
                        let remote_settings = vec![
                            taurine_core::db::crud::CloudSettingPayload {
                                key: "audio_volume".to_string(),
                                value: serde_json::json!(75),
                                version: 2,
                                updated_at: Some("2026-10-05T10:00:00Z".to_string()),
                            },
                            taurine_core::db::crud::CloudSettingPayload {
                                key: "voice_input_device".to_string(),
                                value: serde_json::json!("Remote Hardware Mic"),
                                version: 10,
                                updated_at: Some("2026-10-05T10:00:00Z".to_string()),
                            },
                            taurine_core::db::crud::CloudSettingPayload {
                                key: "scripts_enabled".to_string(),
                                value: serde_json::json!(false),
                                version: 10,
                                updated_at: Some("2026-10-05T10:00:00Z".to_string()),
                            },
                        ];
                        let body = serde_json::to_string(&remote_settings).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/user_settings") {
                        assert!(req.contains("resolution=merge-duplicates"));
                        assert!(req.contains("\"wpm\""));
                        assert!(!req.contains("voice_input_device"));
                        assert!(!req.contains("scripts_enabled"));

                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("GET /rest/v1/") {
                        let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
                        stream.write_all(resp.as_bytes()).unwrap();
                    }
                    handled += 1;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
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
            pulled_snippets: 0,
            pushed_snippets: 0,
            pulled_settings: 1,
            pushed_settings: 1,
            pulled_events: 0,
            pushed_stats: 0,
        }
    );

    // Verify sensitive and hardware keys were NOT updated from remote
    let dev = taurine_core::db::crud::get_setting(&conn, "voice_input_device")
        .unwrap()
        .unwrap();
    assert_eq!(dev.value, "\"Realtek Mic\"");

    let scripts = taurine_core::db::crud::get_setting(&conn, "scripts_enabled")
        .unwrap()
        .unwrap();
    assert_eq!(scripts.value, "true");

    // Verify syncable setting was updated from remote
    let vol = taurine_core::db::crud::get_setting(&conn, "audio_volume")
        .unwrap()
        .unwrap();
    assert_eq!(vol.value, "75");

    // Verify local wpm is marked synced
    let wpm = taurine_core::db::crud::get_setting(&conn, "wpm")
        .unwrap()
        .unwrap();
    assert!(wpm.is_synced);

    let _ = server_handle.join();
    taurine_core::cloud::clear_tokens().unwrap();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_sync_trigger_daily_stats_and_global_stats_upload() {
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
    ensure_schema(&conn).unwrap();

    // Insert dummy trigger for foreign key reference
    conn.execute(
        "INSERT INTO triggers (id, description, output, created_at, updated_at) VALUES ('trig-stat-1', 'test_trig', 'out', 100, 100)",
        [],
    )
    .unwrap();

    // Insert unsynced trigger_daily_stats
    taurine_core::db::crud::stats::upsert_trigger_daily_stat_with_conn(
        &conn,
        "trig-stat-1",
        "2026-10-05",
        "code.exe",
        5,
        0,
        100,
        2500,
    )
    .unwrap();

    // Insert unsynced global stats
    taurine_core::db::crud::increment_stat(
        &conn,
        "2026-10-05",
        &taurine_core::db::crud::StatDeltas {
            executions: 5,
            words_expanded: 100,
            time_saved_ms: 2500,
            ..Default::default()
        },
    )
    .unwrap();

    // Insert unsynced app stats
    taurine_core::db::crud::upsert_app_stat_with_conn(
        &conn,
        "code.exe",
        "2026-10-05",
        5,
        0,
        100,
        2500,
    )
    .unwrap();

    // Insert unsynced activity event
    taurine_core::db::crud::insert_activity_event(
        &conn,
        &taurine_core::db::crud::NewActivityEvent {
            id: Some("evt-sync-1".to_string()),
            trigger_id: Some("trig-stat-1".to_string()),
            timestamp: Some(1728123456000),
            date: Some("2026-10-05".to_string()),
            category: taurine_core::db::crud::ActivityCategory::Text,
            app_key: Some("code.exe".to_string()),
            word_count: 20,
            time_saved_ms: 500,
            content: None,
        },
    )
    .unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let start = std::time::Instant::now();
        let mut handled = 0;
        while handled < 10 && start.elapsed() < std::time::Duration::from_secs(3) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut buf = [0u8; 4096];
                    let n = match stream.read(&mut buf) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /rest/v1/") {
                        let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/trigger_daily_stats") {
                        assert!(req.contains("resolution=merge-duplicates"));
                        assert!(req.contains("trig-stat-1"));
                        assert!(req.contains("code.exe"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/user_stats") {
                        assert!(req.contains("resolution=merge-duplicates"));
                        assert!(req.contains("2026-10-05"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/user_app_stats") {
                        assert!(req.contains("resolution=merge-duplicates"));
                        assert!(req.contains("code.exe"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/activity_events") {
                        assert!(req.contains("resolution=merge-duplicates"));
                        assert!(req.contains("evt-sync-1"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    }
                    handled += 1;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
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
            pulled_snippets: 0,
            pushed_snippets: 0,
            pulled_settings: 0,
            pushed_settings: 0,
            pulled_events: 0,
            pushed_stats: 4,
        }
    );

    // Verify all local rows are marked is_synced = 1
    assert!(
        taurine_core::db::crud::get_unsynced_trigger_daily_stats(&conn, 10)
            .unwrap()
            .is_empty(),
        "trigger_daily_stats must be marked synced"
    );
    assert!(
        taurine_core::db::crud::get_unsynced_stats(&conn, 10)
            .unwrap()
            .is_empty(),
        "stats must be marked synced"
    );
    assert!(
        taurine_core::db::crud::get_unsynced_app_stats(&conn, 10)
            .unwrap()
            .is_empty(),
        "app_stats must be marked synced"
    );
    assert!(
        taurine_core::db::crud::get_unsynced_activity_events(&conn, 10)
            .unwrap()
            .is_empty(),
        "activity_events must be marked synced"
    );

    let _ = server_handle.join();
    taurine_core::cloud::clear_tokens().unwrap();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_activity_events_sync_and_mark_synced() {
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
    ensure_schema(&conn).unwrap();

    // Insert unsynced activity event with speech content
    taurine_core::db::crud::insert_activity_event(
        &conn,
        &taurine_core::db::crud::NewActivityEvent {
            id: Some("evt-speech-sync-1".to_string()),
            trigger_id: None,
            timestamp: Some(1728123456000),
            date: Some("2026-10-05".to_string()),
            category: taurine_core::db::crud::ActivityCategory::Voice,
            app_key: Some("slack.exe".to_string()),
            word_count: 4,
            time_saved_ms: 1200,
            content: Some("testing synced speech content".to_string()),
        },
    )
    .unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let start = std::time::Instant::now();
        let mut handled = 0;
        while handled < 7 && start.elapsed() < std::time::Duration::from_secs(3) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut buf = [0u8; 4096];
                    let n = match stream.read(&mut buf) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /rest/v1/") {
                        let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/activity_events") {
                        assert!(req.contains("resolution=merge-duplicates"));
                        assert!(req.contains("evt-speech-sync-1"));
                        assert!(
                            req.contains(r#""content":"testing synced speech content""#),
                            "payload must contain speech content: {}",
                            req
                        );
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    }
                    handled += 1;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
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
            pulled_snippets: 0,
            pushed_snippets: 0,
            pulled_settings: 0,
            pushed_settings: 0,
            pulled_events: 0,
            pushed_stats: 1,
        }
    );

    // Verify activity event is marked synced
    assert!(
        taurine_core::db::crud::get_unsynced_activity_events(&conn, 10)
            .unwrap()
            .is_empty(),
        "activity_events must be marked synced"
    );

    let _ = server_handle.join();
    taurine_core::cloud::clear_tokens().unwrap();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_bidirectional_sync_restores_all_entities_on_fresh_database() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::hook::tests::mock_keystore::use_mock_keystore();

    let tokens = taurine_core::cloud::AuthTokens {
        access_token: "mock-token-fresh-restore".to_string(),
        refresh_token: "mock-refresh".to_string(),
        user_id: "user-restore-1".to_string(),
        expires_at: Some(2000000000),
    };
    taurine_core::cloud::store_tokens(&tokens).unwrap();

    let conn = Connection::open_in_memory().unwrap();
    ensure_schema(&conn).unwrap();
    taurine_core::db::init::seed::ensure_defaults(&conn).unwrap();

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let start = std::time::Instant::now();
        let mut handled = 0;
        // Cycle 1: 6 GETs (restore from cloud)
        // Cycle 2: 6 GETs + 6 POSTs (push local mutations to cloud)
        while handled < 18 && start.elapsed() < std::time::Duration::from_secs(6) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut buf = [0u8; 8192];
                    let n = match stream.read(&mut buf) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /rest/v1/snippets") {
                        let remote_snippet = CloudSnippetPayload {
                            id: "cloud-snippet-1".to_string(),
                            workspace_id: "default".to_string(),
                            name: "Cloud Trigger 1".to_string(),
                            output: "Hello from cloud".to_string(),
                            description: Some("Cloud snippet description".to_string()),
                            action_type: "text".to_string(),
                            target_os: Some("all".to_string()),
                            only_apps: None,
                            except_apps: None,
                            tags: Some("[\"cloud\"]".to_string()),
                            is_enabled: true,
                            auto_case: false,
                            version: 1,
                            is_deleted: false,
                            updated_at: 1730000000,
                            created_at: 1730000000,
                            aliases: vec![TriggerAliasRow {
                                id: "alias-cloud-1".to_string(),
                                trigger_id: "cloud-snippet-1".to_string(),
                                invocation: "hfc".to_string(),
                                invocation_type: InvocationType::Word,
                                require_confirmation: false,
                                strict_threshold: None,
                                created_at: 1730000000,
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
                        assert!(req.contains("local-snippet-2"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("GET /rest/v1/user_settings") {
                        let remote_setting = taurine_core::db::crud::CloudSettingPayload {
                            key: "wpm".to_string(),
                            value: serde_json::json!(150),
                            version: 2,
                            updated_at: Some("2026-10-28T05:00:00Z".to_string()),
                        };
                        let body = serde_json::to_string(&vec![remote_setting]).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/user_settings") {
                        assert!(req.contains("audio_volume"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("GET /rest/v1/activity_events") {
                        let remote_event = CloudActivityEventPayload {
                            device_id: "device-remote-1".to_string(),
                            id: "evt-cloud-1".to_string(),
                            trigger_id: Some("cloud-snippet-1".to_string()),
                            timestamp: 1730000100000,
                            date: "2026-10-28".to_string(),
                            category: "text".to_string(),
                            app_key: "code.exe".to_string(),
                            word_count: 5,
                            time_saved_ms: 1000,
                            content: Some("Hello from cloud".to_string()),
                            version: 1,
                        };
                        let body = serde_json::to_string(&vec![remote_event]).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/activity_events") {
                        assert!(req.contains("evt-local-2"));
                        assert!(req.contains("\"device_id\":"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("GET /rest/v1/trigger_daily_stats") {
                        let remote_trigger_stat = CloudTriggerDailyStatPayload {
                            device_id: "device-remote-1".to_string(),
                            trigger_id: "cloud-snippet-1".to_string(),
                            date: "2026-10-28".to_string(),
                            app_key: "code.exe".to_string(),
                            executions: 3,
                            words_spoken: 0,
                            words_expanded: 15,
                            time_saved_ms: 3000,
                            version: 1,
                        };
                        let body = serde_json::to_string(&vec![remote_trigger_stat]).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/trigger_daily_stats") {
                        assert!(req.contains("local-snippet-2"));
                        assert!(req.contains("\"device_id\":"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("GET /rest/v1/user_stats") {
                        let remote_user_stat = CloudUserStatPayload {
                            device_id: "device-remote-1".to_string(),
                            date: "2026-10-28".to_string(),
                            executions: 12,
                            ai_executions: 0,
                            voice_executions: 0,
                            words_spoken: 0,
                            words_expanded: 60,
                            time_saved_ms: 12000,
                            version: 1,
                        };
                        let body = serde_json::to_string(&vec![remote_user_stat]).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/user_stats") {
                        assert!(req.contains("2026-10-28"));
                        assert!(req.contains("\"device_id\":"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("GET /rest/v1/user_app_stats") {
                        let remote_app_stat = CloudUserAppStatPayload {
                            device_id: "device-remote-1".to_string(),
                            app_key: "code.exe".to_string(),
                            date: "2026-10-28".to_string(),
                            executions: 8,
                            words_spoken: 0,
                            words_expanded: 40,
                            time_saved_ms: 8000,
                            version: 1,
                        };
                        let body = serde_json::to_string(&vec![remote_app_stat]).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream.write_all(resp.as_bytes()).unwrap();
                    } else if req.starts_with("POST /rest/v1/user_app_stats") {
                        assert!(req.contains("code.exe"));
                        assert!(req.contains("\"device_id\":"));
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        stream.write_all(resp.as_bytes()).unwrap();
                    }
                    handled += 1;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(_) => break,
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
            pushed_snippets: 0,
            pulled_settings: 1,
            pushed_settings: 0,
            pulled_events: 1,
            pushed_stats: 0,
        }
    );

    // Verify pulled snippet was restored in SQLite
    let snippet_trigger = taurine_core::db::crud::triggers::get_trigger(&conn, "cloud-snippet-1")
        .unwrap()
        .expect("snippet must exist locally");
    assert_eq!(snippet_trigger.output, "Hello from cloud");
    assert!(snippet_trigger.is_synced);

    let aliases = taurine_core::db::crud::triggers::list_aliases(&conn, "cloud-snippet-1").unwrap();
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0].invocation, "hfc");

    // Verify seeded factory default setting (wpm) was updated to cloud value
    let wpm_setting = taurine_core::db::crud::get_setting(&conn, "wpm")
        .unwrap()
        .expect("wpm setting must exist");
    assert_eq!(wpm_setting.value, "150");
    assert_eq!(wpm_setting.version, 2);
    assert!(wpm_setting.is_synced);

    // Verify activity event was restored
    let events = taurine_core::db::crud::get_activity_events(&conn, 10, 0).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, "evt-cloud-1");
    assert_eq!(events[0].content.as_deref(), Some("Hello from cloud"));
    assert!(events[0].is_synced);

    // Verify user stats restored
    let user_stat = taurine_core::db::crud::get_stat(&conn, "2026-10-28")
        .unwrap()
        .expect("user stat must exist");
    assert_eq!(user_stat.executions, 12);
    assert_eq!(user_stat.words_expanded, 60);
    let stat_is_synced: bool = conn
        .query_row(
            "SELECT is_synced FROM stats WHERE date = '2026-10-28'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(stat_is_synced);

    // Verify trigger daily stats restored
    let trig_stats =
        taurine_core::db::crud::get_trigger_daily_stats_by_trigger(&conn, "cloud-snippet-1", 10)
            .unwrap();
    assert_eq!(trig_stats.len(), 1);
    assert_eq!(trig_stats[0].executions, 3);
    assert_eq!(trig_stats[0].app_key, "code.exe");
    assert!(trig_stats[0].is_synced);

    // Ensure no unsynced rows are queued for re-upload
    assert!(
        taurine_core::db::crud::triggers::get_unsynced_triggers(&conn)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_settings(&conn)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_activity_events(&conn, 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_stats(&conn, 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_trigger_daily_stats(&conn, 10)
            .unwrap()
            .is_empty()
    );

    // =========================================================================
    // CYCLE 2: Local mutations on the device -> verify push uploads to cloud
    // =========================================================================

    // 1. Create a new trigger locally (marked is_synced = 0)
    conn.execute(
        "INSERT INTO triggers (
            id, description, output, action_type, target_os, only_apps, except_apps,
            tags, is_enabled, auto_case, version, is_deleted, is_synced, created_at, updated_at
        ) VALUES ('local-snippet-2', 'Local snippet 2', 'Local output 2', 'text', 'all', NULL, NULL,
                  '[]', 1, 0, 1, 0, 0, 1000, 1000)",
        [],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO trigger_aliases (
            id, trigger_id, invocation, invocation_type, require_confirmation, strict_threshold, created_at
        ) VALUES ('alias-loc2', 'local-snippet-2', 'loc2', 'word', 0, NULL, 1000)",
        [],
    )
    .unwrap();

    // 2. Update a syncable setting locally (marked is_synced = 0)
    taurine_core::db::crud::upsert_setting(&conn, "audio_volume", "85").unwrap();

    // 3. Generate a local activity event (marked is_synced = 0)
    taurine_core::db::crud::insert_activity_event(
        &conn,
        &taurine_core::db::crud::NewActivityEvent {
            id: Some("evt-local-2".to_string()),
            trigger_id: Some("local-snippet-2".to_string()),
            timestamp: Some(1730000500000),
            date: Some("2026-10-28".to_string()),
            category: taurine_core::db::crud::ActivityCategory::Text,
            app_key: Some("code.exe".to_string()),
            word_count: 8,
            time_saved_ms: 2000,
            content: Some("Local output 2".to_string()),
        },
    )
    .unwrap();

    // 4. Record local trigger daily stat (marked is_synced = 0)
    taurine_core::db::crud::stats::upsert_trigger_daily_stat_with_conn(
        &conn,
        "local-snippet-2",
        "2026-10-28",
        "code.exe",
        2,
        0,
        16,
        4000,
    )
    .unwrap();

    // 5. Increment local global stat (marked is_synced = 0)
    taurine_core::db::crud::increment_stat(
        &conn,
        "2026-10-28",
        &taurine_core::db::crud::StatDeltas {
            executions: 2,
            words_expanded: 16,
            time_saved_ms: 4000,
            ..Default::default()
        },
    )
    .unwrap();

    // 6. Record local app stat (marked is_synced = 0)
    taurine_core::db::crud::upsert_app_stat_with_conn(
        &conn,
        "code.exe",
        "2026-10-28",
        2,
        0,
        16,
        4000,
    )
    .unwrap();

    // Verify unsynced state before cycle 2
    assert_eq!(
        taurine_core::db::crud::triggers::get_unsynced_triggers(&conn)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        taurine_core::db::crud::get_unsynced_settings(&conn)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        taurine_core::db::crud::get_unsynced_activity_events(&conn, 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        taurine_core::db::crud::get_unsynced_stats(&conn, 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        taurine_core::db::crud::get_unsynced_trigger_daily_stats(&conn, 10)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        taurine_core::db::crud::get_unsynced_app_stats(&conn, 10)
            .unwrap()
            .len(),
        1
    );

    // Run cycle 2
    let status2 = SyncWorker::run_sync_cycle(&conn, Some(&client)).await;
    assert_eq!(
        status2,
        SyncStatus::Success {
            pulled_snippets: 0,
            pushed_snippets: 1,
            pulled_settings: 0,
            pushed_settings: 1,
            pulled_events: 0,
            pushed_stats: 4,
        }
    );

    // Verify all local mutations are now marked is_synced = 1 in SQLite
    assert!(
        taurine_core::db::crud::triggers::get_unsynced_triggers(&conn)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_settings(&conn)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_activity_events(&conn, 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_stats(&conn, 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_trigger_daily_stats(&conn, 10)
            .unwrap()
            .is_empty()
    );
    assert!(
        taurine_core::db::crud::get_unsynced_app_stats(&conn, 10)
            .unwrap()
            .is_empty()
    );

    let _ = server_handle.join();
    taurine_core::cloud::clear_tokens().unwrap();
}

#[tokio::test]
#[allow(clippy::await_holding_lock)]
async fn test_event_driven_sync_worker_wakes_on_notification() {
    let _lock = crate::hook::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    crate::hook::tests::mock_keystore::use_mock_keystore();

    let tokens = taurine_core::cloud::AuthTokens {
        access_token: "mock-token-xyz".to_string(),
        refresh_token: "mock-refresh".to_string(),
        user_id: "user-event-1".to_string(),
        expires_at: Some(2000000000),
    };
    taurine_core::cloud::store_tokens(&tokens).unwrap();

    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("event_sync.db");
    {
        let init_conn = Connection::open(&db_path).unwrap();
        ensure_schema(&init_conn).unwrap();
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();

    let cycle_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cycle_count_server = cycle_count.clone();

    let server_handle = std::thread::spawn(move || {
        use std::io::{Read, Write};
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(10) {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let _ = stream.set_nonblocking(false);
                    let mut buf = [0u8; 8192];
                    let n = match stream.read(&mut buf) {
                        Ok(n) => n,
                        Err(_) => continue,
                    };
                    let req = String::from_utf8_lossy(&buf[..n]);

                    if req.starts_with("GET /rest/v1/snippets") {
                        let cur_cycle =
                            cycle_count_server.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let snippets = if cur_cycle == 0 {
                            vec![]
                        } else {
                            vec![CloudSnippetPayload {
                                id: "event-snippet-1".to_string(),
                                workspace_id: "default".to_string(),
                                name: "Event Trigger".to_string(),
                                output: "Event-driven output".to_string(),
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
                                updated_at: 1730000000,
                                created_at: 1730000000,
                                aliases: vec![],
                            }]
                        };
                        let body = serde_json::to_string(&snippets).unwrap();
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes());
                    } else if req.starts_with("GET /rest/v1/") {
                        let resp = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]";
                        let _ = stream.write_all(resp.as_bytes());
                    } else if req.starts_with("POST /rest/v1/") {
                        let resp = "HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(resp.as_bytes());
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    });

    let prev_url = std::env::var("TAURINE_SUPABASE_URL").ok();
    // SAFETY: Serialized via TEST_LOCK; setting mock server URL for test.
    unsafe {
        std::env::set_var("TAURINE_SUPABASE_URL", format!("http://127.0.0.1:{port}"));
    }

    struct EnvGuard {
        prev: Option<String>,
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: Serialized via TEST_LOCK; restoring TAURINE_SUPABASE_URL.
            unsafe {
                match &self.prev {
                    Some(v) => std::env::set_var("TAURINE_SUPABASE_URL", v),
                    None => std::env::remove_var("TAURINE_SUPABASE_URL"),
                }
            }
        }
    }
    let _env_guard = EnvGuard { prev: prev_url };

    let notify = std::sync::Arc::new(tokio::sync::Notify::new());
    let db_path_clone = db_path.clone();
    let conn_provider = move || rusqlite::Connection::open(&db_path_clone);

    // Spawn event-driven worker with a 600s fallback interval (10 minutes)
    let _worker_handle = SyncWorker::spawn_event_driven(
        conn_provider,
        std::time::Duration::from_secs(600),
        notify.clone(),
    );

    // Wait until boot cycle completes (cycle 0)
    let boot_wait_start = std::time::Instant::now();
    while cycle_count.load(std::sync::atomic::Ordering::SeqCst) == 0
        && boot_wait_start.elapsed() < std::time::Duration::from_secs(3)
    {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(
        cycle_count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "Boot cycle must complete"
    );

    // Wait for the 3s cooldown window to elapse so that notify trigger executes promptly
    tokio::time::sleep(std::time::Duration::from_millis(3100)).await;

    // Fire notification to wake the worker
    let notify_start = std::time::Instant::now();
    notify.notify_one();

    // Assert that the sync cycle executes within 1-2 seconds (debounced ~300ms, not waiting 600s)
    let mut found = false;
    while notify_start.elapsed() < std::time::Duration::from_secs(3) {
        let check_conn = rusqlite::Connection::open(&db_path).unwrap();
        if let Ok(Some(trig)) =
            taurine_core::db::crud::triggers::get_trigger(&check_conn, "event-snippet-1")
            && trig.output == "Event-driven output"
        {
            found = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert!(
        found,
        "Snippet must be synchronized following event notification"
    );
    let elapsed = notify_start.elapsed();
    assert!(
        elapsed >= std::time::Duration::from_millis(300),
        "Must debounce for at least 300ms; elapsed: {:?}",
        elapsed
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "Event-driven sync cycle must execute within 1-2 seconds instead of waiting 600s; elapsed: {:?}",
        elapsed
    );

    let _ = server_handle.join();
    taurine_core::cloud::clear_tokens().unwrap();
}
