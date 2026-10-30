use std::io::Write;
use taurine_core::db::crud::UserTier;
use taurine_core::error::Result;

/// Ensures that the user is currently authenticated with Taurine Cloud.
///
/// Returns the stored AuthTokens if present, or an Error::Config prompting
/// the user to run `taurine login`.
pub fn ensure_authenticated() -> Result<taurine_core::cloud::AuthTokens> {
    match taurine_core::cloud::get_tokens() {
        Ok(Some(tokens)) => Ok(tokens),
        _ => Err(taurine_core::error::Error::Config(
            "Please log in to use Taurine. Run 'taurine login' to continue.".into(),
        )),
    }
}

/// Formats the success message string displayed on successful login.
pub(crate) fn format_login_success_text(email: &str) -> String {
    format!("Successfully logged in as {email}")
}

/// Formats the success JSON payload emitted on successful login.
pub(crate) fn format_login_success_json(email: &str, user_id: &str) -> serde_json::Value {
    serde_json::json!({
        "status": "logged_in",
        "email": email,
        "user_id": user_id,
    })
}

/// Formats the terminal message strings displayed when the user is already authenticated.
pub(crate) fn format_already_logged_in_text(email: &str) -> (String, String) {
    (
        format!("You are already logged into: {email}"),
        "Run 'taurine logout' first if you want to switch accounts.".to_string(),
    )
}

/// Formats the JSON payload emitted when the user is already authenticated.
pub(crate) fn format_already_logged_in_json(email: &str, user_id: &str) -> serde_json::Value {
    serde_json::json!({
        "authenticated": true,
        "email": email,
        "user_id": user_id,
        "message": "Already logged in"
    })
}

/// Executes the cloud login sequence via GoTrue OAuth PKCE or Email/Password.
pub fn execute_login(
    no_browser: bool,
    provider: Option<&str>,
    email: Option<&str>,
    json: bool,
) -> Result<()> {
    if let Ok(Some(tokens)) = taurine_core::cloud::get_tokens() {
        let current_email = taurine_core::cloud::extract_jwt_email(&tokens.access_token)
            .unwrap_or_else(|| tokens.user_id.clone());
        if json {
            println!(
                "{}",
                format_already_logged_in_json(&current_email, &tokens.user_id)
            );
        } else {
            let (line1, line2) = format_already_logged_in_text(&current_email);
            println!("{line1}");
            println!("{line2}");
        }
        return Ok(());
    }

    let (verifier, challenge) = taurine_core::cloud::generate_pkce_challenge();
    let config = taurine_core::cloud::CloudConfig::default_or_from_env();
    let client = taurine_core::cloud::CloudClient::new(config);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let tokens = if let Some(user_email) = email {
        if !json {
            println!("Signing in with email: {user_email}");
            print!("Enter password: ");
            let _ = std::io::stdout().flush();
        }
        let mut pass_input = String::new();
        std::io::stdin()
            .read_line(&mut pass_input)
            .map_err(taurine_core::Error::Io)?;
        let password = pass_input.trim();
        rt.block_on(async { client.sign_in_with_password(user_email, password).await })?
    } else {
        let provider_name = provider.unwrap_or("google");
        if !provider_name.eq_ignore_ascii_case("google") {
            return Err(taurine_core::Error::Config(
                "Unsupported auth provider. Only 'google' and email login are supported.".into(),
            ));
        }
        let (code, redirect_uri) = if no_browser {
            let redirect_uri = "http://127.0.0.1/callback";
            let auth_url = client.auth_url_for_provider(provider_name, redirect_uri, &challenge);
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "auth_url": auth_url,
                        "prompt": "Enter authorization code"
                    })
                );
            } else {
                println!(
                    "Please open the following URL in your browser to log in:\n\n{auth_url}\n"
                );
                print!("Enter authorization code: ");
                let _ = std::io::stdout().flush();
            }

            let mut code_input = String::new();
            std::io::stdin()
                .read_line(&mut code_input)
                .map_err(taurine_core::Error::Io)?;
            let trimmed_code = code_input.trim().to_string();
            if trimmed_code.is_empty() {
                return Err(taurine_core::Error::Config(
                    "No authorization code provided.".into(),
                ));
            }
            (trimmed_code, redirect_uri.to_string())
        } else {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let port = listener.local_addr()?.port();
            let redirect_uri = format!("http://127.0.0.1:{port}/callback");
            let auth_url = client.auth_url_for_provider(provider_name, &redirect_uri, &challenge);

            if !json {
                println!("Opening browser for authentication ({provider_name})...");
                println!("If your browser does not open automatically, visit:\n{auth_url}\n");
            }

            open_browser(&auth_url);

            let (mut stream, _) = listener.accept()?;
            use std::io::{BufRead, BufReader};
            let mut reader = BufReader::new(&stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line)?;

            let code = extract_code_from_request_line(&request_line).ok_or_else(|| {
                taurine_core::Error::Config(
                    "Failed to extract authorization code from OAuth callback.".into(),
                )
            })?;

            let response_body = "<!DOCTYPE html><html><head><title>Taurine Login</title></head><body><h1>Authentication Successful</h1><p>You can close this tab and return to your terminal.</p></body></html>";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();

            (code, redirect_uri)
        };

        rt.block_on(async {
            client
                .exchange_code_for_session(&code, &verifier, &redirect_uri)
                .await
        })?
    };

    taurine_core::cloud::store_tokens(&tokens)?;

    // Fetch profile and update local tier cache
    let profile_res = rt.block_on(async {
        client
            .get_profile(&tokens.access_token, &tokens.user_id)
            .await
    });

    let tier = match profile_res {
        Ok(profile) => match profile.tier.to_lowercase().as_str() {
            "pro" => UserTier::Pro,
            "max" => UserTier::Max,
            "team" => UserTier::Team,
            _ => UserTier::Free,
        },
        Err(err) => {
            tracing::warn!("Failed to fetch profile tier: {err}; defaulting to Free");
            UserTier::Free
        }
    };

    let conn = taurine_core::db::init::setup()?;
    taurine_core::db::crud::set_user_tier(&conn, tier)?;

    // Synchronize local database with cloud state immediately upon login
    let _ = rt.block_on(taurine_daemon::SyncWorker::run_sync_cycle(
        &conn,
        Some(&client),
    ));

    if taurine_core::service::is_service_running() {
        taurine_core::rpc::notify_daemon_reload();
    } else {
        let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
        let _ = taurine_core::service::up(settings.start_on_boot);
    }

    let email = taurine_core::cloud::extract_jwt_email(&tokens.access_token)
        .unwrap_or_else(|| tokens.user_id.clone());

    if json {
        println!("{}", format_login_success_json(&email, &tokens.user_id));
    } else {
        println!("{}", format_login_success_text(&email));
    }

    Ok(())
}

/// Logs out the user by stopping the daemon, clearing credentials from the keystore,
/// and purging the local database.
pub fn execute_logout(json: bool) -> Result<()> {
    // Attempt a final sync cycle before purging local state so nothing is lost
    if let Ok(conn) = taurine_core::db::init::setup() {
        let client = taurine_core::cloud::CloudClient::default_or_from_env();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build();
        if let Ok(rt) = rt {
            let _ = rt.block_on(async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    taurine_daemon::SyncWorker::run_sync_cycle(&conn, Some(&client)),
                )
                .await
            });
        }
    }

    // If the service is running, stop it
    if taurine_core::service::is_service_running() {
        let _ = taurine_core::service::down();
    }

    taurine_core::cloud::clear_tokens()?;
    taurine_core::db::init::purge_local_database()?;

    if json {
        println!("{}", serde_json::json!({ "status": "logged_out" }));
    } else {
        println!("Successfully logged out.");
    }
    Ok(())
}

fn extract_code_from_request_line(line: &str) -> Option<String> {
    let code_idx = line.find("code=")?;
    let after_code = &line[code_idx + 5..];
    let end_idx = after_code
        .find(['&', ' ', '\r', '\n'])
        .unwrap_or(after_code.len());

    let raw_code = &after_code[..end_idx];
    if raw_code.is_empty() {
        None
    } else {
        Some(raw_code.to_string())
    }
}

#[cfg(target_os = "windows")]
fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        use std::ptr;
        let wide_verb = to_wide_null("open");
        let wide_url = to_wide_null(url);
        // SAFETY: ShellExecuteW invokes the default system handler for the URL protocol with valid null-terminated wide strings
        unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                ptr::null_mut(),
                wide_verb.as_ptr(),
                wide_url.as_ptr(),
                ptr::null(),
                ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "windows")]
    fn test_to_wide_null_preserves_query_parameters() {
        let url = "https://example.supabase.co/auth/v1/authorize?provider=google&code_challenge=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk&code_challenge_method=s256&redirect_to=http%3A%2F%2F127.0.0.1%3A54321%2Fcallback";
        let wide = to_wide_null(url);
        assert_eq!(*wide.last().unwrap(), 0);

        let decoded = String::from_utf16(&wide[..wide.len() - 1]).expect("valid utf-16");
        assert_eq!(decoded, url);
        assert_eq!(decoded.matches('&').count(), 3);
        assert!(decoded.contains("provider=google"));
        assert!(decoded.contains("code_challenge="));
        assert!(decoded.contains("code_challenge_method="));
        assert!(decoded.contains("redirect_to="));
    }

    #[test]
    fn test_extract_code_from_request_line() {
        assert_eq!(
            extract_code_from_request_line("GET /callback?code=abc123xyz HTTP/1.1"),
            Some("abc123xyz".to_string())
        );
        assert_eq!(
            extract_code_from_request_line("GET /callback?code=abc123xyz&state=state1 HTTP/1.1"),
            Some("abc123xyz".to_string())
        );
        assert_eq!(
            extract_code_from_request_line("GET /callback HTTP/1.1"),
            None
        );
    }

    #[test]
    #[ignore = "touches host browser (native_shell_open); opt-in with TAURINE_ALLOW_HOST_INPUT=1"]
    fn test_open_browser_live_url_invocation() {
        if !taurine_daemon::platform::host_tests_allowed() {
            return;
        }
        open_browser(
            "https://example.supabase.co/auth/v1/authorize?provider=google&code_challenge=test&redirect_to=http://127.0.0.1:8080",
        );
    }

    #[test]
    fn test_login_success_formatting_excludes_tier_and_uses_email() {
        let email = "engineer@taurine.ai";
        let user_id = "00000000-0000-0000-0000-000000000000";

        let text_output = format_login_success_text(email);
        assert_eq!(text_output, "Successfully logged in as engineer@taurine.ai");
        assert!(!text_output.contains("Tier"));
        assert!(!text_output.contains("tier"));
        assert!(!text_output.contains(user_id));

        let json_val = format_login_success_json(email, user_id);
        assert_eq!(json_val["status"], "logged_in");
        assert_eq!(json_val["email"], email);
        assert_eq!(json_val["user_id"], user_id);
        assert!(json_val.get("tier").is_none());
    }

    #[test]
    fn test_already_logged_in_formatting() {
        let email = "engineer@taurine.ai";
        let user_id = "00000000-0000-0000-0000-000000000000";

        let (line1, line2) = format_already_logged_in_text(email);
        assert_eq!(line1, "You are already logged into: engineer@taurine.ai");
        assert_eq!(
            line2,
            "Run 'taurine logout' first if you want to switch accounts."
        );

        let json_val = format_already_logged_in_json(email, user_id);
        assert_eq!(json_val["authenticated"], true);
        assert_eq!(json_val["email"], email);
        assert_eq!(json_val["user_id"], user_id);
        assert_eq!(json_val["message"], "Already logged in");
    }

    #[test]
    fn test_login_session_guard_when_already_authenticated_with_email() {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        crate::commands::test_keyring::use_shared_test_keyring();
        let _ = taurine_core::cloud::clear_tokens();

        let mock_tokens = taurine_core::cloud::AuthTokens {
            access_token: "e30.eyJzdWIiOiJ1MSIsImVtYWlsIjoidXNlckBleGFtcGxlLmNvbSJ9.sig"
                .to_string(),
            refresh_token: "mock-refresh".to_string(),
            user_id: "user-uuid-12345".to_string(),
            expires_at: Some(9999999999),
        };
        taurine_core::cloud::store_tokens(&mock_tokens).expect("store tokens succeeds");

        // Plain text execution should hit session guard and return Ok(()) without network or browser
        let res_plain = execute_login(false, None, None, false);
        assert!(
            res_plain.is_ok(),
            "execute_login must return Ok when already logged in"
        );

        // JSON execution should also return Ok(())
        let res_json = execute_login(true, None, None, true);
        assert!(
            res_json.is_ok(),
            "execute_login json must return Ok when already logged in"
        );

        let _ = taurine_core::cloud::clear_tokens();
    }

    #[test]
    fn test_login_session_guard_fallback_when_token_has_no_email() {
        let _guard = crate::commands::TEST_LOCK.lock().unwrap();
        crate::commands::test_keyring::use_shared_test_keyring();
        let _ = taurine_core::cloud::clear_tokens();

        let mock_tokens = taurine_core::cloud::AuthTokens {
            access_token: "opaque-or-no-email-token".to_string(),
            refresh_token: "mock-refresh".to_string(),
            user_id: "user-fallback-uuid".to_string(),
            expires_at: Some(9999999999),
        };
        taurine_core::cloud::store_tokens(&mock_tokens).expect("store tokens succeeds");

        let res = execute_login(true, None, None, false);
        assert!(
            res.is_ok(),
            "execute_login fallback to user_id must succeed"
        );

        let _ = taurine_core::cloud::clear_tokens();
    }
}
