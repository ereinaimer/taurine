use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::status::DaemonStatus;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Page {
    #[default]
    Home,
    Library,
    Settings,
}

impl Page {
    pub(crate) const ALL: [Self; 3] = [Self::Home, Self::Library, Self::Settings];

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Library => "Library",
            Self::Settings => "Settings",
        }
    }

    pub(crate) const fn nav_index(self) -> usize {
        match self {
            Self::Home => 0,
            Self::Library => 1,
            Self::Settings => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct App {
    active_page: Page,
    daemon_status: DaemonStatus,
    should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            active_page: Page::Home,
            daemon_status: DaemonStatus::Stopped,
            should_quit: false,
        }
    }
}

impl App {
    pub(crate) const fn active_page(&self) -> Page {
        self.active_page
    }

    pub(crate) const fn daemon_status(&self) -> DaemonStatus {
        self.daemon_status
    }

    pub(crate) const fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub(crate) fn set_daemon_status(&mut self, daemon_status: DaemonStatus) {
        self.daemon_status = daemon_status;
    }

    pub(crate) fn handle_key_event(&mut self, key: KeyEvent) {
        self.handle_key(key.code, key.modifiers);
    }

    pub(crate) fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match (code, modifiers) {
            (KeyCode::Char('1'), _) => self.active_page = Page::Home,
            (KeyCode::Char('2'), _) => self.active_page = Page::Library,
            (KeyCode::Char('3'), _) => self.active_page = Page::Settings,
            (KeyCode::Char('q'), KeyModifiers::NONE) => self.should_quit = true,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_home_page() {
        let app = App::default();
        assert_eq!(app.active_page(), Page::Home);
    }

    #[test]
    fn pressing_one_selects_home() {
        let mut app = App {
            active_page: Page::Settings,
            ..App::default()
        };
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        assert_eq!(app.active_page(), Page::Home);
    }

    #[test]
    fn pressing_two_selects_library() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(app.active_page(), Page::Library);
    }

    #[test]
    fn pressing_three_selects_settings() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('3'), KeyModifiers::NONE);
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
}
