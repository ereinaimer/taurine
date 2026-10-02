// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

mod overlay;
pub mod terminal;
mod theme;
pub use crate::widgets::library::actions::{LibraryImportConflictMode, RememberedConflictChoice};
pub use overlay::{
    ExportFormResult, ImportFormResult, run_ai_overlay, run_conflict_prompt, run_export_overlay,
    run_import_overlay,
};
mod widgets;

use std::io;
use std::time::Duration;

use crate::theme::Theme;
use crate::widgets::library;
use crate::widgets::settings;
use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, style::Style, widgets::Block};
use terminal::app::{App, Page};
use terminal::event::{Event, EventHandler};
use tracing::error;
use widgets::notification;

const EVENT_TICK_RATE: Duration = Duration::from_millis(250);

pub fn run() -> taurine_core::Result<()> {
    let mut app = App::default();
    refresh_library_page(&mut app);
    refresh_settings_page(&mut app);
    // honey: bottom-anchored lists open on the row touching the search bar.
    app.library_page_mut().select_last();
    app.settings_page_mut().select_last();

    let mut terminal = TerminalGuard::new()?;
    setup_signal_handler(|code| std::process::exit(code));
    let mut events = EventHandler::new(EVENT_TICK_RATE);
    let mut last_area = ratatui::layout::Rect::default();

    loop {
        terminal.terminal.draw(|frame| {
            let area = frame.area();
            last_area = area;
            let theme = app.theme();

            frame.render_widget(
                Block::default().style(Style::default().bg(theme.background)),
                area,
            );

            let layout = terminal::mouse::frame_layout(area);

            render_page_content(frame, layout.page, &app, theme);

            if app.active_page() == Page::Library
                && let Some(column) =
                    library::divider_column(layout.page, app.library_page().split_ratio())
                && area.height > 0
            {
                use ratatui::text::Line;

                // honey: divider lifts one subtle step on hover and while dragging.
                let color =
                    if app.library_page().divider_drag() || app.library_page().divider_hover() {
                        library::DIVIDER_HOVER_COLOR
                    } else {
                        theme.border
                    };
                let glyphs = vec![Line::from("│"); area.height as usize];
                frame.render_widget(
                    ratatui::widgets::Paragraph::new(glyphs)
                        .style(ratatui::style::Style::default().fg(color)),
                    ratatui::layout::Rect {
                        x: column,
                        y: area.y,
                        width: 1,
                        height: area.height,
                    },
                );
            }

            if let Some(msg) = app.notification() {
                notification::render_notification(frame, area, theme, msg);
            }
        })?;

        match events.next()? {
            Event::Key(key) => handle_tui_key_event(&mut app, key),
            Event::Mouse(mouse) => handle_tui_mouse_event(&mut app, mouse, last_area),
            Event::Tick => {}
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
        symbols::border,
        widgets::{Block, Borders},
    };
    match app.active_page() {
        Page::Library => {
            library::render_library_content(frame, area, theme, app.library_page());
            if let Some(modal) = app.library_page().modal() {
                library::modals::render_library_modal(frame, area, theme, modal);
            }
        }
        Page::Settings => {
            let content_block = Block::default()
                .borders(Borders::ALL)
                .border_set(border::ROUNDED)
                .border_style(ratatui::style::Style::default().fg(theme.border));
            let inner = content_block.inner(area);
            frame.render_widget(content_block, area);

            settings::render_settings_content(frame, inner, theme, app.settings_page());
            if let Some(modal) = app.settings_page().modal() {
                settings::modals::render_settings_modal(frame, area, theme, modal);
            }
        }
    }
}

fn handle_tui_key_event(app: &mut App, key: crossterm::event::KeyEvent) {
    app.clear_notification();

    // honey: Ctrl+C quits cleanly from anywhere, including modals and
    // search, since raw mode delivers it as a key event, not a signal.
    if key.code == crossterm::event::KeyCode::Char('c')
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
    {
        app.request_quit();
        return;
    }

    if app.active_page() == Page::Settings
        && (app.settings_page().is_modal_open() || app.settings_page().is_search_active())
    {
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

    let previous_page = app.active_page();
    app.handle_key_event(key);
    if app.active_page() != previous_page {
        // honey: the navigation key itself must not leak into the fresh page.
        return;
    }

    if app.active_page() == Page::Library {
        let interaction = app.library_page_mut().handle_key(key);
        apply_library_interaction(app, interaction);
        return;
    }

    if app.active_page() == Page::Settings {
        let interaction = app.settings_page_mut().handle_key(key);
        apply_settings_interaction(app, interaction);
    }
}

fn handle_tui_mouse_event(
    app: &mut App,
    mouse: crossterm::event::MouseEvent,
    area: ratatui::layout::Rect,
) {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};

    fn scroll_key(down: bool) -> KeyEvent {
        KeyEvent::new(
            if down { KeyCode::Down } else { KeyCode::Up },
            KeyModifiers::NONE,
        )
    }

    app.clear_notification();

    let modal_open = match app.active_page() {
        Page::Library => app.library_page().is_modal_open(),
        Page::Settings => app.settings_page().is_modal_open(),
    };

    // honey: grabbing the divider starts a drag; pointer motion after that
    // moves the split freely without re-hitting the gutter column.
    fn grab_divider(app: &mut App, page: ratatui::layout::Rect, column: u16, row: u16) -> bool {
        let ratio = app.library_page().split_ratio();
        if !library::divider_hit(page, ratio, column, row) {
            return false;
        }
        app.library_page_mut().set_divider_drag(true);
        true
    }

    fn drag_divider_to(app: &mut App, page: ratatui::layout::Rect, column: u16) {
        let ratio = library::split_ratio_for_column(page, column);
        app.library_page_mut().set_split_ratio(ratio);
    }

    match mouse.kind {
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            if modal_open {
                return;
            }
            if app.active_page() == Page::Library {
                let layout = terminal::mouse::frame_layout(area);
                let page = app.library_page();
                if library::detail::detail_contains(
                    layout.page,
                    page.split_ratio(),
                    mouse.column,
                    mouse.row,
                ) {
                    let down = mouse.kind == MouseEventKind::ScrollDown;
                    let content = library::detail::right_content(layout.page, page.split_ratio());
                    let item = match page
                        .selected_index()
                        .and_then(|index| page.item_at_filtered(index))
                    {
                        Some(item) => item.clone(),
                        None => return,
                    };
                    let max =
                        library::detail::content_scroll_max(content.height, content.width, &item);
                    app.library_page_mut()
                        .scroll_detail(if down { 1 } else { -1 }, max);
                    return;
                }
            }
            handle_tui_key_event(app, scroll_key(mouse.kind == MouseEventKind::ScrollDown));
        }
        MouseEventKind::Moved => {
            if modal_open || app.active_page() != Page::Library {
                return;
            }
            let layout = terminal::mouse::frame_layout(area);
            let page = app.library_page();
            let hover =
                library::divider_hit(layout.page, page.split_ratio(), mouse.column, mouse.row);
            if page.divider_drag() {
                drag_divider_to(app, layout.page, mouse.column);
            } else {
                app.library_page_mut().set_divider_hover(hover);
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if modal_open || app.active_page() != Page::Library {
                return;
            }
            if app.library_page().divider_drag() {
                let layout = terminal::mouse::frame_layout(area);
                drag_divider_to(app, layout.page, mouse.column);
            }
        }
        MouseEventKind::Up(_) => {
            if app.active_page() == Page::Library {
                app.library_page_mut().set_divider_drag(false);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if modal_open {
                // honey: info popup dismisses on outside click; other
                // modals keep ignoring background clicks.
                if app.active_page() == Page::Library
                    && let Some(library::LibraryModal::Info(_)) = app.library_page().modal()
                {
                    let layout = terminal::mouse::frame_layout(area);
                    let popup = library::modals::info_popup_rect(layout.page);
                    if !terminal::mouse::contains(popup, mouse.column, mouse.row) {
                        app.library_page_mut().clear_modal();
                    }
                }
                return;
            }
            let layout = terminal::mouse::frame_layout(area);
            if app.active_page() == Page::Library
                && grab_divider(app, layout.page, mouse.column, mouse.row)
            {
                return;
            }
            match app.active_page() {
                Page::Library => {
                    match library::list::hit_test(
                        layout.page,
                        app.library_page(),
                        mouse.column,
                        mouse.row,
                    ) {
                        Some(library::list::LibraryHit::Item(position)) => {
                            let anchor =
                                library::list::window_start(layout.page, app.library_page());
                            let interaction = app.library_page_mut().click_item(position, anchor);
                            apply_library_interaction(app, interaction);
                        }
                        Some(library::list::LibraryHit::Search) => {
                            app.library_page_mut().activate_search();
                        }
                        // honey: header enable toggle and info button are
                        // clickable; the rest of the preview is read-only.
                        None => {
                            let page = app.library_page();
                            let hit = library::detail::hit_test(
                                layout.page,
                                page.split_ratio(),
                                page,
                                mouse.column,
                                mouse.row,
                            );
                            match hit {
                                Some(library::detail::DetailHit::InfoOpen) => {
                                    app.library_page_mut().open_info_modal_for_selected();
                                }
                                Some(library::detail::DetailHit::EnableToggle) => {
                                    let interaction =
                                        app.library_page_mut().toggle_selected_enabled();
                                    apply_library_interaction(app, interaction);
                                }
                                None => {}
                            }
                        }
                    }
                }
                Page::Settings => {
                    let inner = terminal::mouse::page_inner(layout.page);
                    match settings::hit_test(inner, app.settings_page(), mouse.column, mouse.row) {
                        Some(settings::SettingsHit::Row(key)) => {
                            let anchor = settings::visible_window_start(inner, app.settings_page());
                            let interaction = app.settings_page_mut().click_setting(key, anchor);
                            apply_settings_interaction(app, interaction);
                        }
                        Some(settings::SettingsHit::Search) => {
                            app.settings_page_mut().activate_search();
                        }
                        None => {}
                    }
                }
            }
        }
        _ => {}
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
        Ok(()) => {
            refresh_settings_page(app);
            apply_saved_cursor_style();
        }
        Err(error) => app.settings_page_mut().set_save_error(error.to_string()),
    }
}

/// Best-effort caret styling from the TUI-only cursor style setting.
pub(crate) fn apply_saved_cursor_style() {
    let Ok(conn) = taurine_core::db::init::setup() else {
        return;
    };
    let style = taurine_core::settings::SettingsManager::new(&conn)
        .load_all()
        .tui_cursor_style;
    let _ = execute!(
        io::stdout(),
        crate::widgets::settings::cursor_set_cursor_style(style)
    );
}

pub(crate) fn reset_cursor_style() {
    use crossterm::cursor::SetCursorStyle;
    let _ = execute!(io::stdout(), SetCursorStyle::DefaultUserShape);
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
                app.library_page_mut().open_export_result_modal(&path);
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
    }

    if let Some(pending_toggle) = interaction.pending_toggle() {
        let restore_index = pending_toggle.restore_index;
        match pending_toggle.apply() {
            Ok(()) => {
                refresh_library_page(app);
                app.library_page_mut().select_after_delete(restore_index);
            }
            Err(error) => app.library_page_mut().set_save_error(error.to_string()),
        }
    }
}

fn refresh_library_page(app: &mut App) {
    match taurine_core::db::init::setup()
        .and_then(|conn| taurine_core::db::crud::get_library_triggers(&conn).map_err(Into::into))
    {
        Ok(items) => {
            let items = items
                .into_iter()
                .flat_map(library::LibraryTrigger::expand)
                .collect();
            app.library_page_mut().replace_items(items);
        }
        Err(error) => {
            error!(error = %error, "Failed to refresh TUI library state");
            app.library_page_mut().set_load_error(error.to_string());
        }
    }
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
        if let Err(error) = execute!(stdout, EnableMouseCapture) {
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

        apply_saved_cursor_style();

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
    reset_cursor_style();
    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        Show
    );
}

#[cfg(test)]
mod tests {
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

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use taurine_core::db::crud::{InvocationType, TriggerAliasRow, TriggerListItem};

    use super::*;
    use crate::widgets::library::LibraryTrigger;

    fn plain_key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE)
    }

    fn hotkey_alias(id: &str, invocation: &str) -> TriggerAliasRow {
        TriggerAliasRow {
            id: format!("alias-{invocation}"),
            trigger_id: id.to_string(),
            invocation: invocation.to_string(),
            invocation_type: InvocationType::Hotkey,
            require_confirmation: false,
            strict_threshold: None,
            created_at: 0,
        }
    }

    fn seed_single_library_item(app: &mut App) {
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::single(TriggerListItem {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                invocations: vec![hotkey_alias("test", "alt+t")],
                display: "alt+t".to_string(),
                output: "test".to_string(),
                action_type: "text".to_string(),
                target_os: "win".to_string(),
                only_apps: None,
                except_apps: None,
                auto_case: false,
                is_enabled: true,
                usage_count: 0,
                last_used_at: None,
                created_at: 0,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            })]);
    }

    #[test]
    fn typing_q_while_library_search_is_active_does_not_quit() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, plain_key('/'));
        handle_tui_key_event(&mut app, plain_key('q'));

        assert!(!app.should_quit());
        assert_eq!(app.library_page().search_query(), "q");
    }

    #[test]
    fn navigating_to_library_leaves_search_inactive() {
        let mut app = App::default();
        handle_tui_key_event(&mut app, plain_key('1'));

        assert_eq!(app.active_page(), Page::Library);
        assert!(!app.library_page().is_search_active());
        assert_eq!(app.library_page().search_query(), "");
    }

    #[test]
    fn navigating_to_settings_leaves_search_inactive() {
        let mut app = App::default();
        handle_tui_key_event(&mut app, plain_key('2'));

        assert_eq!(app.active_page(), Page::Settings);
        assert!(!app.settings_page().is_search_active());
        assert_eq!(app.settings_page().search_query(), "");
    }

    #[test]
    fn typing_one_while_library_search_is_active_does_not_change_page() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, plain_key('/'));
        handle_tui_key_event(&mut app, plain_key('1'));

        assert_eq!(app.active_page(), Page::Library);
        assert_eq!(app.library_page().search_query(), "1");
    }

    #[test]
    fn typing_q_while_library_modal_is_open_does_not_quit() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, plain_key('q'));

        assert!(!app.should_quit());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn slash_goes_to_modal_while_library_modal_is_open() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, plain_key('/'));

        assert!(app.library_page().is_modal_open());
        assert_eq!(app.library_page().search_query(), "");
    }

    #[test]
    fn typing_q_while_library_delete_confirmation_is_open_does_not_quit() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::single(TriggerListItem {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                invocations: vec![hotkey_alias("test", "alt+t")],
                display: "alt+t".to_string(),
                output: "test".to_string(),
                action_type: "text".to_string(),
                target_os: "win".to_string(),
                only_apps: None,
                except_apps: None,
                auto_case: false,
                is_enabled: true,
                usage_count: 0,
                last_used_at: None,
                created_at: 0,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            })]);

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.library_page_mut().open_delete_modal_for_selected();
        handle_tui_key_event(&mut app, plain_key('q'));

        assert!(!app.should_quit());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn slash_does_not_activate_search_while_library_delete_confirmation_is_open() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::single(TriggerListItem {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: None,
                invocations: vec![hotkey_alias("test", "alt+t")],
                display: "alt+t".to_string(),
                output: "test".to_string(),
                action_type: "text".to_string(),
                target_os: "win".to_string(),
                only_apps: None,
                except_apps: None,
                auto_case: false,
                is_enabled: true,
                usage_count: 0,
                last_used_at: None,
                created_at: 0,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            })]);

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.library_page_mut().open_delete_modal_for_selected();
        handle_tui_key_event(&mut app, plain_key('/'));

        assert!(!app.library_page().is_search_active());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn escape_closes_library_modal_without_changing_page() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(app.active_page(), Page::Library);
        assert!(!app.library_page().is_modal_open());
    }

    fn mouse_at(column: u16, row: u16, kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn left_click(column: u16, row: u16) -> MouseEvent {
        mouse_at(column, row, MouseEventKind::Down(MouseButton::Left))
    }

    const TEST_AREA: ratatui::layout::Rect = ratatui::layout::Rect::new(0, 0, 100, 30);

    #[test]
    fn clicking_settings_search_bar_focuses_search() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert!(!app.settings_page().is_search_active());

        handle_tui_mouse_event(&mut app, left_click(30, 26), TEST_AREA);

        assert!(app.settings_page().is_search_active());
        assert_eq!(app.settings_page().search_query(), "");
    }

    #[test]
    fn clicking_settings_row_selects_without_activating() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        app.settings_page_mut().selected = 1;

        handle_tui_mouse_event(&mut app, left_click(30, 5), TEST_AREA);

        assert_eq!(app.settings_page().selected_index(), 0);
        assert!(app.settings_page().modal().is_none());
    }

    #[test]
    fn clicking_library_row_selects_without_opening() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        app.library_page_mut().replace_items(vec![
            LibraryTrigger::single(TriggerListItem {
                id: "mouse-aaa".to_string(),
                name: String::new(),
                description: None,
                invocations: vec![hotkey_alias("mouse-aaa", "aaa")],
                display: "aaa".to_string(),
                output: "aaa out".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                auto_case: false,
                is_enabled: true,
                usage_count: 0,
                last_used_at: None,
                created_at: 0,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            }),
            LibraryTrigger::single(TriggerListItem {
                id: "mouse-bbb".to_string(),
                name: String::new(),
                description: None,
                invocations: vec![hotkey_alias("mouse-bbb", "bbb")],
                display: "bbb".to_string(),
                output: "bbb out".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                auto_case: false,
                is_enabled: true,
                usage_count: 0,
                last_used_at: None,
                created_at: 0,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            }),
        ]);

        handle_tui_mouse_event(&mut app, left_click(30, 22), TEST_AREA);

        assert_eq!(app.library_page().selected_index(), Some(1));
        assert!(!app.library_page().is_modal_open());
    }

    #[test]
    fn pressing_divider_starts_drag_without_selecting() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);

        handle_tui_mouse_event(&mut app, left_click(49, 10), TEST_AREA);

        assert!(app.library_page().divider_drag());
        assert_eq!(app.library_page().split_ratio(), 0.5);
        assert!(!app.library_page().is_modal_open());
    }

    #[test]
    fn dragging_divider_moves_split_and_release_ends_it() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        handle_tui_mouse_event(&mut app, left_click(49, 10), TEST_AREA);

        handle_tui_mouse_event(
            &mut app,
            mouse_at(60, 10, MouseEventKind::Drag(MouseButton::Left)),
            TEST_AREA,
        );

        let ratio = app.library_page().split_ratio();
        assert!((ratio - 58.0 / 95.0).abs() < 0.01);
        assert!(app.library_page().divider_drag());

        handle_tui_mouse_event(
            &mut app,
            mouse_at(60, 10, MouseEventKind::Up(MouseButton::Left)),
            TEST_AREA,
        );

        assert!(!app.library_page().divider_drag());
        assert!((app.library_page().split_ratio() - 58.0 / 95.0).abs() < 0.01);
    }

    #[test]
    fn hovering_divider_sets_hover_and_leaving_clears() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);

        handle_tui_mouse_event(&mut app, mouse_at(49, 10, MouseEventKind::Moved), TEST_AREA);
        assert!(app.library_page().divider_hover());

        handle_tui_mouse_event(&mut app, mouse_at(10, 10, MouseEventKind::Moved), TEST_AREA);
        assert!(!app.library_page().divider_hover());
    }

    #[test]
    fn wheel_scroll_moves_settings_selection() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);

        handle_tui_mouse_event(
            &mut app,
            mouse_at(30, 10, MouseEventKind::ScrollDown),
            TEST_AREA,
        );

        assert_eq!(app.settings_page().selected_index(), 1);
    }

    #[test]
    fn wheel_scroll_moves_selection_while_search_focused() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('2'), KeyModifiers::NONE);
        handle_tui_key_event(&mut app, plain_key('/'));
        assert!(app.settings_page().is_search_active());

        handle_tui_mouse_event(
            &mut app,
            mouse_at(30, 10, MouseEventKind::ScrollDown),
            TEST_AREA,
        );

        assert_eq!(app.settings_page().selected_index(), 1);
        assert!(app.settings_page().is_search_active());
    }

    #[test]
    fn clicks_are_ignored_while_library_modal_is_open() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();
        let selected_before = app.library_page().selected_index();

        handle_tui_mouse_event(&mut app, left_click(30, 10), TEST_AREA);

        assert!(app.library_page().is_modal_open());
        assert_eq!(app.library_page().selected_index(), selected_before);
    }

    #[test]
    fn pressing_ctrl_c_quits_without_changing_page() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );

        assert_eq!(app.active_page(), Page::Library);
        assert!(app.should_quit());
    }

    #[test]
    fn pressing_ctrl_c_quits_while_search_is_active() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        handle_tui_key_event(&mut app, plain_key('/'));

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );

        assert!(app.should_quit());
    }

    #[test]
    fn pressing_ctrl_c_quits_while_modal_is_open() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();
        assert!(app.library_page().is_modal_open());

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );

        assert!(app.should_quit());
    }

    #[test]
    fn pressing_n_on_library_stays_on_page_without_modal() {
        let mut app = App::default();
        app.handle_key(KeyCode::Char('1'), KeyModifiers::NONE);

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        handle_tui_key_event(&mut app, plain_key('n'));

        assert_eq!(app.active_page(), Page::Library);
        assert!(!app.library_page().is_modal_open());
        assert!(app.library_page().is_search_active());
        assert_eq!(app.library_page().search_query(), "n");
    }
}
