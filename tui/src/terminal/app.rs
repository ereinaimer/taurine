use crate::theme::Theme;
use crate::theme::builtin::DARK_THEME;
use crate::widgets::library::LibraryPageState;
use crate::widgets::settings::state::SettingsPageState;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct App {
    library_page: LibraryPageState,
    settings_page: SettingsPageState,
    settings_overlay_open: bool,
    should_quit: bool,
    notification: Option<String>,
    current_theme: &'static Theme,
}

impl Default for App {
    fn default() -> Self {
        Self {
            library_page: LibraryPageState::default(),
            settings_page: SettingsPageState::default(),
            settings_overlay_open: false,
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

    pub(crate) const fn library_page(&self) -> &LibraryPageState {
        &self.library_page
    }

    pub(crate) fn library_page_mut(&mut self) -> &mut LibraryPageState {
        &mut self.library_page
    }

    /// Settings lives as an overlay over the library now; the flag is
    /// the only page state left.
    pub(crate) const fn is_settings_overlay_open(&self) -> bool {
        self.settings_overlay_open
    }

    pub(crate) fn open_settings_overlay(&mut self) {
        self.settings_overlay_open = true;
    }

    pub(crate) fn close_settings_overlay(&mut self) {
        self.settings_overlay_open = false;
        self.settings_page.clear_modal();
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

    pub(crate) fn request_quit(&mut self) {
        self.should_quit = true;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_closed_settings_overlay() {
        let app = App::default();
        assert!(!app.is_settings_overlay_open());
    }

    #[test]
    fn overlay_open_close_round_trip() {
        let mut app = App::default();
        app.open_settings_overlay();
        assert!(app.is_settings_overlay_open());
        app.close_settings_overlay();
        assert!(!app.is_settings_overlay_open());
    }

    #[test]
    fn request_quit_marks_app_for_quit() {
        let mut app = App::default();
        app.request_quit();
        assert!(app.should_quit());
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
