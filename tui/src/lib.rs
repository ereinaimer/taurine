// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

mod overlay;
pub mod terminal;
mod theme;
pub use crate::widgets::library::actions::{LibraryImportConflictMode, RememberedConflictChoice};
pub use overlay::{
    ExportFormResult, ImportFormResult, prompt_password, run_conflict_prompt, run_export_overlay,
    run_import_overlay,
};
mod widgets;

use std::io;
use std::time::{Duration, Instant};

use crate::theme::Theme;
use crate::widgets::library;
use crate::widgets::settings;
use crossterm::{
    cursor::Show,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Margin},
    style::Style,
    widgets::Block,
};
use terminal::app::{App, Page};
use terminal::control::{
    DaemonController, SystemDaemonController, action_for_status, toggle_daemon,
    transition_status_for_action,
};
use terminal::event::{Event, EventHandler};
use tracing::error;
use widgets::{footer::FooterWidget, header::HeaderWidget, home, nav, notification};

const EVENT_TICK_RATE: Duration = Duration::from_millis(250);
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

pub fn run() -> taurine_core::Result<()> {
    let mut app = App::default();
    app.set_daemon_status(terminal::status::probe_daemon_status());
    refresh_home_stats(&mut app);
    refresh_library_page(&mut app);
    refresh_settings_page(&mut app);
    let daemon_controller = SystemDaemonController;

    let mut terminal = TerminalGuard::new()?;
    setup_signal_handler(|code| std::process::exit(code));
    let mut events = EventHandler::new(EVENT_TICK_RATE);
    let mut last_status_refresh = Instant::now();

    loop {
        terminal.terminal.draw(|frame| {
            let area = frame.area();
            let theme = app.theme();

            frame.render_widget(
                Block::default().style(Style::default().bg(theme.background)),
                area,
            );

            let inner = area.inner(Margin {
                vertical: 1,
                horizontal: 2,
            });

            let sections = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Min(0),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            frame.render_widget(
                HeaderWidget {
                    theme,
                    daemon_status: app.daemon_status(),
                },
                sections[0],
            );

            if app.nav_visible() {
                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([
                        Constraint::Length(22),
                        Constraint::Length(1),
                        Constraint::Min(0),
                    ])
                    .split(sections[2]);

                nav::render_navigation(frame, body[0], theme, app.active_page());
                render_page_content(frame, body[2], &app, theme);
            } else {
                render_page_content(frame, sections[2], &app, theme);
            }

            frame.render_widget(FooterWidget { theme, app: &app }, sections[4]);

            if let Some(msg) = app.notification() {
                notification::render_notification(frame, area, theme, msg);
            }
        })?;

        match events.next()? {
            Event::Key(key) => handle_tui_key_event(&mut app, key, &daemon_controller),
            Event::Tick => {
                if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
                    app.set_daemon_status(terminal::status::probe_daemon_status());
                    refresh_home_stats(&mut app);
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

fn render_page_content(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    app: &App,
    theme: &Theme,
) {
    use ratatui::{
        style::Modifier,
        symbols::border,
        text::Span,
        widgets::{Block, Borders},
    };
    let content_block = Block::default()
        .title(Span::styled(
            format!(" {} ", app.active_page().title()),
            ratatui::style::Style::default()
                .fg(theme.text)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_set(border::ROUNDED)
        .border_style(ratatui::style::Style::default().fg(theme.border));
    let inner = content_block.inner(area);
    frame.render_widget(content_block, area);

    match app.active_page() {
        Page::Home => {
            home::render_home_content(frame, inner, theme, app.home_stats());
        }
        Page::Library => {
            library::render_library_content(frame, inner, theme, app.library_page());
            if let Some(modal) = app.library_page().modal() {
                library::modals::render_library_modal(frame, area, theme, modal);
            }
        }
        Page::Settings => {
            settings::render_settings_content(frame, inner, theme, app.settings_page());
            if let Some(modal) = app.settings_page().modal() {
                settings::modals::render_settings_modal(frame, area, theme, modal);
            }
        }
    }
}

fn handle_tui_key_event<C: DaemonController>(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    daemon_controller: &C,
) {
    app.clear_notification();

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
        if let Some(library::LibraryModal::Import(state)) = app.library_page().modal()
            && let Some(err) = state.error()
        {
            app.set_notification(err.to_string());
        }
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
        let current_status = app.daemon_status();
        if current_status.is_transitioning() {
            return;
        }

        let action = action_for_status(current_status);
        app.set_daemon_status(transition_status_for_action(action));

        match toggle_daemon(daemon_controller, current_status) {
            Ok(outcome) => app.set_daemon_status(outcome.status),
            Err(err) => {
                app.set_daemon_status(terminal::status::probe_daemon_status());
                error!(error = %err, "Failed to toggle daemon lifecycle from the TUI");
            }
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

    if let Some(pending_import_prepare) = interaction.pending_import_prepare() {
        match pending_import_prepare.prepare() {
            Ok(library::LibraryImportPreparedResult::NeedsRunVariableConfirmation {
                prepared,
                return_to_modal,
            }) => {
                app.library_page_mut()
                    .open_import_run_variables_modal(prepared, *return_to_modal);
            }
            Ok(library::LibraryImportPreparedResult::Imported(outcome)) => {
                app.library_page_mut().clear_modal();
                if outcome.imported() > 0 {
                    taurine_core::rpc::notify_daemon_reload();
                }
                refresh_library_page(app);
                if outcome.imported_settings() {
                    refresh_settings_page(app);
                }
                if should_refresh_home_after_import(&outcome) {
                    refresh_home_stats(app);
                }
                app.library_page_mut().open_import_result_modal(&outcome);
            }
            Err(error) => {
                app.library_page_mut().set_save_error(error.to_string());
                app.set_notification(error.to_string());
            }
        }
        return;
    }

    if let Some(prepared_import) = interaction.pending_import_commit() {
        match prepared_import.apply() {
            Ok(outcome) => {
                app.library_page_mut().clear_modal();
                if outcome.imported() > 0 {
                    taurine_core::rpc::notify_daemon_reload();
                }
                refresh_library_page(app);
                if outcome.imported_settings() {
                    refresh_settings_page(app);
                }
                if should_refresh_home_after_import(&outcome) {
                    refresh_home_stats(app);
                }
                app.library_page_mut().open_import_result_modal(&outcome);
            }
            Err(error) => app.library_page_mut().set_save_error(error.to_string()),
        }
        return;
    }

    if let Some(pending_export) = interaction.pending_export() {
        match pending_export.apply() {
            Ok(path) => {
                app.library_page_mut().clear_modal();
                app.library_page_mut().open_export_result_modal(
                    &path,
                    pending_export.encrypt(),
                    pending_export.include_settings(),
                    pending_export.include_stats(),
                );
            }
            Err(error) => app.library_page_mut().set_save_error(error.to_string()),
        }
        return;
    }

    if let Some(pending_save) = interaction.pending_save() {
        let contains_clip = pending_save.content.contains("[clip");
        match pending_save.apply() {
            Ok(trigger_id) => {
                refresh_library_page(app);
                app.library_page_mut().select_item_by_id(&trigger_id);
                app.library_page_mut().clear_modal();
                if contains_clip && let Ok(conn) = taurine_core::db::get_conn() {
                    let settings = taurine_core::settings::SettingsManager::new(&conn).load_all();
                    if !settings.clipboard_history_enabled {
                        app.library_page_mut().set_status_message(
                            "Warning: '[clip]' system variable won't work because clipboard history is disabled.".to_string()
                        );
                    }
                }
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
        library::LibraryOpenRequest::Selected(id) => match load_library_trigger_detail(&id) {
            Ok(Some(trigger)) => app.library_page_mut().open_editor_modal(trigger),
            Ok(None) => error!(trigger_id = %id, "Selected library trigger no longer exists"),
            Err(error) => error!(
                trigger_id = %id,
                error = %error,
                "Failed to load TUI library trigger detail"
            ),
        },
        library::LibraryOpenRequest::Create => {
            app.library_page_mut().open_create_modal();
        }
    }
}

fn should_refresh_home_after_import(outcome: &library::LibraryImportOutcome) -> bool {
    outcome.imported_settings() || outcome.imported_stats()
}

fn refresh_home_stats(app: &mut App) {
    match taurine_core::db::init::setup()
        .and_then(|conn| taurine_core::stats::load_home_stats(&conn))
    {
        Ok(home_stats) => app.set_home_stats(home_stats),
        Err(err) => error!(error = %err, "Failed to refresh TUI home stats"),
    }
}

fn refresh_library_page(app: &mut App) {
    match taurine_core::db::init::setup()
        .and_then(|conn| taurine_core::db::crud::get_triggers_list(&conn).map_err(Into::into))
    {
        Ok(items) => {
            let items = items
                .into_iter()
                .map(library::LibraryTrigger::from)
                .collect();
            app.library_page_mut().replace_items(items);
        }
        Err(error) => {
            error!(error = %error, "Failed to refresh TUI library state");
            app.library_page_mut().set_load_error(error.to_string());
        }
    }
}

fn load_library_trigger_detail(
    id: &str,
) -> taurine_core::Result<Option<library::LibraryTriggerDetail>> {
    let conn = taurine_core::db::init::setup()?;
    let Some(trigger) = taurine_core::db::crud::get_trigger(&conn, id)? else {
        return Ok(None);
    };

    if trigger.is_deleted || !trigger.is_enabled {
        return Ok(None);
    }

    library::LibraryTriggerDetail::from_row(trigger).map(Some)
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

#[cfg(all(test, unix))]
static REGISTRATION_TX: std::sync::Mutex<Option<std::sync::mpsc::Sender<()>>> =
    std::sync::Mutex::new(None);

// Setup OS signal handling for the TUI.
//
// NOTE: Since the TUI runs in crossterm raw mode, Ctrl+C is intercepted by
// crossterm as a key event (which the application handles/ignores), rather than
// raising a SIGINT signal. Therefore, this handler is primarily active for
// external termination signals (such as SIGTERM/SIGINT from process supervisors).
fn setup_signal_handler<F>(exit_fn: F)
where
    F: FnOnce(i32) + Send + 'static,
{
    if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        std::thread::spawn(move || {
            rt.block_on(async {
                #[cfg(unix)]
                {
                    let mut sigterm = match tokio::signal::unix::signal(
                        tokio::signal::unix::SignalKind::terminate(),
                    ) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            error!("Failed to register SIGTERM handler: {}", e);
                            None
                        }
                    };
                    let mut sigint = match tokio::signal::unix::signal(
                        tokio::signal::unix::SignalKind::interrupt(),
                    ) {
                        Ok(s) => Some(s),
                        Err(e) => {
                            error!("Failed to register SIGINT handler: {}", e);
                            None
                        }
                    };

                    #[cfg(test)]
                    if let Ok(mut lock) = REGISTRATION_TX.lock()
                        && let Some(tx) = lock.take()
                    {
                        let _ = tx.send(());
                    }

                    tokio::select! {
                        _ = async {
                            if let Some(ref mut sig) = sigterm {
                                sig.recv().await;
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {
                            restore_terminal();
                            exit_fn(143);
                        }
                        _ = async {
                            if let Some(ref mut sig) = sigint {
                                sig.recv().await;
                            } else {
                                std::future::pending::<()>().await;
                            }
                        } => {
                            restore_terminal();
                            exit_fn(130);
                        }
                    }
                }
                #[cfg(windows)]
                {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        restore_terminal();
                        exit_fn(0);
                    }
                }
            });
        });
    }
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, Show);
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    #[cfg(unix)]
    #[tokio::test]
    async fn test_signal_handler_restores_terminal() {
        let called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called_clone = called.clone();

        let exit_fn = move |code: i32| {
            assert_eq!(code, 143);
            called_clone.store(true, std::sync::atomic::Ordering::SeqCst);
        };

        let (tx, rx) = std::sync::mpsc::channel();
        if let Ok(mut lock) = REGISTRATION_TX.lock() {
            *lock = Some(tx);
        }

        super::setup_signal_handler(exit_fn);

        // Wait deterministically for tokio to complete registration of the signal handler.
        // Once rx.recv() returns, the OS hook is guaranteed to be installed.
        let _ = rx.recv();

        // Trigger SIGTERM
        // SAFETY: Raising SIGTERM on the current process is safe because we have verified via the
        // mpsc channel that the tokio signal handler is fully registered with the OS.
        unsafe {
            libc::raise(libc::SIGTERM);
        }

        // Wait and check if called is true
        for _ in 0..20 {
            if called.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
    }

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use taurine_core::{
        db::crud::{TriggerListItem, TriggerRow, TriggerType},
        engine::shell::{ScriptBehavior, ScriptInterpreter, compress},
    };

    use super::*;
    use crate::widgets::library::LibraryTrigger;

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

    fn sample_library_modal() -> library::LibraryTriggerDetail {
        library::LibraryTriggerDetail::from_row(TriggerRow {
            id: "library-modal".to_string(),
            name: "Library Modal".to_string(),
            description: Some("Open Reddit".to_string()),
            trigger_type: TriggerType::Hotkey,
            trigger: "alt+r".to_string(),
            output: "[Script: powershell]".to_string(),
            action_type: "script".to_string(),
            target_os: "win".to_string(),
            only_apps: None,
            except_apps: None,
            tags: "[]".to_string(),
            usage_count: 6,
            last_used_at: Some(1),
            created_at: 1,
            updated_at: 1,
            version: 1,
            is_deleted: false,
            is_synced: true,
            is_enabled: true,
            auto_case: false,
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Silent),
            script_binary: Some(compress("Start-Process https://reddit.com").unwrap()),
        })
        .unwrap()
    }

    fn sample_import_outcome(
        imported: usize,
        imported_settings: bool,
        imported_stats: bool,
    ) -> library::LibraryImportOutcome {
        library::LibraryImportOutcome::new(imported, imported_settings, imported_stats)
    }

    #[test]
    fn pressing_x_on_home_calls_start_when_stopped() {
        let mut app = App::default();
        app.set_daemon_status(terminal::status::DaemonStatus::Stopped);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 1);
        assert_eq!(controller.stop_calls.get(), 0);
    }

    #[test]
    fn pressing_x_on_home_calls_stop_when_running() {
        let mut app = App::default();
        app.set_daemon_status(terminal::status::DaemonStatus::Running);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 1);
    }

    #[test]
    fn pressing_x_on_home_calls_stop_when_paused() {
        let mut app = App::default();
        app.set_daemon_status(terminal::status::DaemonStatus::Paused);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 1);
    }

    #[test]
    fn pressing_x_on_home_ignores_duplicate_requests_while_starting() {
        let mut app = App::default();
        app.set_daemon_status(terminal::status::DaemonStatus::Starting);
        let controller = MockController::default();

        handle_tui_key_event(&mut app, plain_key('x'), &controller);

        assert_eq!(controller.start_calls.get(), 0);
        assert_eq!(controller.stop_calls.get(), 0);
        assert_eq!(
            app.daemon_status(),
            terminal::status::DaemonStatus::Starting
        );
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
    fn home_stats_refreshes_after_import_when_settings_are_imported() {
        let outcome = sample_import_outcome(0, true, false);

        assert!(should_refresh_home_after_import(&outcome));
    }

    #[test]
    fn home_stats_refreshes_after_import_when_stats_are_imported() {
        let outcome = sample_import_outcome(0, false, true);

        assert!(should_refresh_home_after_import(&outcome));
    }

    #[test]
    fn home_stats_does_not_refresh_when_neither_settings_nor_stats_are_imported() {
        let outcome = sample_import_outcome(0, false, false);

        assert!(!should_refresh_home_after_import(&outcome));
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
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::from(TriggerListItem {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                trigger_type: TriggerType::Hotkey,
                trigger: "alt+t".to_string(),
                output: "test".to_string(),
                action_type: "text".to_string(),
                target_os: "win".to_string(),
                only_apps: None,
                except_apps: None,
                usage_count: 0,
                last_used_at: None,
                created_at: 0,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            })]);

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
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::from(TriggerListItem {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                trigger_type: TriggerType::Hotkey,
                trigger: "alt+t".to_string(),
                output: "test".to_string(),
                action_type: "text".to_string(),
                target_os: "win".to_string(),
                only_apps: None,
                except_apps: None,
                usage_count: 0,
                last_used_at: None,
                created_at: 0,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            })]);

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
