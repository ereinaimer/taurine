use nite_core::error::Error;

pub fn execute_up(json: bool) -> Result<(), Error> {
    let settings = {
        let conn = nite_core::db::init::setup()?;
        nite_core::settings::SettingsManager::new(&conn).load_all()
    };

    nite_core::service::up(settings.start_on_boot)?;
    if json {
        println!("{}", serde_json::json!({"status": "started"}));
    }
    Ok(())
}

pub fn execute_down(json: bool) -> Result<(), Error> {
    nite_core::service::down()?;
    if json {
        println!("{}", serde_json::json!({"status": "stopped"}));
    }
    Ok(())
}

pub fn execute_restart(json: bool) -> Result<(), Error> {
    let start_on_boot = start_on_boot_preference()?;
    nite_core::service::restart(start_on_boot)?;
    if json {
        println!("{}", serde_json::json!({"status": "restarted"}));
    }
    Ok(())
}

pub fn execute_status(json: bool) -> Result<(), Error> {
    if json {
        use nite_core::rpc;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let status_json = rt.block_on(async {
            if let Ok(mut client) = rpc::get_client().await {
                let request = tonic::Request::new(rpc::StatusRequest {});
                if let Ok(resp) = client.get_status(request).await {
                    let s = resp.into_inner();
                    Some(serde_json::json!({
                        "running": s.online,
                        "paused": s.paused,
                        "hook_listener_running": s.hook_listener_running,
                        "last_hook_error": if s.last_hook_error.is_empty() {
                            serde_json::Value::Null
                        } else {
                            serde_json::Value::String(s.last_hook_error)
                        },
                        "keyboard_capture": s.keyboard_capture,
                    }))
                } else {
                    None
                }
            } else {
                None
            }
        });
        match status_json {
            Some(json) => println!("{}", json),
            None => println!("{}", serde_json::json!({"running": false})),
        }
    } else {
        nite_core::service::status()?;
    }
    Ok(())
}

fn start_on_boot_preference() -> Result<bool, Error> {
    // Open the DB (idempotent: initializes schema + seeds if needed) and
    // read the user's start_on_boot preference before handing off to
    // the platform service layer.
    use nite_core::db::init;
    use nite_core::settings::SettingsManager;
    let conn = init::setup()?;
    Ok(SettingsManager::new(&conn).load_all().start_on_boot)
}
