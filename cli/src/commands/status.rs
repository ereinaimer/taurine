use taurine_core::db::crud::{
    FREE_TIER_MAX_SNIPPETS, FREE_TIER_MAX_WORKSPACES, get_user_tier, get_workspaces,
};
use taurine_core::db::init;
use taurine_core::error::Result;

/// Displays status of local daemon service combined with Taurine Cloud status.
pub fn execute_status(json: bool) -> Result<()> {
    // 1. Service status via RPC if daemon is online
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    let rpc_status = rt.block_on(async {
        if let Ok(mut client) = taurine_core::rpc::get_client().await {
            let request = tonic::Request::new(taurine_core::rpc::StatusRequest {});
            client
                .get_status(request)
                .await
                .ok()
                .map(|r| r.into_inner())
        } else {
            None
        }
    });

    let (service_status, is_running, is_paused) = match rpc_status {
        Some(ref s) if s.paused => ("Paused", true, true),
        Some(ref s) if s.online => ("Running", true, false),
        _ => ("Stopped", false, false),
    };

    // 2. Cloud auth status
    let tokens_opt = taurine_core::cloud::get_tokens().ok().flatten();
    let is_authenticated = tokens_opt.is_some();
    let user_id = tokens_opt.as_ref().map(|t| t.user_id.clone());

    // 3. Database connection & Tier
    let conn = init::setup()?;
    let tier = get_user_tier(&conn);

    // 4. Weekly Quota
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let db_key = taurine_core::db::key::get_or_create_db_key();

    let quota_row = db_key.ok().and_then(|k| {
        taurine_core::db::crud::quota::get_or_init_quota_ledger(&conn, &k, now_epoch).ok()
    });
    let (quota_remaining, rollover_epoch) = match quota_row {
        Some(q) => {
            let rollover = q.week_start_epoch + 7 * 86400;
            (q.remaining_percentage, rollover)
        }
        None => (
            100.0,
            taurine_core::db::crud::quota::calculate_week_start_epoch(now_epoch) + 7 * 86400,
        ),
    };

    // 5. Active Snippets
    let snippet_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM triggers WHERE is_deleted = 0",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // 6. Active Workspaces
    let workspace_count = get_workspaces(&conn).map(|w| w.len()).unwrap_or(1);

    // 7. Active Devices
    let device_name = taurine_core::cloud::get_device_name();
    let device_hw_id = taurine_core::cloud::get_device_hardware_id();
    let device_platform = taurine_core::cloud::get_device_platform();

    if json {
        let json_obj = serde_json::json!({
            "running": is_running,
            "paused": is_paused,
            "service_status": service_status,
            "authenticated": is_authenticated,
            "user_id": user_id,
            "tier": format!("{:?}", tier),
            "weekly_quota": {
                "remaining_percentage": quota_remaining,
                "rollover_epoch": rollover_epoch,
            },
            "active_snippets": {
                "count": snippet_count,
                "limit": if tier.is_unlimited() { serde_json::Value::Null } else { serde_json::json!(FREE_TIER_MAX_SNIPPETS) },
            },
            "active_workspaces": {
                "count": workspace_count,
                "limit": if tier.is_unlimited() { serde_json::Value::Null } else { serde_json::json!(FREE_TIER_MAX_WORKSPACES) },
            },
            "active_devices": {
                "count": 1,
                "limit": if tier.is_unlimited() { serde_json::Value::Null } else { serde_json::json!(1) },
                "current_device": {
                    "name": device_name,
                    "hardware_id": device_hw_id,
                    "platform": device_platform,
                }
            }
        });
        println!("{}", json_obj);
    } else {
        println!("Taurine Status");
        println!("  Service:          {}", service_status);
        if let Some(ref uid) = user_id {
            println!("  Account ID:       {}", uid);
        } else {
            println!("  Account ID:       Not logged in (run 'taurine login')");
        }
        println!("  Tier:             {:?}", tier);
        if tier.is_unlimited() {
            println!("  Active Devices:   1 active ({}, Unlimited)", device_name);
            println!(
                "  Weekly Quota:     {:.1}% remaining (rollover epoch: {})",
                quota_remaining, rollover_epoch
            );
            println!("  Active Snippets:  {} active (Unlimited)", snippet_count);
            println!("  Workspaces:       {} active (Unlimited)", workspace_count);
        } else {
            println!("  Active Devices:   1 / 1 active ({})", device_name);
            println!(
                "  Weekly Quota:     {:.1}% remaining (rollover epoch: {})",
                quota_remaining, rollover_epoch
            );
            println!(
                "  Active Snippets:  {} / {} active",
                snippet_count, FREE_TIER_MAX_SNIPPETS
            );
            println!(
                "  Workspaces:       {} / {} active",
                workspace_count, FREE_TIER_MAX_WORKSPACES
            );
        }
    }

    Ok(())
}
