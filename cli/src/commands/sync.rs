// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

use taurine_core::error::Result;

pub fn execute_sync(json: bool) -> Result<()> {
    // Check authentication
    let tokens_opt = taurine_core::cloud::get_tokens().ok().flatten();
    if tokens_opt.is_none() {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "success": false,
                    "error": "Please log in to use Taurine. Run 'taurine login' to continue."
                })
            );
        } else {
            eprintln!("Please log in to use Taurine. Run 'taurine login' to continue.");
        }
        return Ok(());
    }

    // If daemon is running, call RPC for centralized coordinator
    if taurine_core::service::is_service_running() {
        match taurine_core::rpc::request_daemon_sync() {
            Ok(resp) => {
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "success": resp.success,
                            "pulled_snippets": resp.pulled_snippets,
                            "pushed_snippets": resp.pushed_snippets,
                            "pulled_settings": resp.pulled_settings,
                            "pushed_settings": resp.pushed_settings,
                            "pulled_events": resp.pulled_events,
                            "pushed_stats": resp.pushed_stats,
                            "error": if resp.error_message.is_empty() { None } else { Some(resp.error_message) }
                        })
                    );
                } else if resp.success {
                    println!("Cloud synchronization complete:");
                    println!(
                        "  • Snippets:  {} pulled, {} pushed",
                        resp.pulled_snippets, resp.pushed_snippets
                    );
                    println!(
                        "  • Settings:  {} pulled, {} pushed",
                        resp.pulled_settings, resp.pushed_settings
                    );
                    println!(
                        "  • Telemetry: {} events pulled, {} stats pushed",
                        resp.pulled_events, resp.pushed_stats
                    );
                } else {
                    eprintln!("Synchronization failed: {}", resp.error_message);
                }
                return Ok(());
            }
            Err(e) => {
                tracing::debug!("RPC sync request failed ({e}); falling back to direct db sync");
            }
        }
    }

    // Direct synchronization fallback when daemon is offline
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let conn = taurine_core::db::init::setup()?;
    let client = taurine_core::cloud::CloudClient::default_or_from_env();

    let outcome = rt.block_on(taurine_daemon::SyncWorker::run_sync_cycle(
        &conn,
        Some(&client),
    ));

    match outcome {
        taurine_daemon::SyncStatus::Success {
            pulled_snippets,
            pushed_snippets,
            pulled_settings,
            pushed_settings,
            pulled_events,
            pushed_stats,
        } => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "success": true,
                        "pulled_snippets": pulled_snippets,
                        "pushed_snippets": pushed_snippets,
                        "pulled_settings": pulled_settings,
                        "pushed_settings": pushed_settings,
                        "pulled_events": pulled_events,
                        "pushed_stats": pushed_stats,
                    })
                );
            } else {
                println!("Cloud synchronization complete (standalone):");
                println!("  • Snippets:  {pulled_snippets} pulled, {pushed_snippets} pushed");
                println!("  • Settings:  {pulled_settings} pulled, {pushed_settings} pushed");
                println!(
                    "  • Telemetry: {pulled_events} events pulled, {pushed_stats} stats pushed"
                );
            }
        }
        taurine_daemon::SyncStatus::NotAuthenticated => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "success": false, "error": "Not authenticated" })
                );
            } else {
                eprintln!(
                    "You are not logged in. Run `taurine login` to connect to Taurine Cloud."
                );
            }
        }
        taurine_daemon::SyncStatus::Error(err) => {
            if json {
                println!("{}", serde_json::json!({ "success": false, "error": err }));
            } else {
                eprintln!("Synchronization failed: {err}");
            }
        }
    }

    Ok(())
}
