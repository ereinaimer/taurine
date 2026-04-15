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
}

impl DaemonService {
    pub fn new(
        shutdown_sender: mpsc::Sender<()>,
        state: Arc<EngineState>,
        paused: Arc<AtomicBool>,
        pause_notifications_enabled: Arc<AtomicBool>,
        pause_hotkey_spec: Arc<RwLock<crate::hotkey::HotkeySpec>>,
        pause_hotkey_display: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            shutdown_sender,
            state,
            paused,
            pause_notifications_enabled,
            pause_hotkey_spec,
            pause_hotkey_display,
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

        let actions = active.into_iter().map(|(t, a)| (t, a));
        self.state.load_actions(actions);

        // 2. Reload Settings
        use taurine_core::settings::SettingsManager;
        let settings_manager = SettingsManager::new(&conn);
        let settings = settings_manager.load_all();

        // Update trigger char (atomic)
        self.state
            .trigger_char
            .store(settings.trigger_char as u32, Ordering::Relaxed);

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

        info!("Successfully reloaded snippets and settings into daemon.");
        Ok(Response::new(ReloadResponse { success: true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use taurine_core::db::crud::add_automation_by_trigger;
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
        );

        // Initially state should be empty
        assert_eq!(state.source.get_action("hello"), None);

        // Add a snippet to DB
        add_automation_by_trigger(&conn, "hello", "world").expect("Failed to add to DB");

        // trigger reload directly via gRPC service method
        let req = Request::new(taurine_core::rpc::ReloadRequest {});
        let res = service.reload(req).await.expect("Reload failed");

        assert!(
            res.into_inner().success,
            "Reload response should be success"
        );

        // Now the in-memory cache should have the expansion
        assert_eq!(state.source.get_action("hello").map(|a| a.output).as_deref(), Some("world"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&test_dir);
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
        unsafe { std::env::remove_var("TAURINE_DB_PATH") };
    }
}
