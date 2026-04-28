use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use taurine_core::engine::EngineState;
use taurine_core::rpc::{
    ReloadRequest, ReloadResponse, ShutdownRequest, ShutdownResponse, StatusRequest,
    StatusResponse, daemon_control_server::DaemonControl,
};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use tracing::info;

pub struct DaemonService {
    shutdown_sender: mpsc::Sender<()>,
    state: Arc<EngineState>,
    paused: Arc<AtomicBool>,
    pause_notifications_enabled: Arc<AtomicBool>,
    pause_hotkey_spec: Arc<RwLock<crate::hotkey::HotkeySpec>>,
    pause_hotkey_display: Arc<RwLock<String>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
}

impl DaemonService {
    pub fn new(
        shutdown_sender: mpsc::Sender<()>,
        state: Arc<EngineState>,
        paused: Arc<AtomicBool>,
        pause_notifications_enabled: Arc<AtomicBool>,
        pause_hotkey_spec: Arc<RwLock<crate::hotkey::HotkeySpec>>,
        pause_hotkey_display: Arc<RwLock<String>>,
        spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    ) -> Self {
        Self {
            shutdown_sender,
            state,
            paused,
            pause_notifications_enabled,
            pause_hotkey_spec,
            pause_hotkey_display,
            spinner_style,
        }
    }
}

#[tonic::async_trait]
impl DaemonControl for DaemonService {
    async fn get_status(
        &self,
        _request: Request<StatusRequest>,
    ) -> Result<Response<StatusResponse>, Status> {
        let pause_hotkey = self
            .pause_hotkey_display
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "Unknown".to_string());

        Ok(Response::new(StatusResponse {
            online: true,
            paused: self.paused.load(Ordering::Relaxed),
            pause_hotkey,
        }))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        info!("Received gRPC shutdown request, signaling background process...");
        let _ = self.shutdown_sender.send(()).await;
        Ok(Response::new(ShutdownResponse { success: true }))
    }

    async fn reload(
        &self,
        _request: Request<ReloadRequest>,
    ) -> Result<Response<ReloadResponse>, Status> {
        info!("Received gRPC reload request, refreshing snippets and settings...");

        let conn = taurine_core::db::init::setup()
            .map_err(|e| Status::internal(format!("Database connection failed: {}", e)))?;

        // 1. Reload Snippets
        let active = taurine_core::db::crud::get_all_active_automations(&conn)
            .map_err(|e| Status::internal(format!("Failed to retrieve automations: {}", e)))?;
        let history = taurine_core::db::crud::get_active_word_trigger_history(&conn)
            .map_err(|e| Status::internal(format!("Failed to retrieve trigger history: {}", e)))?;
        let hotkeys =
            taurine_core::db::crud::get_all_active_hotkey_automations(&conn).map_err(|e| {
                Status::internal(format!("Failed to retrieve hotkey automations: {}", e))
            })?;

        self.state.load_actions(active);
        self.state.load_word_trigger_history(history);
        self.state.load_hotkey_actions(hotkeys);

        // 2. Reload AI Presets
        let presets = taurine_core::db::crud::ai_presets::list_presets(&conn)
            .map_err(|e| Status::internal(format!("Failed to retrieve AI presets: {}", e)))?;
        let presets_map: Vec<(String, String)> =
            presets.into_iter().map(|p| (p.name, p.prompt)).collect();
        self.state.load_ai_presets(presets_map);

        // 3. Reload Settings
        use taurine_core::settings::SettingsManager;
        let settings_manager = SettingsManager::new(&conn);
        let settings = settings_manager.load_all();

        // Update trigger char (atomic)
        self.state
            .trigger_char
            .store(settings.trigger_char as u32, Ordering::Relaxed);

        // Update inline AI delimiter (atomic)
        self.state
            .inline_ai_delimiter
            .store(settings.inline_ai_delimiter as u32, Ordering::Relaxed);

        self.state
            .inline_tab_completion_enabled
            .store(settings.inline_tab_completion_enabled, Ordering::Relaxed);
        self.state
            .inline_history_enabled
            .store(settings.inline_history_enabled, Ordering::Relaxed);

        // Update pause notifications (atomic)
        self.pause_notifications_enabled
            .store(settings.pause_notifications_enabled, Ordering::Relaxed);

        // Update pause hotkey spec (RwLock)
        if let Some(spec) = crate::hotkey::parse_pause_hotkey_setting(&settings.pause_hotkey)
            && let Ok(mut lock) = self.pause_hotkey_spec.write()
        {
            *lock = spec;
        }

        // Update pause hotkey display (RwLock)
        if let Ok(mut lock) = self.pause_hotkey_display.write() {
            *lock = settings.pause_hotkey;
        }

        // Update spinner style (RwLock)
        if let Ok(mut lock) = self.spinner_style.write() {
            *lock = settings.spinner_style;
        }

        info!("Successfully reloaded snippets and settings into daemon.");
        Ok(Response::new(ReloadResponse { success: true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use taurine_core::db::crud::{
        TriggerType, add_automation_by_trigger, upsert_automation_with_trigger_type,
    };
    use taurine_core::db::init;
    use taurine_core::engine::EngineState;
    use tokio::sync::mpsc;
    use tonic::Request;

    #[tokio::test]
    async fn test_daemon_reload_syncs_with_db() {
        // Setup tracing + env override for test dir
        taurine_core::logs::init_tracing_for_tests();
        let test_dir = std::env::temp_dir().join("taurine_reload_test");
        unsafe { std::env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };
        let test_db = test_dir.join("test_taurine.db");
        unsafe { std::env::set_var("TAURINE_DB_PATH", test_db.to_str().unwrap()) };
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();

        let conn = init::setup().expect("Failed to setup DB");

        let state = Arc::new(EngineState::new('>'));
        let (tx, _rx) = mpsc::channel(1);
        let pause_hotkey = "Alt + `".to_string();
        let pause_hotkey_spec = Arc::new(std::sync::RwLock::new(
            crate::hotkey::parse_pause_hotkey_setting(&pause_hotkey).unwrap(),
        ));
        let service = DaemonService::new(
            tx,
            state.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(true)),
            pause_hotkey_spec,
            Arc::new(std::sync::RwLock::new(pause_hotkey)),
            Arc::new(std::sync::RwLock::new(
                taurine_core::settings::SpinnerStyle::default(),
            )),
        );

        // Initially state should be empty
        assert_eq!(state.fetch_expansion("hello"), None);

        // Add a snippet to DB
        add_automation_by_trigger(&conn, "hello", "world", "all").expect("Failed to add to DB");

        // trigger reload directly via gRPC service method
        let req = Request::new(taurine_core::rpc::ReloadRequest {});
        let res = service.reload(req).await.expect("Reload failed");

        assert!(
            res.into_inner().success,
            "Reload response should be success"
        );

        // Now the in-memory cache should have the expansion
        let expansion = state
            .fetch_expansion("hello")
            .expect("reload should repopulate cache");
        assert_eq!(
            expansion.steps,
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                "world".to_string()
            )]
        );
        assert_eq!(
            state.matching_word_trigger_history(""),
            vec!["hello".to_string()]
        );
        assert!(state.inline_tab_completion_enabled.load(Ordering::Relaxed));
        assert!(state.inline_history_enabled.load(Ordering::Relaxed));
        assert!(state.get_hotkey_action("ctrl+shift+g").is_none());

        upsert_automation_with_trigger_type(
            &conn,
            "hotkey-id",
            "Hotkey",
            None,
            TriggerType::Hotkey,
            "ctrl+shift+g",
            "git status",
            "text",
            "all",
            "[]",
            0,
            None,
        )
        .unwrap();

        let req = Request::new(taurine_core::rpc::ReloadRequest {});
        service.reload(req).await.expect("Reload failed");

        assert_eq!(
            state.get_hotkey_action("ctrl+shift+g").unwrap().output,
            "git status"
        );
        assert!(state.fetch_expansion("ctrl+shift+g").is_none());

        taurine_core::settings::SettingsManager::new(&conn)
            .update_setting("inline_tab_completion_enabled", false)
            .unwrap();
        taurine_core::settings::SettingsManager::new(&conn)
            .update_setting("inline_history_enabled", false)
            .unwrap();

        let req = Request::new(taurine_core::rpc::ReloadRequest {});
        service.reload(req).await.expect("Reload failed");

        assert!(!state.inline_tab_completion_enabled.load(Ordering::Relaxed));
        assert!(!state.inline_history_enabled.load(Ordering::Relaxed));

        // Cleanup
        let _ = std::fs::remove_dir_all(&test_dir);
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
        unsafe { std::env::remove_var("TAURINE_DB_PATH") };
    }
}
