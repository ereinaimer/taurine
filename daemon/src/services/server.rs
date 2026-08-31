use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use taurine_core::engine::EngineState;
use taurine_core::rpc::{
    PauseRequest, PauseResponse, ReloadRequest, ReloadResponse, ResumeRequest, ResumeResponse,
    ShutdownRequest, ShutdownResponse, StatusRequest, StatusResponse,
    daemon_control_server::DaemonControl,
};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status};
use tracing::debug;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcServerSettings {
    pub rpc_mode: taurine_core::settings::RpcMode,
    pub rpc_host: String,
    pub rpc_port: u16,
}

pub struct DaemonService {
    shutdown_sender: mpsc::Sender<()>,
    state: Arc<EngineState>,
    paused: Arc<AtomicBool>,
    pause_notifications_enabled: Arc<AtomicBool>,
    pause_hotkey_spec: Arc<RwLock<crate::input::hotkey::HotkeySpec>>,
    pause_hotkey_display: Arc<RwLock<String>>,
    spinner_style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    pause_audio_enabled: Arc<AtomicBool>,
    system_tray_enabled: Arc<AtomicBool>,
    hook_health: crate::input::hook_health::HookHealth,
    active_rpc_settings: Arc<RwLock<RpcServerSettings>>,
    rpc_reload_sender: mpsc::Sender<()>,
    pause_transition_tx: mpsc::Sender<bool>,
}

impl DaemonService {
    pub fn builder() -> DaemonServiceBuilder {
        DaemonServiceBuilder::new()
    }
}

pub struct DaemonServiceBuilder {
    shutdown_sender: Option<mpsc::Sender<()>>,
    state: Option<Arc<EngineState>>,
    paused: Option<Arc<AtomicBool>>,
    pause_notifications_enabled: Option<Arc<AtomicBool>>,
    pause_hotkey_spec: Option<Arc<RwLock<crate::input::hotkey::HotkeySpec>>>,
    pause_hotkey_display: Option<Arc<RwLock<String>>>,
    spinner_style: Option<Arc<RwLock<taurine_core::settings::SpinnerStyle>>>,
    pause_audio_enabled: Option<Arc<AtomicBool>>,
    system_tray_enabled: Option<Arc<AtomicBool>>,
    hook_health: Option<crate::input::hook_health::HookHealth>,
    active_rpc_settings: Option<Arc<RwLock<RpcServerSettings>>>,
    rpc_reload_sender: Option<mpsc::Sender<()>>,
    pause_transition_tx: Option<mpsc::Sender<bool>>,
}

impl Default for DaemonServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonServiceBuilder {
    pub fn new() -> Self {
        Self {
            shutdown_sender: None,
            state: None,
            paused: None,
            pause_notifications_enabled: None,
            pause_hotkey_spec: None,
            pause_hotkey_display: None,
            spinner_style: None,
            pause_audio_enabled: None,
            system_tray_enabled: None,
            hook_health: None,
            active_rpc_settings: None,
            rpc_reload_sender: None,
            pause_transition_tx: None,
        }
    }

    pub fn shutdown_sender(mut self, sender: mpsc::Sender<()>) -> Self {
        self.shutdown_sender = Some(sender);
        self
    }

    pub fn state(mut self, state: Arc<EngineState>) -> Self {
        self.state = Some(state);
        self
    }

    pub fn paused(mut self, paused: Arc<AtomicBool>) -> Self {
        self.paused = Some(paused);
        self
    }

    pub fn pause_notifications_enabled(mut self, enabled: Arc<AtomicBool>) -> Self {
        self.pause_notifications_enabled = Some(enabled);
        self
    }

    pub fn pause_hotkey_spec(
        mut self,
        spec: Arc<RwLock<crate::input::hotkey::HotkeySpec>>,
    ) -> Self {
        self.pause_hotkey_spec = Some(spec);
        self
    }

    pub fn pause_hotkey_display(mut self, display: Arc<RwLock<String>>) -> Self {
        self.pause_hotkey_display = Some(display);
        self
    }

    pub fn spinner_style(
        mut self,
        style: Arc<RwLock<taurine_core::settings::SpinnerStyle>>,
    ) -> Self {
        self.spinner_style = Some(style);
        self
    }

    pub fn pause_audio_enabled(mut self, enabled: Arc<AtomicBool>) -> Self {
        self.pause_audio_enabled = Some(enabled);
        self
    }

    pub fn system_tray_enabled(mut self, enabled: Arc<AtomicBool>) -> Self {
        self.system_tray_enabled = Some(enabled);
        self
    }

    pub fn hook_health(mut self, hook_health: crate::input::hook_health::HookHealth) -> Self {
        self.hook_health = Some(hook_health);
        self
    }

    pub fn active_rpc_settings(mut self, settings: Arc<RwLock<RpcServerSettings>>) -> Self {
        self.active_rpc_settings = Some(settings);
        self
    }

    pub fn rpc_reload_sender(mut self, sender: mpsc::Sender<()>) -> Self {
        self.rpc_reload_sender = Some(sender);
        self
    }

    pub fn pause_transition_tx(mut self, tx: mpsc::Sender<bool>) -> Self {
        self.pause_transition_tx = Some(tx);
        self
    }

    pub fn build(self) -> Result<DaemonService, String> {
        Ok(DaemonService {
            shutdown_sender: self.shutdown_sender.ok_or("shutdown_sender is required")?,
            state: self.state.ok_or("state is required")?,
            paused: self.paused.ok_or("paused is required")?,
            pause_notifications_enabled: self
                .pause_notifications_enabled
                .ok_or("pause_notifications_enabled is required")?,
            pause_hotkey_spec: self
                .pause_hotkey_spec
                .ok_or("pause_hotkey_spec is required")?,
            pause_hotkey_display: self
                .pause_hotkey_display
                .ok_or("pause_hotkey_display is required")?,
            spinner_style: self.spinner_style.ok_or("spinner_style is required")?,
            pause_audio_enabled: self
                .pause_audio_enabled
                .ok_or("pause_audio_enabled is required")?,
            system_tray_enabled: self
                .system_tray_enabled
                .ok_or("system_tray_enabled is required")?,
            hook_health: self.hook_health.ok_or("hook_health is required")?,
            active_rpc_settings: self
                .active_rpc_settings
                .ok_or("active_rpc_settings is required")?,
            rpc_reload_sender: self
                .rpc_reload_sender
                .ok_or("rpc_reload_sender is required")?,
            pause_transition_tx: self
                .pause_transition_tx
                .ok_or("pause_transition_tx is required")?,
        })
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
        let hook_health = self.hook_health.snapshot();
        let keyboard_capture = hook_health.keyboard_capture_state().as_str().to_string();
        let recovery_suggestion = hook_health.recovery_suggestion().unwrap_or_default();
        let last_hook_error = hook_health.last_hook_error.clone().unwrap_or_default();

        Ok(Response::new(StatusResponse {
            online: true,
            paused: self.paused.load(Ordering::Relaxed),
            pause_hotkey,
            keyboard_capture,
            hook_listener_running: hook_health.listener_running,
            hook_thread_started_at_unix_ms: hook_health.hook_thread_started_at_unix_ms,
            last_keyboard_event_at_unix_ms: hook_health.last_keyboard_event_at_unix_ms,
            last_hook_error,
            recovery_suggestion,
        }))
    }

    async fn shutdown(
        &self,
        _request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        debug!("Received gRPC shutdown request, signaling background process...");
        let _ = self.shutdown_sender.send(()).await;
        Ok(Response::new(ShutdownResponse { success: true }))
    }

    async fn reload(
        &self,
        _request: Request<ReloadRequest>,
    ) -> Result<Response<ReloadResponse>, Status> {
        debug!("Received gRPC reload request, refreshing snippets and settings...");

        let settings = {
            let conn = taurine_core::db::init::setup()
                .map_err(|e| Status::internal(format!("Database connection failed: {}", e)))?;

            // 1. Reload Snippets
            let active = taurine_core::db::crud::get_all_active_triggers(&conn)
                .map_err(|e| Status::internal(format!("Failed to retrieve triggers: {}", e)))?;
            let hotkeys =
                taurine_core::db::crud::get_all_active_hotkey_triggers(&conn).map_err(|e| {
                    Status::internal(format!("Failed to retrieve hotkey triggers: {}", e))
                })?;
            let regexes =
                taurine_core::db::crud::get_all_active_regex_triggers(&conn).map_err(|e| {
                    Status::internal(format!("Failed to retrieve regex triggers: {}", e))
                })?;

            self.state.load_actions(active);
            self.state.load_hotkey_actions(hotkeys);
            self.state.load_regex_actions(regexes);

            // 3. Reload Settings
            use taurine_core::settings::SettingsManager;
            let settings_manager = SettingsManager::new(&conn);
            let settings = settings_manager.load_all();

            taurine_core::settings::set_cached_wpm(settings.wpm);
            taurine_core::settings::set_cached_clipboard_restore_delay(
                settings.clipboard_restore_delay_ms,
            );
            taurine_core::settings::set_cached_script_timeout(settings.script_timeout);
            taurine_core::settings::set_cached_clipboard_history_enabled(
                settings.clipboard_history_enabled,
            );
            taurine_core::settings::set_cached_clipboard_history_retention_secs(
                settings.clipboard_history_retention_secs,
            );
            taurine_core::settings::set_cached_inline_emoji_enabled(settings.inline_emoji_enabled);
            taurine_core::settings::set_cached_inline_emoji_trigger_char(
                settings.inline_emoji_trigger_char,
            );
            taurine_core::settings::set_cached_inline_datetime_enabled(
                settings.inline_datetime_enabled,
            );
            taurine_core::settings::set_cached_inline_datetime_date_format(
                settings.inline_datetime_date_format.clone(),
            );
            taurine_core::settings::set_cached_inline_datetime_time_format(
                settings.inline_datetime_time_format.clone(),
            );
            taurine_core::settings::set_cached_inline_datetime_datetime_format(
                settings.inline_datetime_datetime_format.clone(),
            );
            taurine_core::settings::set_cached_inline_datetime_dialect(
                settings.inline_datetime_dialect.clone(),
            );
            taurine_core::settings::set_cached_inline_datetime_enabled(
                settings.inline_datetime_enabled,
            );
            taurine_core::settings::set_cached_inline_currency_to_words_enabled(
                settings.inline_currency_to_words_enabled,
            );
            taurine_core::settings::set_cached_inline_dictionary_enabled(
                settings.inline_dictionary_enabled,
            );
            taurine_core::settings::set_cached_inline_dictionary_mode(
                settings.inline_dictionary_mode,
            );
            taurine_core::settings::set_cached_scripts_enabled(settings.scripts_enabled);
            taurine_core::settings::set_cached_inline_case_transform_enabled(
                settings.inline_case_transform_enabled,
            );

            self.state
                .inline_datetime_enabled
                .store(settings.inline_datetime_enabled, Ordering::Relaxed);
            self.state
                .inline_currency_to_words_enabled
                .store(settings.inline_currency_to_words_enabled, Ordering::Relaxed);
            self.state
                .set_inline_datetime_date_format(settings.inline_datetime_date_format.clone());
            self.state
                .set_inline_datetime_time_format(settings.inline_datetime_time_format.clone());
            self.state.set_inline_datetime_datetime_format(
                settings.inline_datetime_datetime_format.clone(),
            );
            self.state
                .set_inline_datetime_dialect(settings.inline_datetime_dialect.clone());

            // Update AI triggers
            self.state
                .set_inline_ai_trigger_mode(settings.inline_ai_trigger_mode);
            self.state
                .set_inline_ai_trigger(settings.inline_ai_trigger.clone());
            self.state
                .set_inline_ai_trigger_open(settings.inline_ai_trigger_open.clone());
            self.state
                .set_inline_ai_trigger_close(settings.inline_ai_trigger_close.clone());

            self.state
                .inline_tab_completion_enabled
                .store(settings.inline_tab_completion_enabled, Ordering::Relaxed);
            self.state
                .inline_case_transform_enabled
                .store(settings.inline_case_transform_enabled, Ordering::Relaxed);
            self.state
                .inline_dictionary_enabled
                .store(settings.inline_dictionary_enabled, Ordering::Relaxed);

            self.state
                .instant_expand
                .store(settings.instant_expand, Ordering::Relaxed);
            self.state
                .ignore_fullscreen_enabled
                .store(settings.ignore_fullscreen, Ordering::Relaxed);

            // Update pause notifications (atomic)
            self.pause_notifications_enabled
                .store(settings.pause_notifications_enabled, Ordering::Relaxed);

            self.pause_audio_enabled
                .store(settings.pause_audio_enabled, Ordering::Relaxed);

            self.system_tray_enabled
                .store(settings.system_tray_enabled, Ordering::Relaxed);

            // Update pause hotkey spec (RwLock)
            if let Some(spec) =
                crate::input::hotkey::parse_pause_hotkey_setting(&settings.pause_hotkey)
                && let Ok(mut lock) = self.pause_hotkey_spec.write()
            {
                *lock = spec;
            }

            // Update pause hotkey display (RwLock)
            if let Ok(mut lock) = self.pause_hotkey_display.write() {
                *lock = settings.pause_hotkey.clone();
            }

            // Update spinner style (RwLock)
            if let Ok(mut lock) = self.spinner_style.write() {
                *lock = settings.spinner_style;
            }

            settings
        };

        let rpc_settings_changed = if let Ok(active_rpc) = self.active_rpc_settings.read() {
            let db_rpc = RpcServerSettings {
                rpc_mode: settings.rpc_mode,
                rpc_host: settings.rpc_host.clone(),
                rpc_port: settings.rpc_port,
            };
            *active_rpc != db_rpc
        } else {
            false
        };

        if rpc_settings_changed {
            debug!("RPC settings changed in DB. Triggering gRPC server reload...");
            let _ = self.rpc_reload_sender.try_send(());
        }

        debug!("Successfully reloaded snippets and settings into service.");
        taurine_core::settings::set_cached_audio_theme(settings.audio_theme);
        taurine_core::settings::set_cached_audio_volume(settings.audio_volume);
        tokio::spawn(crate::dictionary_manager::check_and_update_dictionary());
        Ok(Response::new(ReloadResponse { success: true }))
    }

    async fn pause(
        &self,
        _request: Request<PauseRequest>,
    ) -> Result<Response<PauseResponse>, Status> {
        debug!("Received gRPC pause request.");
        let was_paused = self.paused.swap(true, Ordering::Relaxed);
        if !was_paused {
            let _ = self.pause_transition_tx.try_send(true);
        }
        Ok(Response::new(PauseResponse { success: true }))
    }

    async fn resume(
        &self,
        _request: Request<ResumeRequest>,
    ) -> Result<Response<ResumeResponse>, Status> {
        debug!("Received gRPC resume request.");
        let was_paused = self.paused.swap(false, Ordering::Relaxed);
        if was_paused {
            let _ = self.pause_transition_tx.try_send(false);
        }
        Ok(Response::new(ResumeResponse { success: true }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use taurine_core::db::crud::{TriggerType, add_trigger, upsert_trigger_with_type};
    use taurine_core::db::init;
    use taurine_core::engine::EngineState;
    use tokio::sync::mpsc;
    use tonic::Request;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // TEST_LOCK must be held for the full test to serialize env var mutation
    async fn test_daemon_reload_syncs_with_db() {
        let _lock = crate::hook::tests::TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // Setup tracing + env override for test dir
        taurine_core::logs::init_tracing_for_tests();
        let test_dir = std::env::temp_dir().join("taurine_reload_test");
        // SAFETY: Setting environment variable for database isolation in server test.
        unsafe { std::env::set_var("TAURINE_DATA_DIR", test_dir.to_str().unwrap()) };
        let test_db = test_dir.join("test_taurine.db");
        unsafe { std::env::set_var("TAURINE_DB_PATH", test_db.to_str().unwrap()) };
        let _ = std::fs::remove_dir_all(&test_dir);
        std::fs::create_dir_all(&test_dir).unwrap();

        let conn = init::setup().expect("Failed to setup DB");

        let state = Arc::new(EngineState::new());
        let (tx, _rx) = mpsc::channel(1);
        let pause_hotkey = "Alt + `".to_string();
        let pause_hotkey_spec = Arc::new(std::sync::RwLock::new(
            crate::input::hotkey::parse_pause_hotkey_setting(&pause_hotkey).unwrap(),
        ));
        let (reload_tx, _reload_rx) = mpsc::channel(1);
        let active_rpc_settings = Arc::new(std::sync::RwLock::new(RpcServerSettings {
            rpc_mode: taurine_core::settings::RpcMode::Tcp,
            rpc_host: String::new(),
            rpc_port: 0,
        }));

        let (pause_tx, _pause_rx) = mpsc::channel(1);
        let service = DaemonService::builder()
            .shutdown_sender(tx)
            .state(state.clone())
            .paused(Arc::new(AtomicBool::new(false)))
            .pause_notifications_enabled(Arc::new(AtomicBool::new(false)))
            .pause_hotkey_spec(pause_hotkey_spec)
            .pause_hotkey_display(Arc::new(std::sync::RwLock::new(pause_hotkey)))
            .spinner_style(Arc::new(std::sync::RwLock::new(
                taurine_core::settings::SpinnerStyle::default(),
            )))
            .pause_audio_enabled(Arc::new(AtomicBool::new(true)))
            .system_tray_enabled(Arc::new(AtomicBool::new(true)))
            .hook_health(crate::input::hook_health::HookHealth::new())
            .active_rpc_settings(active_rpc_settings)
            .rpc_reload_sender(reload_tx)
            .pause_transition_tx(pause_tx)
            .build()
            .expect("builder call site is fully populated");

        // Initially state should be empty
        assert_eq!(state.fetch_expansion("hello", None), None);

        // Add a snippet to DB
        add_trigger(&conn, "hello", "world", "all", None, None, None).expect("Failed to add to DB");

        // trigger reload directly via gRPC service method
        let req = Request::new(taurine_core::rpc::ReloadRequest {});
        let res = service.reload(req).await.expect("Reload failed");

        assert!(
            res.into_inner().success,
            "Reload response should be success"
        );

        // Now the in-memory cache should have the expansion
        let expansion = state
            .fetch_expansion("hello", None)
            .expect("reload should repopulate cache");
        assert_eq!(
            expansion.steps,
            vec![taurine_core::engine::variables::ExpansionStep::Text(
                "world".to_string()
            )]
        );
        assert!(state.inline_tab_completion_enabled.load(Ordering::Relaxed));
        assert!(state.get_hotkey_action("ctrl+shift+g").is_none());

        upsert_trigger_with_type(
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
        assert!(state.fetch_expansion("ctrl+shift+g", None).is_none());

        taurine_core::settings::SettingsManager::new(&conn)
            .update_setting("inline_tab_completion_enabled", false)
            .unwrap();

        let req = Request::new(taurine_core::rpc::ReloadRequest {});
        service.reload(req).await.expect("Reload failed");

        assert!(!state.inline_tab_completion_enabled.load(Ordering::Relaxed));

        // Cleanup
        let _ = std::fs::remove_dir_all(&test_dir);
        unsafe { std::env::remove_var("TAURINE_DATA_DIR") };
        unsafe { std::env::remove_var("TAURINE_DB_PATH") };
    }
}
