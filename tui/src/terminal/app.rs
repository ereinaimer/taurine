use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::theme::Theme;
use crate::theme::builtin::{DARK_THEME, LIGHT_THEME};
use crate::widgets::library::LibraryPageState;
use crate::widgets::settings::state::SettingsPageState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Library,
    Settings,
}

impl Page {
    pub(crate) const ALL: [Self; 2] = [Self::Library, Self::Settings];

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Library => "Library",
            Self::Settings => "Settings",
        }
    }

    #[allow(dead_code)]
    pub(crate) const fn nav_index(self) -> usize {
        match self {
            Self::Library => 0,
            Self::Settings => 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct App {
    active_page: Page,
    nav_visible: bool,
    library_page: LibraryPageState,
    settings_page: SettingsPageState,
    should_quit: bool,
    notification: Option<String>,
    current_theme: &'static Theme,
}

impl Default for App {
    fn default() -> Self {
        Self {
            active_page: Page::Library,
            nav_visible: true,
            library_page: LibraryPageState::default(),
            settings_page: SettingsPageState::default(),
            should_quit: false,
            notification: None,
            current_theme: &DARK_THEME,
        }
    }
}

impl App {
    pub(crate) const fn theme(&self) -> &'static Theme {
        self.current_theme
    }
    #[allow(dead_code)]
    pub(crate) fn set_theme(&mut self, theme: &'static Theme) {
        self.current_theme = theme;
    }
    pub(crate) fn toggle_theme(&mut self) {
        self.current_theme = if self.current_theme.dark {
            &LIGHT_THEME
        } else {
            &DARK_THEME
        };
    }

    pub(crate) const fn active_page(&self) -> Page {
        self.active_page
    }

    pub(crate) const fn nav_visible(&self) -> bool {
        self.nav_visible
    }

    pub(crate) const fn library_page(&self) -> &LibraryPageState {
        &self.library_page
    }

    pub(crate) fn library_page_mut(&mut self) -> &mut LibraryPageState {
        &mut self.library_page
    }

    pub(crate) const fn settings_page(&self) -> &SettingsPageState {
        &self.settings_page
    }

    pub(crate) fn settings_page_mut(&mut self) -> &mut SettingsPageState {
        &mut self.settings_page
    }

    pub(crate) const fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub(crate) fn toggle_nav_visibility(&mut self) {
        self.nav_visible = !self.nav_visible;
    }

    pub(crate) fn notification(&self) -> Option<&str> {
        self.notification.as_deref()
    }

    pub(crate) fn clear_notification(&mut self) {
        self.notification = None;
    }

    pub(crate) fn set_notification(&mut self, message: String) {
        self.notification = Some(message);
    }

    pub(crate) fn handle_key_event(&mut self, key: KeyEvent) {
        self.handle_key(key.code, key.modifiers);
    }

    pub(crate) fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match (code, modifiers) {
            (KeyCode::Char('1'), _) => self.active_page = Page::Library,
            (KeyCode::Char('2'), _) => self.active_page = Page::Settings,
            (KeyCode::Char('t'), KeyModifiers::CONTROL) => self.toggle_theme(),
            (KeyCode::Char('q'), KeyModifiers::NONE) => self.should_quit = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_library_page() {
        let app = App::default();
        assert_eq!(app.active_page(), Page::Library);
    }

    #[test]
    fn pressing_one_selects_library() {
        let mut app = App {
            active_page: Page::Settings,
            ..App::default()
        };
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        assert_eq!(app.active_page(), Page::Library);
    }

    #[test]
    fn pressing_two_selects_settings() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(app.active_page(), Page::Settings);
    }

    #[test]
    fn pressing_q_marks_app_for_quit() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('q'), KeyModifiers::NONE);
        assert!(app.should_quit());
    }

    #[test]
    fn pressing_escape_does_not_quit() {
        let mut app = App::default();
        app.handle_key(KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.should_quit());
    }

    #[test]
    fn pressing_ctrl_c_does_not_quit() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(!app.should_quit());
    }

    #[test]
    fn nav_is_visible_by_default() {
        let app = App::default();
        assert!(app.nav_visible());
    }

    #[test]
    fn toggling_nav_visibility_hides_and_restores_rail() {
        let mut app = App::default();
        app.toggle_nav_visibility();
        assert!(!app.nav_visible());

        app.toggle_nav_visibility();
        assert!(app.nav_visible());
    }

    #[test]
    fn defaults_settings_page_to_first_row() {
        let app = App::default();
        assert_eq!(app.settings_page().selected_index(), 0);
    }

    #[test]
    fn defaults_library_page_to_empty_state() {
        let app = App::default();
        assert_eq!(app.library_page().filtered_len(), 0);
        assert_eq!(
            app.library_page().empty_state_message(),
            Some("No triggers yet.")
        );
    }
}
