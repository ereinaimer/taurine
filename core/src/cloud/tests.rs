use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use crate::cloud::client::CloudClient;
use crate::cloud::keyring::{clear_tokens, get_tokens, store_tokens};
use crate::cloud::types::{AuthTokens, CloudConfig, CloudProfile};
use crate::testing::TEST_LOCK;

fn lock_test() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn test_keyring_token_storage_retrieval_and_clearing() {
    let _guard = lock_test();
    crate::testing::use_shared_test_keyring();

    // Ensure clean initial state
    clear_tokens().expect("clear_tokens on initial state must succeed");
    let initial = get_tokens().expect("get_tokens on empty keystore must succeed");
    assert_eq!(initial, None);

    let tokens = AuthTokens {
        access_token: "jwt_token_abc_123".to_string(),
        refresh_token: "refresh_token_xyz_456".to_string(),
        user_id: "user_uuid_789".to_string(),
        expires_at: Some(1730000000),
    };

    // Store tokens
    store_tokens(&tokens).expect("store_tokens must succeed");

    // Retrieve tokens
    let retrieved = get_tokens()
        .expect("get_tokens must succeed")
        .expect("tokens must be present");
    assert_eq!(retrieved, tokens);

    // Update tokens with new values
    let updated_tokens = AuthTokens {
        access_token: "new_jwt_token_def".to_string(),
        refresh_token: "new_refresh_token_uvw".to_string(),
        user_id: "user_uuid_789".to_string(),
        expires_at: Some(1730003600),
    };
    store_tokens(&updated_tokens).expect("updating tokens must succeed");

    let retrieved_updated = get_tokens()
        .expect("get_tokens must succeed")
        .expect("updated tokens must be present");
    assert_eq!(retrieved_updated, updated_tokens);

    // Clear tokens
    clear_tokens().expect("clear_tokens must succeed");
    let after_clear = get_tokens().expect("get_tokens after clear must succeed");
    assert_eq!(after_clear, None);

    // Calling clear_tokens again when empty is idempotent
    clear_tokens().expect("idempotent clear_tokens must succeed");
}

#[test]
fn test_cloud_config_from_env() {
    let _guard = lock_test();

    // Save previous env vars
    let orig_t_url = std::env::var("TAURINE_SUPABASE_URL").ok();
    let orig_t_key = std::env::var("TAURINE_SUPABASE_ANON_KEY").ok();
    let orig_s_url = std::env::var("SUPABASE_URL").ok();
    let orig_s_key = std::env::var("SUPABASE_ANON_KEY").ok();

    // Clear all
    // SAFETY: Mutating process environment variables is serialized via TEST_LOCK; isolated to this test.
    unsafe {
        std::env::remove_var("TAURINE_SUPABASE_URL");
        std::env::remove_var("TAURINE_SUPABASE_ANON_KEY");
        std::env::remove_var("SUPABASE_URL");
        std::env::remove_var("SUPABASE_ANON_KEY");
    }

    // Missing env vars should fail
    let err = CloudConfig::from_env().expect_err("from_env without vars must fail");
    assert!(
        matches!(err, crate::Error::Config(_)),
        "expected Error::Config, got: {err:?}"
    );

    // Test with TAURINE_SUPABASE_* (with trailing slash on URL to verify trimming)
    // SAFETY: Mutating process environment variables is serialized via TEST_LOCK; isolated to this test.
    unsafe {
        std::env::set_var("TAURINE_SUPABASE_URL", "https://xyzcompany.supabase.co/");
        std::env::set_var("TAURINE_SUPABASE_ANON_KEY", "taurine-anon-key-123");
    }

    let cfg = CloudConfig::from_env().expect("from_env with TAURINE_* vars must succeed");
    assert_eq!(cfg.supabase_url, "https://xyzcompany.supabase.co");
    assert_eq!(cfg.anon_key, "taurine-anon-key-123");

    // Clear TAURINE_* and test fallback to SUPABASE_*
    // SAFETY: Mutating process environment variables is serialized via TEST_LOCK; isolated to this test.
    unsafe {
        std::env::remove_var("TAURINE_SUPABASE_URL");
        std::env::remove_var("TAURINE_SUPABASE_ANON_KEY");
        std::env::set_var("SUPABASE_URL", "https://fallback.supabase.co");
        std::env::set_var("SUPABASE_ANON_KEY", "fallback-anon-key-456");
    }

    let cfg_fallback = CloudConfig::from_env().expect("from_env with fallback vars must succeed");
    assert_eq!(cfg_fallback.supabase_url, "https://fallback.supabase.co");
    assert_eq!(cfg_fallback.anon_key, "fallback-anon-key-456");

    // Also test CloudClient::from_env
    let client = CloudClient::from_env().expect("CloudClient::from_env must succeed");
    assert_eq!(client.config().supabase_url, "https://fallback.supabase.co");

    // Restore original env vars
    let restore = |key: &str, val: Option<String>| {
        // SAFETY: Mutating process environment variables is serialized via TEST_LOCK; restoring original state.
        unsafe {
            match val {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    };
    restore("TAURINE_SUPABASE_URL", orig_t_url);
    restore("TAURINE_SUPABASE_ANON_KEY", orig_t_key);
    restore("SUPABASE_URL", orig_s_url);
    restore("SUPABASE_ANON_KEY", orig_s_key);
}

#[test]
fn test_cloud_config_default_or_from_env() {
    let _guard = lock_test();

    let orig_t_url = std::env::var("TAURINE_SUPABASE_URL").ok();
    let orig_t_key = std::env::var("TAURINE_SUPABASE_ANON_KEY").ok();
    let orig_s_url = std::env::var("SUPABASE_URL").ok();
    let orig_s_key = std::env::var("SUPABASE_ANON_KEY").ok();

    // SAFETY: Mutating process environment variables is serialized via TEST_LOCK; isolated to this test.
    unsafe {
        std::env::remove_var("TAURINE_SUPABASE_URL");
        std::env::remove_var("TAURINE_SUPABASE_ANON_KEY");
        std::env::remove_var("SUPABASE_URL");
        std::env::remove_var("SUPABASE_ANON_KEY");
    }

    let default_cfg = CloudConfig::default_or_from_env();
    assert_eq!(
        default_cfg.supabase_url,
        crate::cloud::types::DEFAULT_SUPABASE_URL
    );
    assert_eq!(
        default_cfg.anon_key,
        crate::cloud::types::DEFAULT_SUPABASE_ANON_KEY
    );

    let default_client = CloudClient::default_or_from_env();
    assert_eq!(
        default_client.config().supabase_url,
        crate::cloud::types::DEFAULT_SUPABASE_URL
    );

    let restore = |key: &str, val: Option<String>| {
        // SAFETY: Mutating process environment variables is serialized via TEST_LOCK; restoring original state.
        unsafe {
            match val {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
        }
    };
    restore("TAURINE_SUPABASE_URL", orig_t_url);
    restore("TAURINE_SUPABASE_ANON_KEY", orig_t_key);
    restore("SUPABASE_URL", orig_s_url);
    restore("SUPABASE_ANON_KEY", orig_s_key);
}

#[test]
fn test_pkce_auth_url_generation() {
    let config = CloudConfig {
        supabase_url: "https://taurine.supabase.co".to_string(),
        anon_key: "anon-key-abc".to_string(),
    };
    let client = CloudClient::new(config);

    let redirect_uri = "http://127.0.0.1:54321/callback";
    let code_challenge = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";

    let url = client.auth_url(redirect_uri, code_challenge);

    assert!(url.starts_with("https://taurine.supabase.co/auth/v1/authorize"));
    assert!(url.contains("code_challenge=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
    assert!(url.contains("code_challenge_method=s256"));
    assert!(url.contains("response_type=code"));
    // The redirect uri should be in the URL (percent-encoded or raw)
    assert!(
        url.contains("http%3A%2F%2F127.0.0.1%3A54321%2Fcallback")
            || url.contains("http://127.0.0.1:54321/callback")
    );
}

#[tokio::test]
async fn test_exchange_code_for_session_mock_http() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]);

        assert!(req.starts_with("POST /auth/v1/token?grant_type=pkce HTTP/1.1"));
        assert!(req.to_lowercase().contains("apikey: test-anon-key"));
        assert!(req.contains("\"code_verifier\":\"verifier-12345\""));
        assert!(req.contains("code-xyz-987"));

        let body = r#"{
            "access_token": "mock_access_token_jwt",
            "refresh_token": "mock_refresh_token_777",
            "expires_at": 1730001234,
            "user": {
                "id": "mock_user_uuid_111"
            }
        }"#;

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let config = CloudConfig {
        supabase_url: format!("http://127.0.0.1:{port}"),
        anon_key: "test-anon-key".to_string(),
    };
    let client = CloudClient::new(config);

    let tokens = client
        .exchange_code_for_session(
            "code-xyz-987",
            "verifier-12345",
            "http://127.0.0.1:54321/callback",
        )
        .await
        .expect("exchange_code_for_session must succeed");

    assert_eq!(tokens.access_token, "mock_access_token_jwt");
    assert_eq!(tokens.refresh_token, "mock_refresh_token_777");
    assert_eq!(tokens.user_id, "mock_user_uuid_111");
    assert_eq!(tokens.expires_at, Some(1730001234));

    server_handle.join().unwrap();
}

#[tokio::test]
async fn test_refresh_session_mock_http() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]);

        assert!(req.starts_with("POST /auth/v1/token?grant_type=refresh_token HTTP/1.1"));
        assert!(req.to_lowercase().contains("apikey: test-anon-key"));
        assert!(req.contains("\"refresh_token\":\"old_refresh_token\""));

        let body = r#"{
            "access_token": "new_access_token_jwt",
            "refresh_token": "new_refresh_token_999",
            "expires_in": 3600,
            "user": {
                "id": "mock_user_uuid_111"
            }
        }"#;

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let config = CloudConfig {
        supabase_url: format!("http://127.0.0.1:{port}"),
        anon_key: "test-anon-key".to_string(),
    };
    let client = CloudClient::new(config);

    let tokens = client
        .refresh_session("old_refresh_token")
        .await
        .expect("refresh_session must succeed");

    assert_eq!(tokens.access_token, "new_access_token_jwt");
    assert_eq!(tokens.refresh_token, "new_refresh_token_999");
    assert_eq!(tokens.user_id, "mock_user_uuid_111");
    assert!(tokens.expires_at.is_some());

    server_handle.join().unwrap();
}

#[tokio::test]
async fn test_get_profile_mock_http() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]);

        assert!(req.starts_with("GET /rest/v1/profiles?id=eq.mock_user_uuid_111 HTTP/1.1"));
        assert!(req.to_lowercase().contains("apikey: test-anon-key"));
        assert!(req.contains("authorization: Bearer test_bearer_token"));

        let body = r#"[
            {
                "id": "mock_user_uuid_111",
                "tier": "pro",
                "stripe_customer_id": "cus_123456789"
            }
        ]"#;

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let config = CloudConfig {
        supabase_url: format!("http://127.0.0.1:{port}"),
        anon_key: "test-anon-key".to_string(),
    };
    let client = CloudClient::new(config);

    let profile = client
        .get_profile("test_bearer_token", "mock_user_uuid_111")
        .await
        .expect("get_profile must succeed");

    assert_eq!(
        profile,
        CloudProfile {
            id: "mock_user_uuid_111".to_string(),
            tier: "pro".to_string(),
            stripe_customer_id: Some("cus_123456789".to_string()),
        }
    );

    server_handle.join().unwrap();
}

#[tokio::test]
async fn test_get_profile_not_found_mock_http() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let _ = stream.read(&mut buf).unwrap();

        // Return empty array for not found
        let body = "[]";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let config = CloudConfig {
        supabase_url: format!("http://127.0.0.1:{port}"),
        anon_key: "test-anon-key".to_string(),
    };
    let client = CloudClient::new(config);

    let err = client
        .get_profile("test_bearer_token", "missing_user")
        .await
        .expect_err("get_profile for non-existent user must fail");

    assert!(
        matches!(err, crate::Error::NotFound(_)),
        "expected Error::NotFound, got: {err:?}"
    );

    server_handle.join().unwrap();
}

#[tokio::test]
async fn test_exchange_code_error_handling_mock_http() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let server_handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let _ = stream.read(&mut buf).unwrap();

        let body = r#"{"error":"invalid_grant","error_description":"Invalid PKCE code"}"#;
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let config = CloudConfig {
        supabase_url: format!("http://127.0.0.1:{port}"),
        anon_key: "test-anon-key".to_string(),
    };
    let client = CloudClient::new(config);

    let err = client
        .exchange_code_for_session("bad_code", "bad_verifier", "http://callback")
        .await
        .expect_err("exchange_code with invalid grant must fail");

    assert!(
        matches!(err, crate::Error::Service(_)),
        "expected Error::Service, got: {err:?}"
    );

    server_handle.join().unwrap();
}

#[test]
fn test_generate_pkce_challenge() {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use sha2::{Digest, Sha256};

    let (verifier, challenge) = super::generate_pkce_challenge();

    // Verifier should decode to 32 bytes from URL-safe unpadded base64
    let verifier_bytes = URL_SAFE_NO_PAD
        .decode(&verifier)
        .expect("verifier must be valid URL-safe unpadded base64");
    assert_eq!(verifier_bytes.len(), 32);

    // Challenge should be SHA-256 of verifier ASCII bytes, URL-safe unpadded base64
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let expected_challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());
    assert_eq!(challenge, expected_challenge);

    // Consecutive calls should generate distinct random verifiers
    let (v2, c2) = super::generate_pkce_challenge();
    assert_ne!(verifier, v2);
    assert_ne!(challenge, c2);
}
