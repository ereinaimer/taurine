use super::progress::ensure_voice_model_downloaded;
use taurine_core::error::Error;

pub fn execute_up(json: bool) -> Result<(), Error> {
    let settings = {
        let conn = taurine_core::db::init::setup()?;
        taurine_core::settings::SettingsManager::new(&conn).load_all()
    };

    // Dictation models: both on capable machines, light only under 8 GB total.
    // If downloading fails (e.g. offline or network issue), continue starting Taurine
    // without voice capabilities instead of aborting startup.
    let models_to_ensure: Vec<&str> = if settings.voice_model.trim().eq_ignore_ascii_case("auto") {
        if taurine_core::voice::get_system_ram_gb() >= 8 {
            vec!["parakeet-unified-en-0.6b", "parakeet-tdt-ctc-110m"]
        } else {
            vec!["parakeet-tdt-ctc-110m"]
        }
    } else {
        vec![taurine_core::voice::resolve_model_alias(
            &settings.voice_model,
        )]
    };

    for model in models_to_ensure {
        if let Err(e) = ensure_voice_model_downloaded(model, json) {
            tracing::warn!(
                error = %e,
                "Voice model '{model}' unavailable; starting Taurine without voice dictation",
            );
            if !json {
                eprintln!(
                    "Warning: Voice model '{model}' is unavailable. Starting Taurine without voice dictation.",
                );
            }
        }
    }

    // Best-effort cleanup of deprecated voice models
    let models_dir = taurine_core::voice::models_dir();
    taurine_core::voice::prune_deprecated_voice_models(&models_dir);

    // If ambient always-on is enabled, also ensure VAD and KWS models are downloaded
    if settings.voice_always_on {
        if let Err(e) = ensure_voice_model_downloaded("silero_vad_v6", json) {
            tracing::warn!(
                error = %e,
                "Silero VAD model unavailable; starting Taurine without ambient voice triggers"
            );
            if !json {
                eprintln!(
                    "Warning: Silero VAD model is unavailable. Ambient voice triggers will be disabled."
                );
            }
        }
        if let Err(e) = ensure_voice_model_downloaded("kws-zipformer-zh-en-3M", json) {
            tracing::warn!(
                error = %e,
                "Zipformer KWS model unavailable; starting Taurine without ambient voice triggers"
            );
            if !json {
                eprintln!(
                    "Warning: Zipformer KWS model is unavailable. Ambient voice triggers will be disabled."
                );
            }
        }
    }

    taurine_core::service::up(settings.start_on_boot)?;
    if json {
        println!("{}", serde_json::json!({"status": "started"}));
    }
    Ok(())
}

pub fn execute_down(json: bool) -> Result<(), Error> {
    taurine_core::service::down()?;
    if json {
        println!("{}", serde_json::json!({"status": "stopped"}));
    }
    Ok(())
}

pub fn execute_restart(json: bool) -> Result<(), Error> {
    let start_on_boot = start_on_boot_preference()?;
    taurine_core::service::restart(start_on_boot)?;
    if json {
        println!("{}", serde_json::json!({"status": "restarted"}));
    }
    Ok(())
}

pub fn execute_status(json: bool) -> Result<(), Error> {
    if json {
        use taurine_core::rpc;
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
        taurine_core::service::status()?;
    }
    Ok(())
}

fn start_on_boot_preference() -> Result<bool, Error> {
    // Open the DB (idempotent: runs migrations + seeds if needed) and
    // read the user's start_on_boot preference before handing off to
    // the platform service layer.
    use taurine_core::db::init;
    use taurine_core::settings::SettingsManager;
    let conn = init::setup()?;
    Ok(SettingsManager::new(&conn).load_all().start_on_boot)
}
