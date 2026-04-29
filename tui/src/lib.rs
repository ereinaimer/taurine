// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

mod app;
mod control;
mod event;
mod library;
mod settings;
mod status;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use app::{App, Page};
use control::{DaemonController, SystemDaemonController, toggle_daemon};
use crossterm::{
    cursor::Show,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use event::{Event, EventHandler};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing::error;

const EVENT_TICK_RATE: Duration = Duration::from_millis(250);
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub fn run() -> taurine_core::Result<()> {
    let mut app = App::default();
    app.set_daemon_status(status::probe_daemon_status());
    refresh_home_metrics(&mut app);
    refresh_library_page(&mut app);
    refresh_settings_page(&mut app);
    let daemon_controller = SystemDaemonController;

    let mut terminal = TerminalGuard::new()?;
    let mut events = EventHandler::new(EVENT_TICK_RATE);
    let mut last_status_refresh = Instant::now();

    loop {
        terminal.terminal.draw(|frame| ui::render(frame, &app))?;

        match events.next()? {
            Event::Key(key) => handle_tui_key_event(&mut app, key, &daemon_controller),
            Event::Tick => {
                if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
                    app.set_daemon_status(status::probe_daemon_status());
                    refresh_home_metrics(&mut app);
                    last_status_refresh = Instant::now();
                }
            }
        }

        if app.should_quit() {
            break;
        }
    }

    Ok(())
}

fn handle_tui_key_event<C: DaemonController>(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    daemon_controller: &C,
) {
    if matches!(key.code, crossterm::event::KeyCode::Char('b' | 'B'))
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
    {
        app.toggle_nav_visibility();
        return;
    }

    if app.active_page() == Page::Settings && app.settings_page().is_modal_open() {
        let interaction = app.settings_page_mut().handle_key(key);
        apply_settings_interaction(app, interaction);
        return;
    }

    if app.active_page() == Page::Library
        && (app.library_page().is_modal_open() || app.library_page().is_search_active())
    {
        let interaction = app.library_page_mut().handle_key(key);
        apply_library_interaction(app, interaction);
        return;
    }

    app.handle_key_event(key);

    if app.active_page() == Page::Library {
        let interaction = app.library_page_mut().handle_key(key);
        apply_library_interaction(app, interaction);
        return;
    }

    if app.active_page() == Page::Settings {
        let interaction = app.settings_page_mut().handle_key(key);
        apply_settings_interaction(app, interaction);
        return;
    }

    if key.code == crossterm::event::KeyCode::Char('x')
        && key.modifiers == crossterm::event::KeyModifiers::NONE
        && app.active_page() == Page::Home
    {
        match toggle_daemon(daemon_controller, app.daemon_status()) {
            Ok(outcome) => app.set_daemon_status(outcome.status),
            Err(err) => error!(error = %err, "Failed to toggle daemon lifecycle from the TUI"),
        }
    }
}

fn apply_settings_interaction(app: &mut App, interaction: settings::SettingsInteraction) {
    if interaction.should_close_modal() {
        app.settings_page_mut().clear_modal();
        return;
    }

    if let Some(pending_reset) = interaction.pending_reset() {
        match pending_reset.apply() {
            Ok(()) => refresh_settings_page(app),
            Err(error) => app.settings_page_mut().set_save_error(error.to_string()),
        }
        return;
    }

    let Some(pending_save) = interaction.pending_save() else {
        return;
    };

    match pending_save.apply() {
        Ok(()) => refresh_settings_page(app),
        Err(error) => app.settings_page_mut().set_save_error(error.to_string()),
    }
}

fn apply_library_interaction(app: &mut App, interaction: library::LibraryInteraction) {
    if interaction.should_close_modal() {
        app.library_page_mut().clear_modal();
        return;
    }

    if let Some(pending_save) = interaction.pending_save() {
        match pending_save.apply() {
            Ok(automation_id) => {
                refresh_library_page(app);
                app.library_page_mut().select_item_by_id(&automation_id);
                app.library_page_mut().clear_modal();
            }
            Err(error) => app.library_page_mut().set_save_error(error.to_string()),
        }
        return;
    }

    if let Some(pending_delete) = interaction.pending_delete() {
        let restore_index = pending_delete.restore_index();
        match pending_delete.apply() {
            Ok(()) => {
                refresh_library_page(app);
                app.library_page_mut().select_after_delete(restore_index);
                app.library_page_mut().clear_modal();
            }
            Err(error) => app.library_page_mut().set_save_error(error.to_string()),
        }
        return;
    }

    let Some(open_request) = interaction.into_open_request() else {
        return;
    };

    match open_request {
        library::LibraryOpenRequest::Selected(id) => match load_library_automation_detail(&id) {
            Ok(Some(automation)) => app.library_page_mut().open_editor_modal(automation),
            Ok(None) => error!(automation_id = %id, "Selected library automation no longer exists"),
            Err(error) => error!(
                automation_id = %id,
                error = %error,
                "Failed to load TUI library automation detail"
            ),
        },
        library::LibraryOpenRequest::Create => {
            app.library_page_mut().open_create_modal();
        }
    }
}

fn refresh_home_metrics(app: &mut App) {
    match taurine_core::db::init::setup()
        .and_then(|conn| taurine_core::metrics::load_home_metrics(&conn))
    {
        Ok(home_metrics) => app.set_home_metrics(home_metrics),
        Err(err) => error!(error = %err, "Failed to refresh TUI home metrics"),
    }
}

fn refresh_library_page(app: &mut App) {
    match taurine_core::db::init::setup()
        .and_then(|conn| taurine_core::db::crud::get_automations_list(&conn).map_err(Into::into))
    {
        Ok(items) => {
            let items = items
                .into_iter()
                .map(library::LibraryAutomation::from)
                .collect();
            app.library_page_mut().replace_items(items);
        }
        Err(error) => {
            error!(error = %error, "Failed to refresh TUI library state");
            app.library_page_mut().set_load_error(error.to_string());
        }
    }
}

fn load_library_automation_detail(
    id: &str,
) -> taurine_core::Result<Option<library::LibraryAutomationDetail>> {
    let conn = taurine_core::db::init::setup()?;
    let Some(automation) = taurine_core::db::crud::get_automation(&conn, id)? else {
        return Ok(None);
    };

    library::LibraryAutomationDetail::from_row(automation).map(Some)
}

fn refresh_settings_page(app: &mut App) {
    match taurine_core::db::init::setup() {
        Ok(conn) => {
            let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
            app.settings_page_mut().replace_settings(settings);
        }
        Err(error) => {
            error!(error = %error, "Failed to refresh TUI settings state");
            app.settings_page_mut().set_load_error(error.to_string());
        }
    }
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;

        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen) {
            restore_terminal();
            return Err(error);
        }

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = match Terminal::new(backend) {
            Ok(terminal) => terminal,
            Err(error) => {
                restore_terminal();
                return Err(error);
            }
        };

        if let Err(error) = terminal.hide_cursor() {
            restore_terminal();
            return Err(error);
        }

        if let Err(error) = terminal.clear() {
            restore_terminal();
            return Err(error);
        }

        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
        let _ = execute!(self.terminal.backend_mut(), Show);
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use taurine_core::{
        db::crud::{AutomationRow, TriggerType},
        engine::shell::{ScriptBehavior, ScriptInterpreter, compress},
    };

    use super::*;

    #[derive(Default)]
    struct MockController {
        start_calls: Cell<usize>,
        stop_calls: Cell<usize>,
    }

    impl DaemonController for MockController {
        fn start(&self) -> taurine_core::Result<()> {
            self.start_calls.set(self.start_calls.get() + 1);
            Ok(())
        }

        fn stop(&self) -> taurine_core::Result<()> {
            self.stop_calls.set(self.stop_calls.get() + 1);
            Ok(())
        }
    }

    fn plain_key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    fn sample_library_modal() -> library::LibraryAutomationDetail {
        library::LibraryAutomationDetail::from_row(AutomationRow {
            id: "library-modal".to_string(),
            name: "Library Modal".to_string(),
            description: Some("Open Reddit".to_string()),
            trigger_type: TriggerType::Hotkey,
            trigger: "alt+r".to_string(),
            output: "[Script: powershell]".to_string(),
            action_type: "script".to_string(),
            target_os: "win".to_string(),
            tags: "[]".to_string(),
            usage_count: 6,
            last_used_at: Some(1),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_synced: true,
            is_enabled: true,
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compress("Start-Process https://reddit.com").unwrap()),
        })
        .unwrap()
    }

    #[test]
    fn pressing_x_on_home_calls_start_when_stopped() {
        let mut app = App::default();
        app.set_daemon_status(status::DaemonStatus::Stopped);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 1);
        assert_eq!(controller.stop_calls.get(), 0);
    }

    #[test]
    fn pressing_x_on_home_calls_stop_when_running() {
        let mut app = App::default();
        app.set_daemon_status(status::DaemonStatus::Running);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 1);
    }

    #[test]
    fn pressing_x_on_home_calls_stop_when_paused() {
        let mut app = App::default();
        app.set_daemon_status(status::DaemonStatus::Paused);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 1);
    }

    #[test]
    fn pressing_x_on_library_does_not_call_lifecycle() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 0);
    }

    #[test]
    fn pressing_x_on_settings_does_not_call_lifecycle() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('3'), KeyModifiers::NONE);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 0);
    }

    #[test]
    fn pressing_ctrl_b_toggles_navigation_visibility() {
        let mut app = App::default();
        let controller = MockController::default();

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            &controller,
        );
        assert!(!app.nav_visible());

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            &controller,
        );
        assert!(app.nav_visible());
    }

    #[test]
    fn pressing_ctrl_b_does_not_change_active_page() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('3'), KeyModifiers::NONE);
        let controller = MockController::default();

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            &controller,
        );

        assert_eq!(app.active_page(), Page::Settings);
    }

    #[test]
    fn typing_q_while_library_search_is_active_does_not_quit() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, plain_key('/'), &controller);
        handle_tui_key_event(&mut app, plain_key('q'), &controller);

        assert!(!app.should_quit());
        assert_eq!(app.library_page().search_query(), "q");
    }

    #[test]
    fn typing_one_while_library_search_is_active_does_not_change_page() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, plain_key('/'), &controller);
        handle_tui_key_event(&mut app, plain_key('1'), &controller);

        assert_eq!(app.active_page(), Page::Library);
        assert_eq!(app.library_page().search_query(), "1");
    }

    #[test]
    fn typing_q_while_library_modal_is_open_does_not_quit() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        app.library_page_mut()
            .open_editor_modal(sample_library_modal());

        handle_tui_key_event(&mut app, plain_key('q'), &controller);

        assert!(!app.should_quit());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn slash_does_not_activate_library_search_while_modal_is_open() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        app.library_page_mut()
            .open_editor_modal(sample_library_modal());

        handle_tui_key_event(&mut app, plain_key('/'), &controller);

        assert!(!app.library_page().is_search_active());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn typing_q_while_library_delete_confirmation_is_open_does_not_quit() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, plain_key('d'), &controller);
        handle_tui_key_event(&mut app, plain_key('q'), &controller);

        assert!(!app.should_quit());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn slash_does_not_activate_search_while_library_delete_confirmation_is_open() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, plain_key('d'), &controller);
        handle_tui_key_event(&mut app, plain_key('/'), &controller);

        assert!(!app.library_page().is_search_active());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn escape_closes_library_modal_without_changing_page() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        app.library_page_mut()
            .open_editor_modal(sample_library_modal());

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            &controller,
        );

        assert_eq!(app.active_page(), Page::Library);
        assert!(!app.library_page().is_modal_open());
    }

    #[test]
    fn pressing_n_on_library_opens_create_modal_without_changing_page() {
        let mut app = App::default();
        let controller = MockController::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, plain_key('n'), &controller);

        assert_eq!(app.active_page(), Page::Library);
        assert!(app.library_page().is_modal_open());
    }
}
