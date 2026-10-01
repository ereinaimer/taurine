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
            "Authentication required. Please run 'taurine login' to continue.".into(),
        )),
    }
}

/// Executes the cloud login sequence via GoTrue OAuth PKCE or Email/Password.
pub fn execute_login(
    no_browser: bool,
    provider: Option<&str>,
    email: Option<&str>,
    json: bool,
) -> Result<()> {
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
        let provider_name = provider.unwrap_or("github");
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

    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "logged_in",
                "user_id": tokens.user_id,
                "tier": format!("{:?}", tier).to_lowercase(),
            })
        );
    } else {
        println!(
            "Successfully logged in as {} (Tier: {:?})",
            tokens.user_id, tier
        );
    }

    Ok(())
}

/// Logs out the user by clearing credentials from the keystore and resetting local tier cache.
pub fn execute_logout(json: bool) -> Result<()> {
    taurine_core::cloud::clear_tokens()?;
    let conn = taurine_core::db::init::setup()?;
    taurine_core::db::crud::set_user_tier(&conn, UserTier::Free)?;

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

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
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
