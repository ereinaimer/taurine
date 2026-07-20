use crate::library::{
    LibraryExportModalField, LibraryExportModalState, LibraryImportConflictMode,
    LibraryImportModalField, LibraryImportModalState, RememberedConflictChoice,
};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::{
    cursor::SetCursorStyle,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use taurine_core::error::Result as CoreResult;
use taurine_core::exchange::{AutomationExport, ExistingAutomationConflict, ImportConflictAction};

fn drain_stale_events() {
    loop {
        if !crossterm::event::poll(Duration::from_millis(1)).unwrap_or(false) {
            break;
        }
        let _ = crossterm::event::read();
    }
}

pub(crate) struct OverlaySession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl OverlaySession {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        execute!(stdout, SetCursorStyle::SteadyBar)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.hide_cursor()?;
        drain_stale_events();
        Ok(Self { terminal })
    }
}

impl Drop for OverlaySession {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
    }
}

#[derive(Debug)]
pub struct ExportFormResult {
    pub path: PathBuf,
    pub encrypt: bool,
    pub password: Option<String>,
    pub include_settings: bool,
    pub include_sensitive_settings: bool,
    pub include_stats: bool,
}

#[derive(Debug)]
pub struct ImportFormResult {
    pub path: PathBuf,
    pub password: Option<String>,
    pub include_settings: bool,
    pub include_sensitive_settings: bool,
    pub stats_mode: taurine_core::exchange::ImportStatsMode,
    pub conflict_mode: LibraryImportConflictMode,
}

pub fn run_export_overlay() -> CoreResult<Option<ExportFormResult>> {
    let mut session = OverlaySession::new()
        .map_err(|e| taurine_core::Error::Service(format!("Failed to initialize overlay: {e}")))?;
    let mut state = LibraryExportModalState::new()?;
    let mut last_move = Instant::now();
    const MOVE_DEBOUNCE: Duration = Duration::from_millis(100);

    let result =
        loop {
            let text_focused = matches!(
                state.focus(),
                LibraryExportModalField::Path | LibraryExportModalField::Password
            );
            if text_focused {
                session.terminal.show_cursor().map_err(|e| {
                    taurine_core::Error::Service(format!("Cursor show failed: {e}"))
                })?;
            }
            session.terminal.draw(|f| {
                crate::overlay_ui::render_export_popup(f, &state);
            })?;
            if !text_focused {
                session.terminal.hide_cursor().map_err(|e| {
                    taurine_core::Error::Service(format!("Cursor hide failed: {e}"))
                })?;
            }

            if let Event::Key(key) = crossterm::event::read().map_err(|e| {
                taurine_core::Error::Service(format!("Overlay event read failed: {e}"))
            })? {
                if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    continue;
                }
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
                let interaction = state.handle_key(key);
                if let Some(pending) = interaction.pending_export() {
                    break Some(ExportFormResult {
                        path: pending.path.clone().into(),
                        encrypt: pending.encrypt,
                        password: pending.password.clone(),
                        include_settings: pending.include_settings,
                        include_sensitive_settings: pending.include_sensitive_settings,
                        include_stats: pending.include_stats,
                    });
                }
                if interaction.should_close_modal() {
                    break None;
                }
            }
        };

    Ok(result)
}

pub fn run_import_overlay(path: Option<&str>) -> CoreResult<Option<ImportFormResult>> {
    let mut session = OverlaySession::new()
        .map_err(|e| taurine_core::Error::Service(format!("Failed to initialize overlay: {e}")))?;
    let mut state = match path {
        Some(p) => LibraryImportModalState::with_path(p),
        None => LibraryImportModalState::new(),
    };
    let mut last_move = Instant::now();
    const MOVE_DEBOUNCE: Duration = Duration::from_millis(100);

    let result =
        loop {
            let text_focused = matches!(
                state.focus(),
                LibraryImportModalField::Path | LibraryImportModalField::Password
            );
            if text_focused {
                session.terminal.show_cursor().map_err(|e| {
                    taurine_core::Error::Service(format!("Cursor show failed: {e}"))
                })?;
            }
            session.terminal.draw(|f| {
                crate::overlay_ui::render_import_popup(f, &state);
            })?;
            if !text_focused {
                session.terminal.hide_cursor().map_err(|e| {
                    taurine_core::Error::Service(format!("Cursor hide failed: {e}"))
                })?;
            }

            if let Event::Key(key) = crossterm::event::read().map_err(|e| {
                taurine_core::Error::Service(format!("Overlay event read failed: {e}"))
            })? {
                if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    continue;
                }
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
                let interaction = state.handle_key(key);
                if let Some(prepare) = interaction.pending_import_prepare() {
                    break Some(ImportFormResult {
                        path: prepare.path.clone().into(),
                        password: prepare.password.clone(),
                        include_settings: prepare.options.include_settings,
                        include_sensitive_settings: prepare.options.include_sensitive_settings,
                        stats_mode: prepare.options.stats_mode,
                        conflict_mode: prepare.conflict_mode,
                    });
                }
                if interaction.should_close_modal() {
                    break None;
                }
            }
        };

    Ok(result)
}

pub fn prompt_password(label: &str, with_confirmation: bool) -> CoreResult<Option<String>> {
    let mut session = OverlaySession::new()
        .map_err(|e| taurine_core::Error::Service(format!("Failed to initialize overlay: {e}")))?;
    let mut password = String::new();
    let mut confirm = String::new();
    let mut focus: usize = 0;
    let mut error: Option<String> = None;

    let result =
        loop {
            let text_focused = focus == 0 || (with_confirmation && focus == 1);
            if text_focused {
                session.terminal.show_cursor().map_err(|e| {
                    taurine_core::Error::Service(format!("Cursor show failed: {e}"))
                })?;
            }
            session.terminal.draw(|f| {
                crate::overlay_ui::render_password_popup(
                    f,
                    label,
                    with_confirmation,
                    &password,
                    &confirm,
                    focus,
                    error.as_deref(),
                );
            })?;
            if !text_focused {
                session.terminal.hide_cursor().map_err(|e| {
                    taurine_core::Error::Service(format!("Cursor hide failed: {e}"))
                })?;
            }

            if let Event::Key(key) = crossterm::event::read().map_err(|e| {
                taurine_core::Error::Service(format!("Overlay event read failed: {e}"))
            })? {
                if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    continue;
                }
                match key.code {
                    KeyCode::Esc => break None,
                    KeyCode::Enter => {
                        if focus == 2 || (!with_confirmation && focus == 1) {
                            if with_confirmation && password != confirm {
                                error = Some("Passwords do not match.".to_string());
                            } else if password.is_empty() {
                                error = Some("Password cannot be empty.".to_string());
                            } else {
                                break Some(password.clone());
                            }
                        } else {
                            let max_focus = if with_confirmation { 3 } else { 2 };
                            focus = (focus + 1).min(max_focus);
                        }
                    }
                    KeyCode::Tab => {
                        let max_focus = if with_confirmation { 4 } else { 3 };
                        focus = (focus + 1) % max_focus;
                    }
                    KeyCode::Char(c) => {
                        error = None;
                        if focus == 0 {
                            password.push(c);
                        } else if with_confirmation && focus == 1 {
                            confirm.push(c);
                        }
                    }
                    KeyCode::Backspace => {
                        if focus == 0 {
                            password.pop();
                        } else if with_confirmation && focus == 1 {
                            confirm.pop();
                        }
                    }
                    _ => {}
                }
            }
        };

    Ok(result)
}

pub fn run_conflict_prompt(
    incoming: &AutomationExport,
    existing: &ExistingAutomationConflict,
    remembered_choice: &mut Option<RememberedConflictChoice>,
) -> CoreResult<ImportConflictAction> {
    if let Some(choice) = remembered_choice {
        return Ok(match choice {
            RememberedConflictChoice::OverwriteAll => ImportConflictAction::Overwrite,
            RememberedConflictChoice::SkipAll => ImportConflictAction::Skip,
        });
    }

    let mut session = OverlaySession::new()
        .map_err(|e| taurine_core::Error::Service(format!("Failed to initialize overlay: {e}")))?;
    let mut selected: usize = 0;
    let mut last_move = Instant::now();
    const MOVE_DEBOUNCE: Duration = Duration::from_millis(100);

    let result = loop {
        session.terminal.draw(|f| {
            crate::overlay_ui::render_conflict_popup(f, incoming, existing, selected);
        })?;

        if let Event::Key(key) = crossterm::event::read()
            .map_err(|e| taurine_core::Error::Service(format!("Overlay event read failed: {e}")))?
        {
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            match key.code {
                KeyCode::Esc => break ImportConflictAction::Skip,
                KeyCode::Enter => {
                    break match selected {
                        0 => ImportConflictAction::Overwrite,
                        1 => ImportConflictAction::Skip,
                        2 => {
                            *remembered_choice = Some(RememberedConflictChoice::OverwriteAll);
                            ImportConflictAction::Overwrite
                        }
                        3 => {
                            *remembered_choice = Some(RememberedConflictChoice::SkipAll);
                            ImportConflictAction::Skip
                        }
                        _ => ImportConflictAction::Skip,
                    };
                }
                KeyCode::Down | KeyCode::Char('j')
                    if last_move.elapsed() >= MOVE_DEBOUNCE && selected < 3 =>
                {
                    selected += 1;
                    last_move = Instant::now();
                }
                KeyCode::Up | KeyCode::Char('k')
                    if last_move.elapsed() >= MOVE_DEBOUNCE && selected > 0 =>
                {
                    selected -= 1;
                    last_move = Instant::now();
                }
                _ => {}
            }
        }
    };

    Ok(result)
}
