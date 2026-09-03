// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

pub mod ui;

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Frame;
use taurine_core::ai::{AiProvider, CredentialStore, OsKeyringStore, configured_providers};
use taurine_core::db::init;
use taurine_core::error::Result as CoreResult;
use taurine_core::settings::SettingsManager;

use super::OverlaySession;

#[derive(Debug, Clone)]
pub struct ConfiguredProviderInfo {
    pub provider: AiProvider,
    pub is_active: bool,
    pub model: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddField {
    Provider,
    Endpoint,
    ApiKey,
    Model,
    Confirm,
}

#[derive(Debug, Clone)]
pub struct AddModalState {
    pub provider_index: usize,
    pub focus: AddField,
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub error_msg: Option<String>,
}

impl AddModalState {
    pub fn new() -> Self {
        let first_provider = AiProvider::ALL[0];
        Self {
            provider_index: 0,
            focus: AddField::Provider,
            api_key: String::new(),
            endpoint: "http://localhost:11434/v1".to_string(),
            model: first_provider.default_model().to_string(),
            error_msg: None,
        }
    }

    pub fn selected_provider(&self) -> AiProvider {
        AiProvider::ALL[self.provider_index % AiProvider::ALL.len()]
    }

    pub fn select_next_provider(&mut self) {
        self.provider_index = (self.provider_index + 1) % AiProvider::ALL.len();
        self.model = self.selected_provider().default_model().to_string();
    }

    pub fn select_prev_provider(&mut self) {
        if self.provider_index == 0 {
            self.provider_index = AiProvider::ALL.len() - 1;
        } else {
            self.provider_index -= 1;
        }
        self.model = self.selected_provider().default_model().to_string();
    }

    pub fn next_field(&mut self) {
        self.error_msg = None;
        let is_custom = self.selected_provider() == AiProvider::Custom;
        self.focus = match self.focus {
            AddField::Provider => {
                if is_custom {
                    AddField::Endpoint
                } else {
                    AddField::ApiKey
                }
            }
            AddField::Endpoint => AddField::ApiKey,
            AddField::ApiKey => AddField::Model,
            AddField::Model => AddField::Confirm,
            AddField::Confirm => AddField::Provider,
        };
    }

    pub fn prev_field(&mut self) {
        self.error_msg = None;
        let is_custom = self.selected_provider() == AiProvider::Custom;
        self.focus = match self.focus {
            AddField::Provider => AddField::Confirm,
            AddField::Endpoint => AddField::Provider,
            AddField::ApiKey => {
                if is_custom {
                    AddField::Endpoint
                } else {
                    AddField::Provider
                }
            }
            AddField::Model => AddField::ApiKey,
            AddField::Confirm => AddField::Model,
        };
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> AddModalAction {
        match key.code {
            KeyCode::Esc => AddModalAction::Cancel,
            KeyCode::Tab => {
                self.next_field();
                AddModalAction::None
            }
            KeyCode::BackTab => {
                self.prev_field();
                AddModalAction::None
            }
            _ => match self.focus {
                AddField::Provider => {
                    match key.code {
                        KeyCode::Char('j') | KeyCode::Down => self.select_next_provider(),
                        KeyCode::Char('k') | KeyCode::Up => self.select_prev_provider(),
                        KeyCode::Enter => self.next_field(),
                        _ => {}
                    }
                    AddModalAction::None
                }
                AddField::Endpoint => {
                    match key.code {
                        KeyCode::Backspace => {
                            self.endpoint.pop();
                        }
                        KeyCode::Enter => self.next_field(),
                        KeyCode::Char(c) => self.endpoint.push(c),
                        _ => {}
                    }
                    AddModalAction::None
                }
                AddField::ApiKey => {
                    match key.code {
                        KeyCode::Backspace => {
                            self.api_key.pop();
                        }
                        KeyCode::Enter => self.next_field(),
                        KeyCode::Char(c) => self.api_key.push(c),
                        _ => {}
                    }
                    AddModalAction::None
                }
                AddField::Model => {
                    match key.code {
                        KeyCode::Backspace => {
                            self.model.pop();
                        }
                        KeyCode::Enter => return AddModalAction::Save,
                        KeyCode::Char(c) => self.model.push(c),
                        _ => {}
                    }
                    AddModalAction::None
                }
                AddField::Confirm => {
                    if key.code == KeyCode::Enter {
                        AddModalAction::Save
                    } else {
                        AddModalAction::None
                    }
                }
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddModalAction {
    None,
    Cancel,
    Save,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditModelModalAction {
    None,
    Cancel,
    Save(String),
}

impl Default for AddModalState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct EditModelModalState {
    pub provider: AiProvider,
    pub model: String,
}

impl EditModelModalState {
    pub fn handle_key(&mut self, key: KeyEvent) -> EditModelModalAction {
        match key.code {
            KeyCode::Esc => EditModelModalAction::Cancel,
            KeyCode::Backspace => {
                self.model.pop();
                EditModelModalAction::None
            }
            KeyCode::Char(c) => {
                self.model.push(c);
                EditModelModalAction::None
            }
            KeyCode::Enter => EditModelModalAction::Save(self.model.trim().to_string()),
            _ => EditModelModalAction::None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ModalView {
    None,
    Add(AddModalState),
    EditModel(EditModelModalState),
    DeleteConfirm(AiProvider),
}

pub struct AiWizardState {
    pub providers: Vec<ConfiguredProviderInfo>,
    pub selected_index: usize,
    pub modal: ModalView,
    pub status_message: Option<(String, bool)>,
    pub should_exit: bool,
}

impl AiWizardState {
    pub fn new() -> CoreResult<Self> {
        let mut state = Self {
            providers: Vec::new(),
            selected_index: 0,
            modal: ModalView::None,
            status_message: None,
            should_exit: false,
        };
        state.refresh_providers()?;
        Ok(state)
    }

    pub fn refresh_providers(&mut self) -> CoreResult<()> {
        let conn = init::setup()?;
        let settings = SettingsManager::new(&conn).load_all();
        let configured = configured_providers(&OsKeyringStore)?;

        let mut list = Vec::new();
        for provider in configured {
            let is_active = settings
                .ai_provider
                .as_deref()
                .map(|p| p.eq_ignore_ascii_case(provider.as_str()))
                .unwrap_or(false);

            let model = if is_active {
                settings
                    .ai_model
                    .clone()
                    .unwrap_or_else(|| provider.default_model().to_string())
            } else {
                provider.default_model().to_string()
            };

            list.push(ConfiguredProviderInfo {
                provider,
                is_active,
                model,
            });
        }

        // Sort so active provider is first, then alphabetical
        list.sort_by(|a, b| match (a.is_active, b.is_active) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.provider.display_name().cmp(b.provider.display_name()),
        });

        self.providers = list;
        if self.selected_index >= self.providers.len() {
            self.selected_index = self.providers.len().saturating_sub(1);
        }

        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> CoreResult<()> {
        match &mut self.modal {
            ModalView::None => self.handle_dashboard_key(key),
            ModalView::Add(modal_state) => match modal_state.handle_key(key) {
                AddModalAction::None => Ok(()),
                AddModalAction::Cancel => {
                    self.modal = ModalView::None;
                    Ok(())
                }
                AddModalAction::Save => {
                    let provider = modal_state.selected_provider();
                    let key = modal_state.api_key.trim();

                    if provider != AiProvider::Custom && key.is_empty() {
                        modal_state.error_msg = Some("API key cannot be empty".to_string());
                        modal_state.focus = AddField::ApiKey;
                        return Ok(());
                    }

                    let api_key = key.to_string();
                    let model = modal_state.model.trim().to_string();
                    let endpoint = modal_state.endpoint.trim().to_string();
                    self.save_new_provider(provider, &api_key, &model, &endpoint)
                }
            },
            ModalView::EditModel(edit_state) => {
                let provider = edit_state.provider;
                match edit_state.handle_key(key) {
                    EditModelModalAction::None => Ok(()),
                    EditModelModalAction::Cancel => {
                        self.modal = ModalView::None;
                        Ok(())
                    }
                    EditModelModalAction::Save(model) => self.save_edited_model(provider, &model),
                }
            }
            ModalView::DeleteConfirm(provider) => {
                let p = *provider;
                self.handle_delete_modal_key(key, p)
            }
        }
    }

    fn handle_dashboard_key(&mut self, key: KeyEvent) -> CoreResult<()> {
        self.status_message = None;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_exit = true;
            }
            KeyCode::Char('j') | KeyCode::Down if !self.providers.is_empty() => {
                self.selected_index = (self.selected_index + 1).min(self.providers.len() - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            KeyCode::Char('n') => {
                self.modal = ModalView::Add(AddModalState::new());
            }
            KeyCode::Enter => {
                if let Some(item) = self.providers.get(self.selected_index) {
                    let provider = item.provider;
                    let model = item.model.clone();

                    // Activate this provider in DB
                    let conn = init::setup()?;
                    let manager = SettingsManager::new(&conn);
                    manager.update_setting("ai_provider", provider.as_str())?;
                    manager.update_setting("ai_model", &model)?;
                    self.refresh_providers()?;

                    // Open edit modal so user can change model if desired
                    self.modal = ModalView::EditModel(EditModelModalState { provider, model });
                }
            }
            KeyCode::Char('d') => {
                if let Some(item) = self.providers.get(self.selected_index) {
                    self.modal = ModalView::DeleteConfirm(item.provider);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn save_new_provider(
        &mut self,
        provider: AiProvider,
        api_key: &str,
        model: &str,
        endpoint: &str,
    ) -> CoreResult<()> {
        // Save key to OS keyring
        OsKeyringStore.set_secret(provider, api_key)?;

        // Update database settings
        let conn = init::setup()?;
        let manager = SettingsManager::new(&conn);
        manager.update_setting("ai_provider", provider.as_str())?;

        let model_str = if model.is_empty() {
            provider.default_model()
        } else {
            model
        };
        manager.update_setting("ai_model", model_str)?;

        if provider == AiProvider::Custom && !endpoint.is_empty() {
            manager.update_setting("ai_custom_endpoint", endpoint)?;
        }

        self.refresh_providers()?;
        self.status_message = Some((
            format!("Configured and activated {}", provider.display_name()),
            false,
        ));
        self.modal = ModalView::None;
        Ok(())
    }

    fn save_edited_model(&mut self, provider: AiProvider, model: &str) -> CoreResult<()> {
        let model_str = if model.is_empty() {
            provider.default_model()
        } else {
            model
        };

        let conn = init::setup()?;
        let manager = SettingsManager::new(&conn);
        manager.update_setting("ai_provider", provider.as_str())?;
        manager.update_setting("ai_model", model_str)?;

        self.refresh_providers()?;
        self.status_message = Some((
            format!(
                "Updated {} model to '{}'",
                provider.display_name(),
                model_str
            ),
            false,
        ));
        self.modal = ModalView::None;
        Ok(())
    }

    fn handle_delete_modal_key(&mut self, key: KeyEvent, provider: AiProvider) -> CoreResult<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                OsKeyringStore.delete_secret(provider)?;

                // If active provider was deleted, unset it or point to remaining
                let conn = init::setup()?;
                let manager = SettingsManager::new(&conn);
                let settings = manager.load_all();
                if let Some(active) = settings.ai_provider
                    && active.eq_ignore_ascii_case(provider.as_str())
                {
                    let remaining = configured_providers(&OsKeyringStore)?;
                    if let Some(next) = remaining.first() {
                        manager.update_setting("ai_provider", next.as_str())?;
                        manager.update_setting("ai_model", next.default_model())?;
                    } else {
                        manager.update_setting("ai_provider", "")?;
                        manager.update_setting("ai_model", "")?;
                    }
                }

                self.refresh_providers()?;
                self.status_message = Some((
                    format!("Removed credentials for {}", provider.display_name()),
                    false,
                ));
                self.modal = ModalView::None;
            }
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.modal = ModalView::None;
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn run_ai_overlay() -> CoreResult<()> {
    let mut session = OverlaySession::new()
        .map_err(|e| taurine_core::Error::Service(format!("Failed to initialize overlay: {e}")))?;
    let mut state = AiWizardState::new()?;
    let mut last_move = Instant::now();
    const MOVE_DEBOUNCE: Duration = Duration::from_millis(80);

    loop {
        if state.should_exit {
            break;
        }

        session.terminal.draw(|f: &mut Frame| {
            ui::render_ai_wizard(f, &state);
        })?;

        let event = crossterm::event::read()
            .map_err(|e| taurine_core::Error::Service(format!("Overlay event read failed: {e}")))?;

        if let Event::Key(key) = event
            && matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
        {
            let is_debounced = matches!(
                key.code,
                KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k')
            );
            if is_debounced && last_move.elapsed() < MOVE_DEBOUNCE {
                continue;
            }
            if is_debounced {
                last_move = Instant::now();
            }
            state.handle_key(key)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_add_modal_field_navigation_standard() {
        let mut modal = AddModalState::new();
        assert_eq!(modal.focus, AddField::Provider);

        modal.handle_key(key(KeyCode::Tab));
        assert_eq!(modal.focus, AddField::ApiKey);

        modal.handle_key(key(KeyCode::Tab));
        assert_eq!(modal.focus, AddField::Model);

        modal.handle_key(key(KeyCode::Tab));
        assert_eq!(modal.focus, AddField::Confirm);

        modal.handle_key(key(KeyCode::Tab));
        assert_eq!(modal.focus, AddField::Provider);

        modal.handle_key(key(KeyCode::BackTab));
        assert_eq!(modal.focus, AddField::Confirm);
    }

    #[test]
    fn test_add_modal_field_navigation_custom_provider() {
        let mut modal = AddModalState::new();
        while modal.selected_provider() != AiProvider::Custom {
            modal.select_next_provider();
        }

        assert_eq!(modal.focus, AddField::Provider);
        modal.handle_key(key(KeyCode::Tab));
        assert_eq!(modal.focus, AddField::Endpoint);

        modal.handle_key(key(KeyCode::Tab));
        assert_eq!(modal.focus, AddField::ApiKey);

        modal.handle_key(key(KeyCode::BackTab));
        assert_eq!(modal.focus, AddField::Endpoint);
    }

    #[test]
    fn test_add_modal_provider_selection_updates_model() {
        let mut modal = AddModalState::new();
        let initial_provider = modal.selected_provider();
        let initial_model = modal.model.clone();

        modal.handle_key(key(KeyCode::Char('j')));
        assert_ne!(modal.selected_provider(), initial_provider);
        assert_ne!(modal.model, initial_model);
        assert_eq!(modal.model, modal.selected_provider().default_model());

        modal.handle_key(key(KeyCode::Char('k')));
        assert_eq!(modal.selected_provider(), initial_provider);
        assert_eq!(modal.model, initial_model);
    }

    #[test]
    fn test_edit_model_modal_state_key_handling() {
        let mut edit = EditModelModalState {
            provider: AiProvider::Openai,
            model: "gpt-4o".to_string(),
        };

        let action = edit.handle_key(key(KeyCode::Backspace));
        assert_eq!(action, EditModelModalAction::None);
        assert_eq!(edit.model, "gpt-4");

        let action = edit.handle_key(key(KeyCode::Char('1')));
        assert_eq!(action, EditModelModalAction::None);
        assert_eq!(edit.model, "gpt-41");

        let action = edit.handle_key(key(KeyCode::Enter));
        assert_eq!(action, EditModelModalAction::Save("gpt-41".to_string()));

        let action = edit.handle_key(key(KeyCode::Esc));
        assert_eq!(action, EditModelModalAction::Cancel);
    }

    #[test]
    fn test_dashboard_exit_on_q_or_esc() {
        let mut state = AiWizardState {
            providers: Vec::new(),
            selected_index: 0,
            modal: ModalView::None,
            status_message: None,
            should_exit: false,
        };

        state.handle_key(key(KeyCode::Char('q'))).unwrap();
        assert!(state.should_exit);

        state.should_exit = false;
        state.handle_key(key(KeyCode::Esc)).unwrap();
        assert!(state.should_exit);
    }

    #[test]
    fn test_dashboard_open_add_modal_on_n() {
        let mut state = AiWizardState {
            providers: Vec::new(),
            selected_index: 0,
            modal: ModalView::None,
            status_message: None,
            should_exit: false,
        };

        state.handle_key(key(KeyCode::Char('n'))).unwrap();
        assert!(matches!(state.modal, ModalView::Add(_)));
    }
}
