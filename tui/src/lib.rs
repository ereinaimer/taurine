// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

mod layout_store;
mod overlay;
mod overlay_fx;
mod screen_fx;
pub mod terminal;
mod terminal_title;
mod theme;
pub use crate::widgets::library::actions::{LibraryImportConflictMode, RememberedConflictChoice};
pub use overlay::{
    ExportFormResult, ImportFormResult, run_ai_overlay, run_conflict_prompt, run_export_overlay,
    run_import_overlay,
};
mod widgets;

use std::io;
use std::time::{Duration, Instant};

use crate::layout_store::TuiLayout;
use crate::overlay_fx::OverlayTransition;
use crate::theme::Theme;
use crate::widgets::home;
use crate::widgets::library;
use crate::widgets::nav_rail::{self, ActivePage, NavRailItem};
use crate::widgets::palette;
use crate::widgets::panes;
use crate::widgets::settings;
use crate::widgets::toggle::{TOGGLE_ANIM_FRAME, ToggleId};
use crate::widgets::util;
use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend, style::Style, widgets::Block};
use terminal::app::App;
use terminal::event::{Event, EventHandler};
use tracing::error;
use widgets::notification;

const EVENT_TICK_RATE: Duration = Duration::from_millis(250);

/// Fast poll while the recorder is open: Windows terminals never report
/// modifier-only presses, so the overlay reads them from the host instead.
const RECORDER_POLL_RATE: Duration = Duration::from_millis(33);

pub fn run() -> taurine_core::Result<()> {
    let mut app = App::default();
    // honey: the saved layout lands before the first draw so the panes
    // open at their stored widths instead of flashing the defaults.
    if let Some(layout) = layout_store::load() {
        app.apply_layout(layout);
    }
    refresh_library_page(&mut app);
    refresh_settings_page(&mut app);
    refresh_home_page(&mut app);

    let mut terminal = TerminalGuard::new()?;
    app.load_rail_icons();
    select_first_visible_library_row(&mut app, &mut terminal);
    select_first_visible_settings_row(&mut app, &mut terminal);
    app.begin_launch_transition();
    setup_signal_handler(|code| std::process::exit(code));
    let mut events = EventHandler::new(EVENT_TICK_RATE);
    let mut last_area = ratatui::layout::Rect::default();
    let mut previous_layers = overlay_fx::layers(&app);
    let mut previous_rects = Vec::new();
    loop {
        let animating = app.advance_toggle_animations(Instant::now());
        let elapsed = app.frame_elapsed();
        terminal.terminal.draw(|frame| {
            let area = frame.area();
            last_area = area;
            render_app(frame, &mut app, area);
            if let Some(effect) = app.transition_effect_mut() {
                effect.process(elapsed.into(), frame.buffer_mut(), area);
            }
        })?;
        app.clear_finished_transition();
        if app.is_closing() && !app.transition_active() {
            persist_layout_if_needed(&mut app, true);
            break;
        }

        if previous_layers.is_any_open() {
            let full = workspace_full(&app, last_area);
            previous_rects = overlay_fx::transition_rects(&app, last_area, full);
        }

        let transition_active = app.transition_active();
        let event = if animating || transition_active {
            let frame = if transition_active {
                screen_fx::SCREEN_FX_FRAME
            } else {
                TOGGLE_ANIM_FRAME
            };
            events.next_with_timeout(frame)?
        } else if app.hotkey_recorder().is_some() {
            events.next_with_timeout(RECORDER_POLL_RATE)?
        } else {
            events.next()?
        };
        // honey: refresh before the event is handled so a chord key sees
        // the modifier state at the instant it went down.
        if let Some(recorder) = app.hotkey_recorder_mut() {
            recorder.poll_physical();
        }
        let layout_released = matches!(
            &event,
            Event::Mouse(mouse) if matches!(mouse.kind, MouseEventKind::Up(_))
        );
        match event {
            Event::Key(key) => {
                let full = workspace_full(&app, last_area);
                track_content_width(&mut app, full);
                let create_auto_case = app.library_page().create_auto_case();
                let palette_was_open = app.palette_open();
                handle_tui_key_event(&mut app, key);
                // honey: the palette toggle reads the daemon's real pause
                // flag, so refresh before the first palette frame draws.
                if !palette_was_open && app.palette_open() {
                    app.refresh_daemon_paused();
                }
                if let Some(from_on) = create_auto_case
                    && app.library_page().create_auto_case() == Some(!from_on)
                {
                    app.start_toggle_anim(ToggleId::CreateAutoCase, from_on);
                }
            }
            Event::Mouse(mouse) => handle_tui_mouse_event(&mut app, mouse, last_area),
            Event::Tick => {
                let interaction = app.library_page_mut().autosave_tick();
                apply_library_interaction(&mut app, interaction);
                // honey: keyboard slider steps queue their write so a held
                // arrow key coalesces into one settings save per tick.
                let interaction = app.settings_page_mut().slider_autosave_tick();
                apply_settings_interaction(&mut app, interaction);
                if app.nav_rail().active_page == ActivePage::Home {
                    let now = library::now_millis();
                    if app.home_page().refresh_due(now) {
                        refresh_home_page(&mut app);
                    }
                }
            }
        }

        // honey: a release ends any divider drag, so the settled width is
        // written once here instead of on every drag tick.
        if layout_released {
            persist_layout_if_needed(&mut app, false);
        }

        // honey: a newly appeared overlay sweeps in; closes fade out smoothly.
        let next_layers = overlay_fx::layers(&app);
        if overlay_fx::appeared(&previous_layers, &next_layers) {
            play_overlay_open(&mut terminal, &mut app, last_area)?;
        } else if overlay_fx::disappeared(&previous_layers, &next_layers) {
            let closing = overlay_fx::closed_rects(&previous_layers, &next_layers, &previous_rects);
            play_overlay_close(&mut terminal, &mut app, last_area, &closing)?;
            previous_rects.clear();
        }
        previous_layers = next_layers;

        if app.should_quit() {
            // honey: the current width must survive a quit mid-drag.
            persist_layout_if_needed(&mut app, true);
            break;
        }
    }

    Ok(())
}

/// Full-height gutter lines for the active workspace page. The line
/// travels the whole terminal height even though the panes themselves
/// keep the page's vertical margins.
fn render_page_dividers(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    full: ratatui::layout::Rect,
    app: &App,
    theme: &Theme,
) {
    if library_visible(app) {
        let page = app.library_page();
        let content_divider_row = library::detail::content_divider_row(
            full,
            page.split_ratio(),
            page.detail_ratio(),
            page,
        );
        for (side, column) in
            library::visible_divider_columns(full, page.split_ratio(), page.detail_ratio())
        {
            panes::render_divider(
                frame,
                area,
                column,
                page.dividers().color(theme, side),
                |row| library::divider_glyph(side, row, content_divider_row),
            );
        }
    }
}

/// Library area after the navigation rail takes its columns. One source
/// of truth for rendering, hit-testing, and divider drags.
fn workspace_full(app: &App, area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    let area = nav_rail::workspace(area, app.nav_rail().collapsed);
    terminal::mouse::library_full_area(terminal::mouse::frame_layout(area).page)
}

/// Full frame paint, shared by the main loop and the overlay open sweep
/// so the transition starts from exactly what is on screen.
fn render_app(frame: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let theme = app.theme();

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );

    let full = workspace_full(app, area);

    render_page_content(frame, full, app, theme);

    if area.height > 0 {
        render_page_dividers(frame, area, full, app, theme);
    }

    let any_modal = app.library_page().is_modal_open()
        || app.is_settings_overlay_open()
        || app.palette_open()
        || app.hotkey_recorder().is_some();
    let (rail, icons) = app.nav_rail_graphics();
    nav_rail::render(frame, area, theme, rail, icons, any_modal);

    if let Some(msg) = app.notification() {
        notification::render_notification(frame, area, theme, msg);
    }

    if any_modal {
        dim_frame(frame);
    }

    // honey: modals render last so they sit above panes,
    // dividers, and toasts.
    if let Some(modal) = app.library_page().modal() {
        library::modals::render_library_modal(
            frame,
            full,
            theme,
            modal,
            app.toggle_animations(),
            app.toggle_hover(),
        );
    }
    if app.is_settings_overlay_open() {
        settings::render_settings_overlay(
            frame,
            area,
            theme,
            app.settings_page(),
            app.toggle_animations(),
            app.toggle_hover(),
        );
        if let Some(modal) = app.settings_page().modal() {
            // honey: a nested popup dims the layer directly beneath it;
            // the workspace already carries the global scrim, so only
            // the settings popup gets the extra pass.
            overlay_fx::dim_rect(
                frame,
                settings::overlay_popup(area),
                overlay_fx::OVERLAY_SCRIM_FACTOR,
            );
            settings::modals::render_settings_modal(frame, area, theme, modal);
        }
    }
    if let Some(palette) = app.palette() {
        if app.is_settings_overlay_open() {
            overlay_fx::dim_rect(
                frame,
                settings::overlay_popup(area),
                overlay_fx::OVERLAY_SCRIM_FACTOR,
            );
        }
        palette::render(
            frame,
            area,
            theme,
            palette,
            app.daemon_paused(),
            &app.settings_page().settings().pause_hotkey,
        );
    }

    // honey: the hotkey recorder sits above every other layer; it is
    // the only thing that can be open while it captures a chord.
    if let Some(recorder) = app.hotkey_recorder() {
        if app.palette().is_some() {
            overlay_fx::dim_rect(
                frame,
                palette::popup(area),
                overlay_fx::OVERLAY_SCRIM_FACTOR,
            );
        } else if app.is_settings_overlay_open() {
            overlay_fx::dim_rect(
                frame,
                settings::overlay_popup(area),
                overlay_fx::OVERLAY_SCRIM_FACTOR,
            );
        }
        library::recorder::render(frame, area, theme, recorder);
    }
}

/// Plays the shared evolve-into sweep over every newly visible overlay
/// popup, blocking the loop for one beat at ~60fps. Input during the
/// sweep queues in the terminal and processes right after.
fn play_overlay_open(
    terminal: &mut TerminalGuard,
    app: &mut App,
    area: ratatui::layout::Rect,
) -> taurine_core::Result<()> {
    use std::time::{Duration as StdDuration, Instant};

    use tachyonfx::EffectRenderer;

    let full = workspace_full(app, area);
    let rects: Vec<ratatui::layout::Rect> = overlay_fx::transition_rects(app, area, full)
        .into_iter()
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .collect();
    if rects.is_empty() {
        return Ok(());
    }
    let theme = app.theme();
    let mut effects: Vec<tachyonfx::Effect> = rects
        .iter()
        .map(|_| overlay_fx::open_effect(theme))
        .collect();
    let _cursor_guard = overlay_fx::CursorSuppressGuard::new();
    let frame_budget = StdDuration::from_millis(16);
    let start = Instant::now() - frame_budget;
    let mut last = start;
    while effects.iter().any(|effect| effect.running()) {
        let elapsed = last.elapsed();
        last = Instant::now();
        terminal.terminal.draw(|frame| {
            render_app(frame, app, area);
            for (effect, rect) in effects.iter_mut().zip(rects.iter()) {
                frame.render_effect(effect, *rect, elapsed.into());
            }
        })?;
        std::thread::sleep(frame_budget.saturating_sub(last.elapsed()));
    }
    Ok(())
}

/// Plays the smooth gradient fade-in transition over the closing overlay rects when an
/// overlay dismisses, revealing the underlying screen at ~60fps over 300ms while
/// restoring normal screen lighting.
fn play_overlay_close(
    terminal: &mut TerminalGuard,
    app: &mut App,
    area: ratatui::layout::Rect,
    closing_rects: &[ratatui::layout::Rect],
) -> taurine_core::Result<()> {
    use std::time::{Duration as StdDuration, Instant};

    use tachyonfx::EffectRenderer;

    let rects: Vec<ratatui::layout::Rect> = closing_rects
        .iter()
        .copied()
        .filter(|rect| rect.width > 0 && rect.height > 0)
        .collect();
    if rects.is_empty() {
        return Ok(());
    }
    let theme = app.theme();
    let mut effects: Vec<tachyonfx::Effect> = rects
        .iter()
        .map(|_| overlay_fx::close_effect(theme))
        .collect();
    let _cursor_guard = overlay_fx::CursorSuppressGuard::new();
    let frame_budget = StdDuration::from_millis(16);
    let total_secs = (overlay_fx::OVERLAY_FX_MS as f32) / 1000.0;
    let any_modal = app.library_page().is_modal_open() || app.is_settings_overlay_open();

    // Start with the first frame budget already elapsed so the transition begins
    // visibly interpolating on the very first draw, eliminating any freeze
    // or perceived lag when dismissing.
    let start = Instant::now() - frame_budget;
    let mut last = start;

    while effects.iter().any(|effect| effect.running()) {
        let elapsed = last.elapsed();
        last = Instant::now();
        let progress = (start.elapsed().as_secs_f32() / total_secs).clamp(0.0, 1.0);
        let dim_factor = 0.4 + 0.6 * progress;

        terminal.terminal.draw(|frame| {
            render_app(frame, app, area);
            if !any_modal {
                dim_frame_factor(frame, dim_factor);
            }
            for (effect, rect) in effects.iter_mut().zip(rects.iter()) {
                frame.render_effect(effect, *rect, elapsed.into());
            }
        })?;
        std::thread::sleep(frame_budget.saturating_sub(last.elapsed()));
    }
    Ok(())
}

fn dim_frame(frame: &mut ratatui::Frame) {
    dim_frame_factor(frame, overlay_fx::OVERLAY_SCRIM_FACTOR);
}

fn dim_frame_factor(frame: &mut ratatui::Frame, factor: f32) {
    overlay_fx::dim_rect(frame, frame.area(), factor);
}

fn render_page_content(
    frame: &mut ratatui::Frame,
    full: ratatui::layout::Rect,
    app: &mut App,
    theme: &Theme,
) {
    match app.nav_rail().active_page {
        ActivePage::Home => home::render_home_content(frame, full, theme, app.home_page_mut()),
        ActivePage::Library => library::render_library_content(
            frame,
            full,
            theme,
            app.library_page(),
            app.toggle_animations(),
            app.toggle_hover(),
            app.props_hover(),
        ),
    }
}

/// Whether the library owns the workspace, and therefore pointer and
/// keyboard input.
fn library_visible(app: &App) -> bool {
    app.nav_rail().active_page == ActivePage::Library
}

/// Whether the home dashboard owns the workspace.
fn home_visible(app: &App) -> bool {
    app.nav_rail().active_page == ActivePage::Home
}

/// Switches the workspace page, flushing any pending library edit and
/// dropping hover state that belongs to the page being left.
fn switch_page(app: &mut App, page: ActivePage) {
    if app.nav_rail().active_page == page {
        return;
    }
    let flush = app.library_page_mut().commit_edit();
    apply_library_interaction(app, flush);
    app.library_page_mut().set_divider_hover(None);
    app.set_toggle_hover(None);
    app.set_props_hover(None);
    app.nav_rail_mut().set_active_page(page);
    if page == ActivePage::Home {
        app.home_page_mut().on_show();
    }
}

fn handle_tui_key_event(app: &mut App, key: crossterm::event::KeyEvent) {
    app.clear_notification();

    // honey: the recorder captures every chord, including Ctrl+C and
    // Ctrl+P, so it takes the keyboard before the global shortcuts.
    if app.hotkey_recorder().is_some() {
        handle_hotkey_recorder_key(app, key);
        return;
    }

    // honey: Ctrl+C quits cleanly from anywhere, including modals and
    // search, since raw mode delivers it as a key event, not a signal.
    // Pending edits flush first so nothing typed is lost to timing.
    // The one exception: an active content selection copies instead.
    if key.code == crossterm::event::KeyCode::Char('c')
        && key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
    {
        if app.is_closing() {
            app.request_quit();
            return;
        }
        if library_visible(app)
            && !app.library_page().is_modal_open()
            && !app.is_settings_overlay_open()
            && !app.palette_open()
            && app.library_page().has_content_selection()
        {
            let interaction = app.library_page_mut().handle_key(key);
            apply_library_interaction(app, interaction);
            return;
        }
        let interaction = app.library_page_mut().commit_edit();
        apply_library_interaction(app, interaction);
        app.begin_close_transition();
        return;
    }

    // honey: Ctrl+P toggles the command palette above every layer; the
    // close sweep owns the screen once quitting starts.
    if key.code == crossterm::event::KeyCode::Char('p')
        && key.modifiers == crossterm::event::KeyModifiers::CONTROL
    {
        if !app.is_closing() {
            app.toggle_palette();
        }
        return;
    }

    // honey: the palette owns the keyboard while open; Ctrl+C above
    // still quits, and Enter hands the command to the app layer.
    if app.palette_open() {
        let outcome = app.palette_mut().map(|palette| palette.handle_key(key));
        match outcome {
            Some(palette::PaletteOutcome::Close) => app.close_palette(),
            Some(palette::PaletteOutcome::Run(command)) => {
                app.close_palette();
                execute_palette_command(app, command);
            }
            _ => {}
        }
        return;
    }

    // honey: Ctrl+N opens the create modal; a library modal owns
    // the screen while open and the settings overlay consumes its
    // own keys, so both wait.
    if key.code == crossterm::event::KeyCode::Char('n')
        && key.modifiers == crossterm::event::KeyModifiers::CONTROL
    {
        if library_visible(app)
            && !app.library_page().is_modal_open()
            && !app.is_settings_overlay_open()
        {
            let flush = app.library_page_mut().commit_edit();
            apply_library_interaction(app, flush);
            app.library_page_mut().open_create_modal();
        }
        return;
    }

    // honey: workspace shortcuts mirror their palette commands; open
    // dialogs keep the keyboard, so the toggles wait for them.
    if key.modifiers == crossterm::event::KeyModifiers::CONTROL {
        let dialog_open = app.library_page().is_modal_open() || app.is_settings_overlay_open();
        match key.code {
            crossterm::event::KeyCode::Char('i') => {
                open_exchange_modal(app, library::LibraryPageState::open_import_modal);
                return;
            }
            crossterm::event::KeyCode::Char('e') => {
                open_exchange_modal(app, library::LibraryPageState::open_export_modal);
                return;
            }
            crossterm::event::KeyCode::Char('f') if !dialog_open => {
                app.library_page_mut()
                    .toggle_panel(library::DividerSide::List);
                return;
            }
            crossterm::event::KeyCode::Char('d') if !dialog_open => {
                app.library_page_mut()
                    .toggle_panel(library::DividerSide::Props);
                return;
            }
            crossterm::event::KeyCode::Char('b') if !dialog_open => {
                app.nav_rail_mut().toggle();
                return;
            }
            _ => {}
        }
    }

    // honey: Ctrl+, toggles the settings overlay; a library modal owns
    // the screen while open, so the toggle waits for it to close.
    if key.code == crossterm::event::KeyCode::Char(',')
        && key.modifiers == crossterm::event::KeyModifiers::CONTROL
    {
        if app.is_settings_overlay_open() {
            app.close_settings_overlay();
        } else if !app.library_page().is_modal_open() {
            let flush = app.library_page_mut().commit_edit();
            apply_library_interaction(app, flush);
            app.open_settings_overlay();
        }
        return;
    }

    if app.is_settings_overlay_open() {
        // honey: Esc exits search, then editor modals, then the overlay
        // itself; settings handle_key owns the first two layers.
        if key.code == crossterm::event::KeyCode::Esc
            && key.modifiers == crossterm::event::KeyModifiers::NONE
            && !app.settings_page().is_modal_open()
            && !app.settings_page().is_search_active()
            && app.settings_page().inline_edit().is_none()
        {
            app.close_settings_overlay();
            return;
        }
        let interaction = app.settings_page_mut().handle_key(key);
        apply_settings_interaction(app, interaction);
        return;
    }

    if app.nav_rail().active_page == ActivePage::Home {
        use crossterm::event::{KeyCode, KeyModifiers};

        if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::NONE {
            refresh_home_page(app);
            return;
        }
        // honey: the activity feed owns Up/Down/Page/Home/End on Home;
        // the page step comes from the viewport the last frame drew.
        if matches!(key.modifiers, KeyModifiers::NONE | KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::Up => app.home_page_mut().move_selection(-1),
                KeyCode::Down => app.home_page_mut().move_selection(1),
                KeyCode::PageUp => app.home_page_mut().page_selection(-1),
                KeyCode::PageDown => app.home_page_mut().page_selection(1),
                KeyCode::Home => app.home_page_mut().select_first(),
                KeyCode::End => app.home_page_mut().select_last(),
                KeyCode::Char('c') | KeyCode::Enter => copy_selected_home_event(app),
                _ => {}
            }
        }
        return;
    }

    if library_visible(app)
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

    if library_visible(app) {
        let interaction = app.library_page_mut().handle_key(key);
        apply_library_interaction(app, interaction);
    }
}

/// Recorder keyboard: physical presses drive the live chord, Esc and
/// Enter close it, Backspace clears.
fn handle_hotkey_recorder_key(app: &mut App, key: crossterm::event::KeyEvent) {
    let action = app
        .hotkey_recorder_mut()
        .map(|recorder| recorder.press(key));
    match action {
        Some(library::recorder::RecorderAction::Cancel) => app.set_hotkey_recorder(None),
        Some(library::recorder::RecorderAction::Done) => commit_hotkey_recorder(app),
        _ => {}
    }
}

/// Opens the recorder on the selected row's own invocation when that
/// row is a hotkey. Returns whether it took the click.
fn open_recorder_for_selected(app: &mut App) -> bool {
    use library::recorder::{HotkeyRecorderState, HotkeyTarget};

    let is_hotkey = {
        let page = app.library_page();
        page.selected_index()
            .and_then(|index| page.item_at_filtered(index))
            .is_some_and(|item| item.is_hotkey())
    };
    if !is_hotkey {
        return false;
    }
    app.set_hotkey_recorder(Some(HotkeyRecorderState::new(HotkeyTarget::PrimaryTrigger)));
    true
}

/// Opens the recorder on the sibling alias drawn at `line` in the
/// properties block. Word aliases keep their inline text edit.
fn open_recorder_for_alias(app: &mut App, line: usize) -> bool {
    use library::recorder::{HotkeyRecorderState, HotkeyTarget};

    let is_alias = {
        let page = app.library_page();
        page.selected_index()
            .and_then(|index| page.item_at_filtered(index))
            .and_then(|item| {
                library::detail::sibling_aliases(item.aliases(), item.trigger())
                    .get(line)
                    .copied()
            })
            .is_some_and(crate::widgets::hotkey::is_hotkey_chord)
    };
    if !is_alias {
        return false;
    }
    app.set_hotkey_recorder(Some(HotkeyRecorderState::new(HotkeyTarget::AliasIndex(
        line,
    ))));
    true
}

/// Builds the same rename edit the inline editor uses, pointed at the
/// recorder's target.
fn recorder_pending_edit(
    app: &App,
    target: library::recorder::HotkeyTarget,
    chord: &str,
) -> Option<library::PendingLibraryEdit> {
    use library::recorder::HotkeyTarget;

    let page = app.library_page();
    let index = page.selected_index()?;
    let item = page.item_at_filtered(index)?;
    let trigger = match target {
        HotkeyTarget::PrimaryTrigger => item.trigger().to_string(),
        HotkeyTarget::AliasIndex(alias) => {
            library::detail::sibling_aliases(item.aliases(), item.trigger())
                .get(alias)
                .map(|alias| (*alias).to_string())?
        }
        HotkeyTarget::Setting(_) => return None,
    };
    Some(library::PendingLibraryEdit {
        trigger_id: item.id().to_string(),
        trigger,
        field: library::EditedField::Trigger(chord.to_string()),
        restore_index: index,
    })
}

/// Saves the recorded chord and closes the overlay. A rejected chord
/// keeps the overlay open with the reason in its header.
fn commit_hotkey_recorder(app: &mut App) {
    let Some((target, chord)) = app
        .hotkey_recorder_mut()
        .and_then(|recorder| recorder.commit().map(|chord| (recorder.target, chord)))
    else {
        return;
    };
    if let library::recorder::HotkeyTarget::Setting(key) = target {
        let pending = settings::PendingSettingSave {
            key,
            value: Some(chord),
        };
        match pending.apply() {
            Ok(()) => {
                app.set_hotkey_recorder(None);
                refresh_settings_page(app);
            }
            Err(error) => {
                if let Some(recorder) = app.hotkey_recorder_mut() {
                    recorder.error = Some(error.to_string());
                }
            }
        }
        return;
    }
    let Some(pending) = recorder_pending_edit(app, target, &chord) else {
        app.set_hotkey_recorder(None);
        return;
    };
    match pending.apply() {
        Ok(()) => {
            app.set_hotkey_recorder(None);
            refresh_library_page(app);
            app.library_page_mut()
                .select_row(&pending.trigger_id, &chord);
        }
        Err(error) => {
            if let Some(recorder) = app.hotkey_recorder_mut() {
                recorder.error = Some(error.to_string());
            }
        }
    }
}

/// Opens the import or export dialog, switching to the library first so
/// the dialog is visible and owns keyboard input.
fn open_exchange_modal(app: &mut App, open: fn(&mut library::LibraryPageState)) {
    if app.library_page().is_modal_open() || app.is_settings_overlay_open() {
        app.set_notification("Close the current dialog first.".to_string());
        return;
    }
    switch_page(app, ActivePage::Library);
    let flush = app.library_page_mut().commit_edit();
    apply_library_interaction(app, flush);
    open(app.library_page_mut());
}

/// Runs a palette command. Gated commands mirror their shortcut rules
/// so the palette never opens a dialog behind another one.
fn execute_palette_command(app: &mut App, command: palette::PaletteCommand) {
    use palette::PaletteCommand;
    match command {
        PaletteCommand::ToggleSearchPane => app
            .library_page_mut()
            .toggle_panel(library::DividerSide::List),
        PaletteCommand::TogglePropertiesPane => app
            .library_page_mut()
            .toggle_panel(library::DividerSide::Props),
        PaletteCommand::ToggleNavRail => app.nav_rail_mut().toggle(),
        PaletteCommand::ResetLayout => app.library_page_mut().reset_layout(),
        PaletteCommand::NewTrigger => {
            if app.library_page().is_modal_open() || app.is_settings_overlay_open() {
                app.set_notification("Close the current dialog first.".to_string());
                return;
            }
            let flush = app.library_page_mut().commit_edit();
            apply_library_interaction(app, flush);
            app.library_page_mut().open_create_modal();
        }
        PaletteCommand::ImportTriggers => {
            open_exchange_modal(app, library::LibraryPageState::open_import_modal);
        }
        PaletteCommand::ExportTriggers => {
            open_exchange_modal(app, library::LibraryPageState::open_export_modal);
        }
        PaletteCommand::OpenSettings => {
            if app.is_settings_overlay_open() {
                app.close_settings_overlay();
            } else if app.library_page().is_modal_open() {
                app.set_notification("Close the current dialog first.".to_string());
            } else {
                let flush = app.library_page_mut().commit_edit();
                apply_library_interaction(app, flush);
                app.open_settings_overlay();
            }
        }
        PaletteCommand::RestartTaurine => {
            // honey: the service restart blocks up to ~10s; move it to
            // a worker if the pause starts feeling slow.
            let start_on_boot = app.settings_page().settings().start_on_boot;
            match taurine_core::service::restart(start_on_boot) {
                Ok(()) => {
                    // A restarted daemon always comes back unpaused.
                    app.set_daemon_paused(false);
                    app.set_notification("Taurine restarted.".to_string());
                }
                Err(err) => app.set_notification(format!("Restart failed: {err}")),
            }
        }
        // honey: the pause/resume RPC blocks up to the 2s transport
        // timeout; move it to a worker if the UI starts feeling slow.
        PaletteCommand::TogglePause => {
            let paused = !app.daemon_paused();
            match taurine_core::rpc::set_daemon_paused(paused) {
                Ok(()) => {
                    app.set_daemon_paused(paused);
                    app.set_notification(
                        if paused {
                            "Taurine paused."
                        } else {
                            "Taurine resumed."
                        }
                        .to_string(),
                    );
                }
                Err(err) => app.set_notification(err),
            }
        }
        PaletteCommand::Quit => {
            if app.is_closing() {
                app.request_quit();
            } else {
                let flush = app.library_page_mut().commit_edit();
                apply_library_interaction(app, flush);
                app.begin_close_transition();
            }
        }
    }
}

/// Tracks the content text width for caret math. Width mirrors the
/// render path (border plus padding); zero falls back to unwrapped math.
fn track_content_width(app: &mut App, page: ratatui::layout::Rect) {
    let ratios = (
        app.library_page().split_ratio(),
        app.library_page().detail_ratio(),
    );
    let content = library::detail::center_content(page, ratios.0, ratios.1);
    app.library_page_mut()
        .set_content_width(content.width.saturating_sub(4));
}

/// Recorder pointer: hover lights keycaps, clicks toggle them or hit a
/// chip.
fn handle_hotkey_recorder_mouse(
    app: &mut App,
    mouse: crossterm::event::MouseEvent,
    area: ratatui::layout::Rect,
) {
    use crossterm::event::{MouseButton, MouseEventKind};

    match mouse.kind {
        MouseEventKind::Moved => {
            if let Some(recorder) = app.hotkey_recorder_mut() {
                recorder.hover(area, mouse.column, mouse.row);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let action = app
                .hotkey_recorder_mut()
                .map(|recorder| recorder.click(area, mouse.column, mouse.row));
            match action {
                Some(library::recorder::RecorderAction::Cancel) => app.set_hotkey_recorder(None),
                Some(library::recorder::RecorderAction::Done) => commit_hotkey_recorder(app),
                _ => {}
            }
        }
        _ => {}
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

    let modal_open = app.library_page().is_modal_open() || app.is_settings_overlay_open();

    // honey: grabbing a divider starts a drag; pointer motion after that
    // moves the split freely without re-hitting the gutter column.
    fn grab_divider(app: &mut App, page: ratatui::layout::Rect, column: u16, row: u16) -> bool {
        let ratios = (
            app.library_page().split_ratio(),
            app.library_page().detail_ratio(),
        );
        let Some(side) = library::divider_hit(page, ratios.0, ratios.1, column, row) else {
            return false;
        };
        // honey: double-click resets the pane instead of grabbing.
        if app.library_page_mut().divider_double_click(side) {
            return true;
        }
        app.library_page_mut().set_divider_drag(Some(side));
        true
    }

    fn drag_divider_to(app: &mut App, page: ratatui::layout::Rect, column: u16) {
        let Some(side) = app.library_page().divider_drag() else {
            return;
        };
        match side {
            library::DividerSide::List => {
                let ratio = library::split_ratio_for_column(page, column);
                app.library_page_mut().set_split_ratio(ratio);
            }
            library::DividerSide::Props => {
                let ratio = library::detail_ratio_for_column(
                    page,
                    app.library_page().split_ratio(),
                    column,
                );
                app.library_page_mut().set_detail_ratio(ratio);
            }
        }
    }

    // honey: the recorder owns the pointer while it is open. The scrim
    // deliberately ignores clicks so a stray press cannot discard a
    // half-recorded chord.
    if app.hotkey_recorder().is_some() {
        handle_hotkey_recorder_mouse(app, mouse, area);
        return;
    }

    match mouse.kind {
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            if app.palette_open() {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                if let Some(palette) = app.palette_mut() {
                    palette.handle_key(scroll_key(down));
                }
                return;
            }
            if app.library_page().header_menu_open() {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                app.library_page_mut().move_header_menu_cursor(down);
                return;
            }
            if app.library_page().app_filter_menu_open() {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                app.library_page_mut().move_app_filter_cursor(down);
                return;
            }
            // honey: wheel over the settings overlay scrolls its list;
            // wheel outside the popup never leaks into the library.
            if app.is_settings_overlay_open() {
                let popup = settings::overlay_popup(area);
                if terminal::mouse::contains(popup, mouse.column, mouse.row) {
                    handle_tui_key_event(app, scroll_key(mouse.kind == MouseEventKind::ScrollDown));
                }
                return;
            }
            if modal_open {
                return;
            }
            if home_visible(app) {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                app.home_page_mut()
                    .move_selection(if down { 1 } else { -1 });
                return;
            }
            if !library_visible(app) {
                return;
            }
            let full = workspace_full(app, area);
            track_content_width(app, full);
            let page = app.library_page();
            if library::detail::detail_contains(
                full,
                page.split_ratio(),
                page.detail_ratio(),
                mouse.column,
                mouse.row,
            ) {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                let content =
                    library::detail::center_content(full, page.split_ratio(), page.detail_ratio());
                let item = match page
                    .selected_index()
                    .and_then(|index| page.item_at_filtered(index))
                {
                    Some(item) => item.clone(),
                    None => return,
                };
                let max = library::detail::content_scroll_max(content.height, content.width, &item);
                app.library_page_mut()
                    .scroll_detail(if down { 1 } else { -1 }, max);
                return;
            }
            handle_tui_key_event(app, scroll_key(mouse.kind == MouseEventKind::ScrollDown));
        }
        MouseEventKind::Moved => {
            app.set_props_hover(None);
            // honey: the rail is behind every overlay, so hover only
            // tracks it on the bare workspace.
            if app.palette_open() || modal_open {
                app.nav_rail_mut().clear_hover();
            } else if app.nav_rail().dragging() {
                app.nav_rail_mut().drag_to(area, mouse.column);
                return;
            } else {
                app.nav_rail_mut().hover_at(area, mouse.column, mouse.row);
                if app.nav_rail().hovered_item.is_some() || app.nav_rail().hovered_divider() {
                    app.set_toggle_hover(None);
                    return;
                }
            }
            if app.palette_open() {
                app.set_toggle_hover(None);
                return;
            }
            if home_visible(app) {
                app.set_toggle_hover(None);
                return;
            }
            if !library_visible(app) {
                app.set_toggle_hover(None);
                return;
            }
            if app.library_page().tags_menu_open() {
                let full = workspace_full(app, area);
                app.library_page_mut()
                    .hover_tags_menu(full, mouse.column, mouse.row);
                return;
            }
            if app.library_page().app_filter_menu_open() {
                let full = workspace_full(app, area);
                app.library_page_mut()
                    .hover_app_filter_menu(full, mouse.column, mouse.row);
                return;
            }
            if app.is_settings_overlay_open() {
                if app.settings_page().modal().is_some() {
                    app.set_toggle_hover(None);
                    return;
                }
                let popup = settings::overlay_popup(area);
                let content = settings::overlay_content(settings::overlay_body(popup));
                let hovered =
                    match settings::hit_test(content, app.settings_page(), mouse.column, mouse.row)
                    {
                        Some(settings::SettingsHit::Toggle(key)) => {
                            Some(ToggleId::Setting(key.storage_key()))
                        }
                        _ => None,
                    };
                app.set_toggle_hover(hovered);
                return;
            }
            if matches!(
                app.library_page().modal(),
                Some(library::LibraryModal::Create(_))
            ) {
                let full = workspace_full(app, area);
                app.library_page_mut()
                    .hover_create_divider(full, mouse.column, mouse.row);
                app.library_page_mut()
                    .drag_create_divider(full, mouse.column);
                let hovered = app.library_page().modal().and_then(|modal| match modal {
                    library::LibraryModal::Create(draft) => {
                        library::modals::create_toggle_hit(draft, full, mouse.column, mouse.row)
                    }
                    _ => None,
                });
                app.set_toggle_hover(hovered);
                return;
            }
            if modal_open {
                app.set_toggle_hover(None);
                return;
            }
            let full = workspace_full(app, area);
            let page = app.library_page();
            let hover = library::divider_hit(
                full,
                page.split_ratio(),
                page.detail_ratio(),
                mouse.column,
                mouse.row,
            );
            let dragging = page.divider_drag();
            if dragging.is_some() {
                drag_divider_to(app, full, mouse.column);
                app.set_toggle_hover(None);
            } else {
                let header_hover = library::detail::hit_test(
                    full,
                    page.split_ratio(),
                    page.detail_ratio(),
                    page,
                    mouse.column,
                    mouse.row,
                ) == Some(library::detail::DetailHit::EnableToggle);
                let props_hover = library::props::toggle_hit_test(
                    full,
                    page.split_ratio(),
                    page.detail_ratio(),
                    page,
                    mouse.column,
                    mouse.row,
                );
                let add_hover = library::props::add_chip_hit_test(
                    full,
                    page.split_ratio(),
                    page.detail_ratio(),
                    page,
                    mouse.column,
                    mouse.row,
                );
                app.library_page_mut().set_divider_hover(hover);
                app.set_toggle_hover(if header_hover {
                    Some(ToggleId::TriggerEnabled)
                } else {
                    props_hover
                });
                app.set_props_hover(add_hover);
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            // honey: a slider drag previews live and saves on release, so
            // pointer ticks never reach the settings store or the library
            // content behind the overlay.
            if app.is_settings_overlay_open() {
                if app.settings_page().modal().is_none() {
                    app.settings_page_mut().drag_slider_to(mouse.column);
                }
                return;
            }
            if app.nav_rail().dragging() {
                app.nav_rail_mut().drag_to(area, mouse.column);
                return;
            }
            if app.palette_open() {
                return;
            }
            if home_visible(app) {
                return;
            }
            if !library_visible(app) {
                return;
            }
            if app.library_page().app_filter_menu_open() {
                let full = workspace_full(app, area);
                app.library_page_mut()
                    .drag_app_filter_divider(full, mouse.column);
                return;
            }
            if matches!(
                app.library_page().modal(),
                Some(library::LibraryModal::Create(_))
            ) {
                let full = workspace_full(app, area);
                app.library_page_mut()
                    .drag_create_divider(full, mouse.column);
                return;
            }
            if modal_open {
                return;
            }
            if app.library_page().divider_drag().is_some() {
                let full = workspace_full(app, area);
                drag_divider_to(app, full, mouse.column);
            }
            // honey: content-box drags stretch the body selection;
            // divider drags never reach here as text.
            if app.library_page().divider_drag().is_none() {
                let full = workspace_full(app, area);
                let page = app.library_page();
                let (list_ratio, props_ratio) = (page.split_ratio(), page.detail_ratio());
                app.library_page_mut().drag_content_select(
                    full,
                    list_ratio,
                    props_ratio,
                    mouse.column,
                    mouse.row,
                );
            }
        }
        MouseEventKind::Up(_) => {
            if app.is_settings_overlay_open() {
                let interaction = app.settings_page_mut().finish_slider_drag();
                apply_settings_interaction(app, interaction);
            }
            app.nav_rail_mut().end_drag();
            app.library_page_mut().set_divider_drag(None);
            app.library_page_mut().release_app_filter_divider();
            app.library_page_mut().release_create_divider();
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if app.palette_open() {
                let paused = app.daemon_paused();
                let pause_hotkey = app.settings_page().settings().pause_hotkey.clone();
                if palette::dismiss_hit(area, mouse.column, mouse.row) {
                    app.close_palette();
                } else if let Some(cursor) = app
                    .palette()
                    .and_then(|state| palette::search_hit(area, state, mouse.column, mouse.row))
                {
                    if let Some(state) = app.palette_mut() {
                        state.activate_search_at(cursor);
                    }
                } else if let Some(index) = app.palette().and_then(|state| {
                    palette::option_hit(area, state, paused, &pause_hotkey, mouse.column, mouse.row)
                }) {
                    if let Some(state) = app.palette_mut() {
                        state.select(index);
                    }
                    let command = app.palette().and_then(|state| state.command());
                    app.close_palette();
                    if let Some(command) = command {
                        execute_palette_command(app, command);
                    }
                }
                return;
            }
            if app.library_page().header_menu_open() {
                let full = workspace_full(app, area);
                let interaction =
                    app.library_page_mut()
                        .click_header_menu(full, mouse.column, mouse.row);
                apply_library_interaction(app, interaction);
                return;
            }
            if app.library_page().tags_menu_open() {
                let full = workspace_full(app, area);
                let interaction =
                    app.library_page_mut()
                        .click_tags_menu(full, mouse.column, mouse.row);
                apply_library_interaction(app, interaction);
                return;
            }
            if app.library_page().app_filter_menu_open() {
                let full = workspace_full(app, area);
                // honey: grabbing the picker divider starts a resize;
                // anywhere else clicks through to the rows.
                if app
                    .library_page_mut()
                    .grab_app_filter_divider(full, mouse.column, mouse.row)
                {
                    return;
                }
                let interaction =
                    app.library_page_mut()
                        .click_app_filter_menu(full, mouse.column, mouse.row);
                apply_library_interaction(app, interaction);
                return;
            }
            // honey: the create form owns its clicks: the shared
            // scrim/Esc-hint rule dismisses it, field rows focus
            // (picker rows open their menus), and properties rows
            // configure the draft.
            if matches!(
                app.library_page().modal(),
                Some(library::LibraryModal::Create(_))
            ) {
                let full = workspace_full(app, area);
                let popup = library::modals::create_popup(full);
                if util::overlay_dismiss_hit(popup, mouse.column, mouse.row) {
                    app.library_page_mut().clear_modal();
                    return;
                }
                if app
                    .library_page_mut()
                    .grab_create_divider(full, mouse.column, mouse.row)
                {
                    return;
                }
                if let Some(library::LibraryModal::Create(draft)) = app.library_page().modal() {
                    match library::modals::create_option_hit(draft, full, mouse.column, mouse.row) {
                        Some(library::modals::CreateHit::Field(field)) => {
                            let from_on = (field == library::LibraryCreateModalField::AutoCase)
                                .then(|| app.library_page().create_auto_case())
                                .flatten();
                            let interaction = app.library_page_mut().click_create_field(field);
                            apply_library_interaction(app, interaction);
                            if let Some(from_on) = from_on
                                && app.library_page().create_auto_case() == Some(!from_on)
                            {
                                app.start_toggle_anim(ToggleId::CreateAutoCase, from_on);
                            }
                        }
                        None => {}
                    }
                }
                return;
            }
            // honey: the delete confirm owns its clicks: option rows
            // select (a click on the cursor row confirms), while the
            // shared scrim/Esc-hint rule cancels. Other modals stay
            // keyboard-only.
            if let Some(library::LibraryModal::ConfirmDelete(_)) = app.library_page().modal() {
                let full = workspace_full(app, area);
                let popup = library::modals::delete_popup(full);
                if util::overlay_dismiss_hit(popup, mouse.column, mouse.row) {
                    app.library_page_mut().clear_modal();
                    return;
                }
                if let Some(yes) = library::modals::delete_option_hit(full, mouse.column, mouse.row)
                {
                    if app.library_page().delete_selected_yes() == Some(yes) {
                        let interaction = app.library_page_mut().confirm_delete();
                        apply_library_interaction(app, interaction);
                    } else {
                        app.library_page_mut().set_delete_selected_yes(yes);
                    }
                }
                return;
            }
            // honey: the shared scrim/Esc-hint rule dismisses the
            // settings overlay and any editor modal above it; row and
            // search clicks reuse the page hit path on the body.
            if app.is_settings_overlay_open() {
                if let Some(modal) = app.settings_page().modal() {
                    let areas = overlay_fx::FxAreas {
                        frame: area,
                        library_full: area,
                    };
                    if let Some(modal_popup) = modal.transition_rect(&areas)
                        && util::overlay_dismiss_hit(modal_popup, mouse.column, mouse.row)
                    {
                        app.settings_page_mut().clear_modal();
                        return;
                    }
                }
                let popup = settings::overlay_popup(area);
                if util::overlay_dismiss_hit(popup, mouse.column, mouse.row) {
                    app.close_settings_overlay();
                    return;
                }
                let content = settings::overlay_content(settings::overlay_body(popup));
                match settings::hit_test(content, app.settings_page(), mouse.column, mouse.row) {
                    Some(settings::SettingsHit::Row(key) | settings::SettingsHit::Toggle(key)) => {
                        app.settings_page_mut().clear_inline_edit();
                        let anchor = settings::visible_window_start(content, app.settings_page());
                        let interaction = app.settings_page_mut().click_setting(key, anchor);
                        apply_settings_interaction(app, interaction);
                    }
                    Some(settings::SettingsHit::SearchAt(cursor)) => {
                        app.settings_page_mut().clear_inline_edit();
                        app.settings_page_mut().activate_search_at(cursor);
                    }
                    Some(settings::SettingsHit::HotkeyValue(key)) => {
                        app.settings_page_mut().clear_inline_edit();
                        let anchor = settings::visible_window_start(content, app.settings_page());
                        app.settings_page_mut().select_key(key, anchor);
                        app.set_hotkey_recorder(Some(library::recorder::HotkeyRecorderState::new(
                            library::recorder::HotkeyTarget::Setting(key),
                        )));
                    }
                    Some(settings::SettingsHit::SelectValue(key)) => {
                        app.settings_page_mut().clear_inline_edit();
                        let anchor = settings::visible_window_start(content, app.settings_page());
                        app.settings_page_mut().open_editor_for_key(key, anchor);
                    }
                    Some(settings::SettingsHit::Slider(key, value, track)) => {
                        app.settings_page_mut().clear_inline_edit();
                        let anchor = settings::visible_window_start(content, app.settings_page());
                        app.settings_page_mut().select_key(key, anchor);
                        app.settings_page_mut().begin_slider_drag(key, value, track);
                    }
                    Some(settings::SettingsHit::InlineValue(key))
                        if app.settings_page().inline_key() != Some(key) =>
                    {
                        let anchor = settings::visible_window_start(content, app.settings_page());
                        app.settings_page_mut().start_inline_edit(key, Some(anchor));
                    }
                    Some(settings::SettingsHit::InlineValue(_)) => {}
                    None => {}
                }
                return;
            }
            if modal_open {
                return;
            }
            let nav_item = {
                let nav = app.nav_rail_mut();
                nav.refresh_hitboxes(area);
                nav.hit(mouse.column, mouse.row)
            };
            if let Some(item) = nav_item {
                match item {
                    NavRailItem::Home => switch_page(app, ActivePage::Home),
                    NavRailItem::Library => switch_page(app, ActivePage::Library),
                    NavRailItem::CommandPalette => app.toggle_palette(),
                    NavRailItem::Settings => {
                        let flush = app.library_page_mut().commit_edit();
                        apply_library_interaction(app, flush);
                        app.open_settings_overlay();
                    }
                }
                return;
            }
            if app
                .nav_rail_mut()
                .start_divider_drag(area, mouse.column, mouse.row)
            {
                return;
            }
            if home_visible(app) {
                if let Some(index) = app.home_page().copy_at(mouse.column, mouse.row) {
                    copy_home_event(app, index);
                } else if let Some(index) = app.home_page().event_at(mouse.column, mouse.row) {
                    app.home_page_mut().select(index);
                }
                return;
            }
            if !library_visible(app) {
                return;
            }
            let full = workspace_full(app, area);
            track_content_width(app, full);
            if grab_divider(app, full, mouse.column, mouse.row) {
                return;
            }
            match library::list::hit_test(full, app.library_page(), mouse.column, mouse.row) {
                Some(library::list::LibraryHit::Item(position)) => {
                    let anchor = library::list::window_start(full, app.library_page());
                    let interaction = app.library_page_mut().click_item(position, anchor);
                    apply_library_interaction(app, interaction);
                }
                Some(library::list::LibraryHit::SearchAt(cursor)) => {
                    let flush = app.library_page_mut().commit_edit();
                    apply_library_interaction(app, flush);
                    app.library_page_mut().activate_search_at(cursor);
                }
                // honey: toggle flips enable (committing any edit
                // first); trigger, description, and content clicks
                // start editing; everything else is read-only.
                None => {
                    let hit = {
                        let page = app.library_page();
                        library::detail::hit_test(
                            full,
                            page.split_ratio(),
                            page.detail_ratio(),
                            page,
                            mouse.column,
                            mouse.row,
                        )
                    };
                    match hit {
                        Some(library::detail::DetailHit::Button(index)) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            app.library_page_mut()
                                .open_header_menu(library::detail::dropdown_kind_for_button(index));
                        }
                        Some(library::detail::DetailHit::TriggerEditAt(cursor)) => {
                            // honey: a hotkey row edits through the
                            // recorder, not the inline text field.
                            if open_recorder_for_selected(app) {
                                return;
                            }
                            let interaction = app.library_page_mut().start_trigger_edit_at(cursor);
                            apply_library_interaction(app, interaction);
                        }
                        Some(library::detail::DetailHit::DescriptionEdit) => {
                            let interaction = app.library_page_mut().start_description_edit();
                            apply_library_interaction(app, interaction);
                        }
                        Some(library::detail::DetailHit::ContentEditAt { row, col }) => {
                            let interaction = {
                                let page = app.library_page();
                                match page.selected_index().and_then(|index| {
                                    page.item_at_filtered(index).cloned().map(|item| {
                                        (
                                            item,
                                            page.detail_scroll(),
                                            page.split_ratio(),
                                            page.detail_ratio(),
                                        )
                                    })
                                }) {
                                    Some((item, scroll, list_ratio, props_ratio)) => {
                                        let width = library::detail::content_text_width(
                                            library::detail::center_content(
                                                full,
                                                list_ratio,
                                                props_ratio,
                                            )
                                            .width,
                                        );
                                        let (srow, scol) = library::detail::content_source_cell(
                                            &item,
                                            width,
                                            scroll + row,
                                            col,
                                        );
                                        // honey: second fast press on
                                        // the cell selects the word,
                                        // otherwise the press anchors
                                        // a drag selection.
                                        let now = library::now_millis();
                                        app.library_page_mut().click_content_cell(
                                            item.id(),
                                            srow,
                                            scol,
                                            now,
                                        )
                                    }
                                    None => library::LibraryInteraction::handled(),
                                }
                            };
                            apply_library_interaction(app, interaction);
                        }
                        Some(library::detail::DetailHit::EnableToggle) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            let from_on = app.library_page().selected_enabled();
                            let interaction = app.library_page_mut().toggle_selected_enabled();
                            apply_library_interaction(app, interaction);
                            if let Some(from_on) = from_on
                                && app.library_page().selected_enabled() == Some(!from_on)
                            {
                                app.start_toggle_anim(ToggleId::TriggerEnabled, from_on);
                            }
                        }
                        None => {}
                    }
                    // honey: clicking a hotkey alias line opens the
                    // recorder on that sibling; word aliases keep the
                    // inline text edit.
                    if let Some(line) = library::props::alias_line_hit(
                        full,
                        app.library_page().split_ratio(),
                        app.library_page().detail_ratio(),
                        app.library_page(),
                        mouse.column,
                        mouse.row,
                    ) && open_recorder_for_alias(app, line)
                    {
                        return;
                    }
                    match library::props::hit_test(
                        full,
                        app.library_page().split_ratio(),
                        app.library_page().detail_ratio(),
                        app.library_page(),
                        mouse.column,
                        mouse.row,
                    ) {
                        // honey: flipping auto-case commits any
                        // open edit first, like the enable toggle.
                        Some(library::props::PropsHit::AutoCase) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            let from_on = app.library_page().selected_auto_case();
                            let interaction = app.library_page_mut().toggle_selected_auto_case();
                            apply_library_interaction(app, interaction);
                            if let Some(from_on) = from_on
                                && app.library_page().selected_auto_case() == Some(!from_on)
                            {
                                app.start_toggle_anim(ToggleId::PropAutoCase, from_on);
                            }
                        }
                        // honey: confirmation flips the same way,
                        // scoped to the selected voice alias row.
                        Some(library::props::PropsHit::Confirm) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            let from_on = app.library_page().selected_require_confirmation();
                            let interaction = app
                                .library_page_mut()
                                .toggle_selected_require_confirmation();
                            apply_library_interaction(app, interaction);
                            if let Some(from_on) = from_on
                                && app.library_page().selected_require_confirmation()
                                    == Some(!from_on)
                            {
                                app.start_toggle_anim(ToggleId::PropConfirm, from_on);
                            }
                        }
                        Some(library::props::PropsHit::Platform) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            app.library_page_mut()
                                .open_header_menu(library::HeaderMenuKind::Platform);
                        }
                        // honey: the Alias row has no separate add editor;
                        // the chip opens the existing inline alias edit.
                        Some(library::props::PropsHit::Alias) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            let interaction =
                                app.library_page_mut().start_trigger_edit_at(usize::MAX);
                            apply_library_interaction(app, interaction);
                        }
                        Some(library::props::PropsHit::UsageToggle) => {
                            app.library_page_mut().toggle_usage();
                        }
                        Some(library::props::PropsHit::Tags) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            let popup = crate::widgets::util::overlay_popup(full);
                            let body = crate::widgets::util::overlay_body(popup);
                            app.library_page_mut().open_tags_modal(body.width);
                        }
                        Some(library::props::PropsHit::Allow) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            app.library_page_mut()
                                .open_app_filter_modal(library::AppFilterSide::Allow);
                        }
                        Some(library::props::PropsHit::Block) => {
                            let flush = app.library_page_mut().commit_edit();
                            apply_library_interaction(app, flush);
                            app.library_page_mut()
                                .open_app_filter_modal(library::AppFilterSide::Block);
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
        app.settings_page_mut().clear_inline_edit();
        return;
    }

    if let Some(pending_reset) = interaction.pending_reset() {
        let key = pending_reset.key;
        let from_on = app.settings_page().setting_toggle_value(key);
        match pending_reset.apply() {
            Ok(()) => {
                refresh_settings_page(app);
                if let Some(from_on) = from_on
                    && app.settings_page().setting_toggle_value(key) == Some(!from_on)
                {
                    app.start_toggle_anim(ToggleId::Setting(key.storage_key()), from_on);
                }
            }
            Err(error) => app.settings_page_mut().set_save_error(error.to_string()),
        }
        return;
    }

    let Some(pending_save) = interaction.pending_save() else {
        return;
    };
    let key = pending_save.key;
    let from_on = app.settings_page().setting_toggle_value(key);

    match pending_save.apply() {
        Ok(()) => {
            app.settings_page_mut().clear_inline_edit();
            refresh_settings_page(app);
            apply_saved_cursor_style();
            if let Some(from_on) = from_on
                && app.settings_page().setting_toggle_value(key) == Some(!from_on)
            {
                app.start_toggle_anim(ToggleId::Setting(key.storage_key()), from_on);
            }
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

    if let Some(pending_create) = interaction.pending_create() {
        match pending_create.apply() {
            Ok(id) => {
                refresh_library_page(app);
                app.library_page_mut().select_by_id(&id);
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

    if let Some(pending_edit) = interaction.pending_edit() {
        let trigger_id = pending_edit.trigger_id.clone();
        let trigger = pending_edit.trigger.clone();
        let restore_index = pending_edit.restore_index;
        let draft = match &pending_edit.field {
            library::EditedField::Trigger(trigger) => Some((
                crate::widgets::library::state::EditTarget::Trigger,
                trigger.clone(),
            )),
            library::EditedField::Description(description) => Some((
                crate::widgets::library::state::EditTarget::Description,
                description.clone().unwrap_or_default(),
            )),
            library::EditedField::Content(body) => Some((
                crate::widgets::library::state::EditTarget::Content,
                body.clone(),
            )),
            // honey: menu picks carry no typed text, so a failed
            // confirm has nothing to restore.
            library::EditedField::InvocationType(_)
            | library::EditedField::ActionType(_)
            | library::EditedField::Interpreter(_)
            | library::EditedField::Behavior(_)
            | library::EditedField::AutoCase(_)
            | library::EditedField::RequireConfirmation(_)
            | library::EditedField::TargetOs(_)
            | library::EditedField::Tags(_)
            | library::EditedField::OnlyApps(_)
            | library::EditedField::ExceptApps(_) => None,
        };
        // honey: the tags builder lives on across live toggles; the
        // menu reopens from refreshed rows after each write.
        let reopen_tags = matches!(pending_edit.field, library::EditedField::Tags(_));
        let reopen_apps = matches!(
            pending_edit.field,
            library::EditedField::OnlyApps(_) | library::EditedField::ExceptApps(_)
        );
        match pending_edit.apply() {
            Ok(()) => {
                refresh_library_page(app);
                // honey: a vanished trigger closes menus with it; the
                // tags menu otherwise reseeds from refreshed rows. A
                // renamed invocation reselects by its new text so the
                // cursor stays on the edited row.
                let select_trigger = match &pending_edit.field {
                    library::EditedField::Trigger(trigger) => trigger.clone(),
                    _ => trigger,
                };
                if !app
                    .library_page_mut()
                    .select_row(&trigger_id, &select_trigger)
                {
                    app.library_page_mut().select_after_delete(restore_index);
                } else if reopen_tags {
                    app.library_page_mut().sync_tags_modal(&trigger_id);
                } else if reopen_apps {
                    app.library_page_mut().sync_app_filter_modal(&trigger_id);
                }
            }
            // honey: typed text is never lost; the edit reopens on the
            // same row with the draft intact.
            Err(error) => {
                app.library_page_mut().set_save_error(error.to_string());
                if let Some((target, text)) = draft {
                    app.library_page_mut()
                        .restore_edit(&trigger_id, target, text);
                }
            }
        }
    }
}

/// Layout worth writing, or `None` when nothing changed. A drag still in
/// flight is skipped unless `force`, so only settled widths persist.
fn layout_to_save(app: &App, force: bool) -> Option<TuiLayout> {
    if app.is_dragging_layout() && !force {
        return None;
    }
    app.layout_needs_save().then(|| app.layout())
}

/// Writes the pane layout only when it changed. A failed write stays dirty
/// so the next release or quit retries, and the TUI keeps running.
fn persist_layout_if_needed(app: &mut App, force: bool) {
    let Some(layout) = layout_to_save(app, force) else {
        return;
    };
    match layout_store::save(layout) {
        Ok(()) => app.mark_layout_saved(),
        Err(error) => error!(error = %error, "Failed to save the TUI pane layout"),
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

/// Copies one activity event's stored text; silent scripts have nothing to
/// copy, so the toast only appears when something was written.
fn copy_home_event(app: &mut App, index: usize) {
    let Some(event) = app.home_page().stats().recent_activity.get(index) else {
        return;
    };
    let body = home::event_body(event).trim();
    if body.is_empty() {
        return;
    }
    library::actions::write_clipboard(body);
    app.set_notification("Copied to clipboard".to_string());
}

fn copy_selected_home_event(app: &mut App) {
    if let Some(index) = app.home_page().selected() {
        copy_home_event(app, index);
    }
}

fn refresh_home_page(app: &mut App) {
    match taurine_core::db::init::setup()
        .and_then(|conn| taurine_core::stats::load_home_stats(&conn))
    {
        Ok(stats) => {
            app.home_page_mut().replace_stats(stats);
            app.home_page_mut().mark_refreshed(library::now_millis());
        }
        Err(error) => {
            error!(error = %error, "Failed to refresh TUI home state");
            app.home_page_mut().set_load_error(error.to_string());
            // honey: back off after a failed load so a broken DB is not
            // reopened on every event tick.
            app.home_page_mut().mark_refreshed(library::now_millis());
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

/// Startup selection from the real terminal size: mirror the draw chain
/// down to the list area so the highlight lands on the bottom row of the
/// first window (topmost entries visible, cursor above the search bar).
fn select_first_visible_library_row(app: &mut App, terminal: &mut TerminalGuard) {
    let Ok(size) = terminal.terminal.size() else {
        return;
    };
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let full = workspace_full(app, area);
    let state = app.library_page();
    let content = library::left_content(full, state.split_ratio(), state.detail_ratio());
    let (list_area, _) = library::content_sections(
        content,
        false,
        state.search_field(),
        state.is_search_active(),
    );
    let capacity = widgets::util::visible_library_item_capacity(list_area.height);
    app.library_page_mut().select_first_window_bottom(capacity);
}

/// Settings startup mirrors the library list: open at the top of the
/// settings, with the highlight on the last row above the search bar.
fn select_first_visible_settings_row(app: &mut App, terminal: &mut TerminalGuard) {
    let Ok(size) = terminal.terminal.size() else {
        return;
    };
    let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
    let popup = settings::overlay_popup(area);
    let content = settings::overlay_content(settings::overlay_body(popup));
    settings::select_first_window_bottom(content, app.settings_page_mut());
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

        // honey: tab reads Taurine for the session; Drop restores it.
        crate::terminal_title::acquire();

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
    crate::terminal_title::release();
}

#[cfg(test)]
mod tests {
    #[test]
    fn divider_glyphs_join_the_content_rule() {
        use super::library::DividerSide;
        use super::panes::divider_glyph;

        assert_eq!(divider_glyph(DividerSide::List, 7, Some(7)), "├");
        assert_eq!(divider_glyph(DividerSide::Props, 7, Some(7)), "┤");
        assert_eq!(divider_glyph(DividerSide::List, 8, Some(7)), "│");
    }

    #[test]
    fn dim_frame_scales_rgb_and_marks_non_rgb() {
        use ratatui::{
            Terminal,
            backend::TestBackend,
            style::{Color, Modifier},
        };

        let backend = TestBackend::new(2, 1);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.buffer_mut().content[0].set_fg(Color::Rgb(100, 100, 100));
                frame.buffer_mut().content[0].set_bg(Color::Rgb(200, 200, 200));
                frame.buffer_mut().content[1].set_fg(Color::Reset);
                super::dim_frame(frame);
                let dimmed = &frame.buffer_mut().content[1];
                assert_eq!(dimmed.fg, Color::Reset);
                assert!(dimmed.modifier.contains(Modifier::DIM));
            })
            .expect("test draw");
        let buffer = terminal.backend().buffer().clone();
        assert_eq!(buffer.content[0].fg, Color::Rgb(40, 40, 40));
        assert_eq!(buffer.content[0].bg, Color::Rgb(80, 80, 80));
    }

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

    /// Workspace tests drive the library panes; the app itself opens on
    /// Home, so tests that exercise library flows land there first.
    fn library_app() -> App {
        let mut app = App::default();
        app.nav_rail_mut().set_active_page(ActivePage::Library);
        app
    }

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
        let mut app = library_app();

        handle_tui_key_event(&mut app, plain_key('/'));
        handle_tui_key_event(&mut app, plain_key('q'));

        assert!(!app.should_quit());
        assert_eq!(app.library_page().search_query(), "q");
    }

    #[test]
    fn pressing_one_leaves_library_search_inactive() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, plain_key('1'));

        assert!(!app.is_settings_overlay_open());
        assert!(!app.library_page().is_search_active());
        assert_eq!(app.library_page().search_query(), "");
    }

    #[test]
    fn pressing_two_does_not_open_settings() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, plain_key('2'));

        assert!(!app.is_settings_overlay_open());
        assert!(!app.settings_page().is_search_active());
        assert_eq!(app.settings_page().search_query(), "");
    }

    #[test]
    fn typing_one_while_library_search_is_active_types_into_search() {
        let mut app = library_app();

        handle_tui_key_event(&mut app, plain_key('/'));
        handle_tui_key_event(&mut app, plain_key('1'));

        assert!(!app.is_settings_overlay_open());
        assert_eq!(app.library_page().search_query(), "1");
    }

    #[test]
    fn typing_q_while_library_modal_is_open_does_not_quit() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, plain_key('q'));

        assert!(!app.should_quit());
        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn slash_goes_to_modal_while_library_modal_is_open() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, plain_key('/'));

        assert!(app.library_page().is_modal_open());
        assert_eq!(app.library_page().search_query(), "");
    }

    #[test]
    fn typing_q_while_library_delete_confirmation_is_open_does_not_quit() {
        let mut app = library_app();
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::single(TriggerListItem {
                id: "test".to_string(),
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
        let mut app = library_app();
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::single(TriggerListItem {
                id: "test".to_string(),
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
    fn escape_closes_library_modal() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

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

    fn ctrl_comma() -> KeyEvent {
        KeyEvent::new(KeyCode::Char(','), KeyModifiers::CONTROL)
    }

    const TEST_AREA: ratatui::layout::Rect = ratatui::layout::Rect::new(0, 0, 100, 30);

    #[test]
    fn ctrl_comma_toggles_settings_overlay() {
        let mut app = library_app();
        assert!(!app.is_settings_overlay_open());

        handle_tui_key_event(&mut app, ctrl_comma());
        assert!(app.is_settings_overlay_open());

        handle_tui_key_event(&mut app, ctrl_comma());
        assert!(!app.is_settings_overlay_open());
    }

    #[test]
    fn ctrl_comma_while_library_modal_is_open_waits() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, ctrl_comma());

        assert!(!app.is_settings_overlay_open());
        assert!(app.library_page().is_modal_open());
    }

    fn ctrl_p() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL)
    }

    fn esc() -> KeyEvent {
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
    }

    fn down() -> KeyEvent {
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)
    }

    fn enter() -> KeyEvent {
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
    }

    #[test]
    fn ctrl_p_toggles_palette() {
        let mut app = library_app();
        assert!(!app.palette_open());

        handle_tui_key_event(&mut app, ctrl_p());
        assert!(app.palette_open());

        handle_tui_key_event(&mut app, ctrl_p());
        assert!(!app.palette_open());
    }

    #[test]
    fn ctrl_p_opens_over_settings_overlay() {
        let mut app = library_app();
        app.open_settings_overlay();

        handle_tui_key_event(&mut app, ctrl_p());
        assert!(app.palette_open());

        handle_tui_key_event(&mut app, esc());
        assert!(!app.palette_open());
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn palette_esc_closes_without_quitting() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());

        handle_tui_key_event(&mut app, esc());

        assert!(!app.palette_open());
        assert!(!app.should_quit());
        assert!(!app.is_closing());
    }

    #[test]
    fn palette_enter_toggles_left_panel() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());

        handle_tui_key_event(&mut app, enter());

        assert!(!app.palette_open());
        assert_eq!(app.library_page().split_ratio(), library::MIN_SPLIT_RATIO);

        handle_tui_key_event(&mut app, ctrl_p());
        handle_tui_key_event(&mut app, enter());
        assert_eq!(
            app.library_page().split_ratio(),
            library::DEFAULT_SPLIT_RATIO
        );
    }

    #[test]
    fn palette_enter_toggles_properties_pane() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());
        handle_tui_key_event(&mut app, down());

        handle_tui_key_event(&mut app, enter());

        assert_eq!(app.library_page().detail_ratio(), library::MIN_DETAIL_RATIO);
    }

    #[test]
    fn palette_reset_layout_restores_defaults() {
        let mut app = library_app();
        app.library_page_mut().set_split_ratio(0.6);
        app.library_page_mut().set_detail_ratio(0.6);
        handle_tui_key_event(&mut app, ctrl_p());
        for _ in 0..3 {
            handle_tui_key_event(&mut app, down());
        }

        handle_tui_key_event(&mut app, enter());

        assert_eq!(
            app.library_page().split_ratio(),
            library::DEFAULT_SPLIT_RATIO
        );
        assert_eq!(
            app.library_page().detail_ratio(),
            library::DEFAULT_DETAIL_RATIO
        );
    }

    #[test]
    fn palette_new_trigger_opens_create_modal() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());
        for _ in 0..4 {
            handle_tui_key_event(&mut app, down());
        }

        handle_tui_key_event(&mut app, enter());

        assert!(!app.palette_open());
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(_))
        ));
    }

    #[test]
    fn palette_import_opens_import_modal() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());
        for _ in 0..5 {
            handle_tui_key_event(&mut app, down());
        }
        handle_tui_key_event(&mut app, enter());
        assert!(!app.palette_open());
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Import(_))
        ));
    }

    #[test]
    fn palette_export_opens_export_modal() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());
        for _ in 0..6 {
            handle_tui_key_event(&mut app, down());
        }
        handle_tui_key_event(&mut app, enter());
        assert!(!app.palette_open());
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Export(_))
        ));
    }

    #[test]
    fn palette_open_settings_opens_overlay() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());
        for _ in 0..7 {
            handle_tui_key_event(&mut app, down());
        }

        handle_tui_key_event(&mut app, enter());

        assert!(!app.palette_open());
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn palette_quit_starts_close_animation() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());
        for _ in 0..10 {
            handle_tui_key_event(&mut app, down());
        }

        handle_tui_key_event(&mut app, enter());

        assert!(!app.palette_open());
        assert!(app.is_closing());
        assert!(!app.should_quit());
    }

    #[test]
    fn clicking_palette_row_runs_command() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());
        let popup = palette::popup(TEST_AREA);
        let first_row = popup.y + 4;

        handle_tui_mouse_event(&mut app, left_click(popup.x + 3, first_row), TEST_AREA);

        assert!(!app.palette_open());
        assert_eq!(app.library_page().split_ratio(), library::MIN_SPLIT_RATIO);
    }

    #[test]
    fn clicking_palette_scrim_closes_it() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_p());

        handle_tui_mouse_event(&mut app, left_click(0, 0), TEST_AREA);

        assert!(!app.palette_open());
    }

    #[test]
    fn esc_closes_settings_overlay_after_search() {
        let mut app = library_app();
        app.open_settings_overlay();
        handle_tui_key_event(&mut app, plain_key('/'));
        assert!(app.settings_page().is_search_active());

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.is_settings_overlay_open());
        assert!(!app.settings_page().is_search_active());

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.is_settings_overlay_open());
    }

    #[test]
    fn clicking_outside_settings_overlay_closes() {
        let mut app = library_app();
        app.open_settings_overlay();

        handle_tui_mouse_event(&mut app, left_click(2, 15), TEST_AREA);

        assert!(!app.is_settings_overlay_open());
    }

    #[test]
    fn settings_overlay_takes_ninety_percent_of_the_terminal() {
        assert_eq!(
            settings::overlay_popup(TEST_AREA),
            ratatui::layout::Rect::new(5, 1, 90, 27)
        );
    }

    #[test]
    fn settings_editor_modal_opens_inside_overlay() {
        use crate::widgets::settings::state::{EditorKind, SettingKeyMeta};

        let mut app = library_app();
        app.open_settings_overlay();
        let position = app
            .settings_page()
            .visible_keys()
            .iter()
            .position(|key| key.editor_kind() != EditorKind::Toggle)
            .expect("editor key");
        app.settings_page_mut().selected = position;

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.is_settings_overlay_open());
        assert!(app.settings_page().is_modal_open());

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.is_settings_overlay_open());
        assert!(!app.settings_page().is_modal_open());
    }

    #[test]
    fn nested_settings_modal_dims_the_settings_overlay() {
        use crate::widgets::settings::state::{EditorKind, SettingKeyMeta};

        let mut app = library_app();
        app.open_settings_overlay();
        let position = app
            .settings_page()
            .visible_keys()
            .iter()
            .position(|key| key.editor_kind() != EditorKind::Toggle)
            .expect("editor key");
        app.settings_page_mut().selected = position;

        let popup = settings::overlay_popup(TEST_AREA);
        let sample = (popup.x + 1, popup.y + 1);
        let render_bg = |app: &mut App| {
            let backend = ratatui::backend::TestBackend::new(TEST_AREA.width, TEST_AREA.height);
            let mut terminal = ratatui::Terminal::new(backend).expect("terminal");
            terminal
                .draw(|frame| render_app(frame, app, TEST_AREA))
                .expect("draw");
            terminal.backend().buffer()[(sample.0, sample.1)].bg
        };

        let at_rest = render_bg(&mut app);
        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.settings_page().is_modal_open());
        let nested = render_bg(&mut app);

        assert_ne!(nested, at_rest);
        if let (ratatui::style::Color::Rgb(r, g, b), ratatui::style::Color::Rgb(dr, dg, db)) =
            (at_rest, nested)
        {
            assert!(
                u32::from(dr) + u32::from(dg) + u32::from(db)
                    < u32::from(r) + u32::from(g) + u32::from(b),
                "{at_rest:?} -> {nested:?}"
            );
        }
    }

    #[test]
    fn clicking_voice_model_value_opens_the_tier_picker() {
        use crate::widgets::settings::state::SettingKey;

        let mut app = library_app();
        app.open_settings_overlay();
        let position = app
            .settings_page()
            .visible_keys()
            .iter()
            .position(|key| *key == SettingKey::VoiceModel)
            .expect("voice model key");
        app.settings_page_mut().selected = position;
        let popup = settings::overlay_popup(TEST_AREA);
        let content = settings::overlay_content(settings::overlay_body(popup));
        let mut target = None;
        for row in 0..TEST_AREA.height {
            for column in 0..TEST_AREA.width {
                if let Some(settings::SettingsHit::SelectValue(found)) =
                    settings::hit_test(content, app.settings_page(), column, row)
                    && found == SettingKey::VoiceModel
                {
                    target = Some((found, column, row));
                    break;
                }
            }
            if target.is_some() {
                break;
            }
        }
        let (key, column, row) = target.expect("select setting value");

        handle_tui_mouse_event(&mut app, left_click(column, row), TEST_AREA);

        let Some(settings::SettingsModal::Select(modal)) = app.settings_page().modal() else {
            panic!("picker should open");
        };
        assert_eq!(modal.key(), key);
        assert_eq!(modal.options(), ["Best", "Balanced", "Fast"]);
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn clicking_an_inline_setting_field_starts_editing() {
        use crate::widgets::settings::state::SettingKeyMeta;

        let mut app = library_app();
        app.open_settings_overlay();
        // honey: anchor the window on a known short field before scanning
        // the hit path; the first rows that fit may all be toggles.
        let position = app
            .settings_page()
            .visible_keys()
            .iter()
            .position(|key| key.inline_edit())
            .expect("inline key");
        app.settings_page_mut().selected = position;
        let popup = settings::overlay_popup(TEST_AREA);
        let content = settings::overlay_content(settings::overlay_body(popup));
        let mut target = None;
        for row in 0..TEST_AREA.height {
            for column in 0..TEST_AREA.width {
                if let Some(settings::SettingsHit::InlineValue(key)) =
                    settings::hit_test(content, app.settings_page(), column, row)
                {
                    target = Some((key, column, row));
                    break;
                }
            }
            if target.is_some() {
                break;
            }
        }
        let (key, column, row) = target.expect("inline setting field");

        handle_tui_mouse_event(&mut app, left_click(column, row), TEST_AREA);

        assert!(app.is_settings_overlay_open());
        assert_eq!(app.settings_page().inline_key(), Some(key));
        let before = app
            .settings_page()
            .inline_edit()
            .expect("inline editor")
            .field()
            .text()
            .to_string();

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE),
        );
        let after = app
            .settings_page()
            .inline_edit()
            .expect("inline editor")
            .field()
            .text()
            .to_string();
        assert_eq!(after, format!("{before}7"));

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.settings_page().inline_edit().is_none());
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn clicking_hotkey_setting_keycaps_opens_the_recorder() {
        let mut app = library_app();
        app.open_settings_overlay();
        let popup = settings::overlay_popup(TEST_AREA);
        let content = settings::overlay_content(settings::overlay_body(popup));
        let mut target = None;
        for row in 0..TEST_AREA.height {
            for column in 0..TEST_AREA.width {
                if let Some(settings::SettingsHit::HotkeyValue(key)) =
                    settings::hit_test(content, app.settings_page(), column, row)
                {
                    target = Some((key, column, row));
                    break;
                }
            }
            if target.is_some() {
                break;
            }
        }
        let (key, column, row) = target.expect("hotkey keycap");

        handle_tui_mouse_event(&mut app, left_click(column, row), TEST_AREA);

        let recorder = app.hotkey_recorder().expect("recorder open");
        assert_eq!(
            recorder.target,
            library::recorder::HotkeyTarget::Setting(key)
        );
        assert_eq!(app.settings_page().selected_key(), key);
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn clicking_settings_search_bar_focuses_search() {
        let mut app = library_app();
        app.open_settings_overlay();
        assert!(!app.settings_page().is_search_active());

        handle_tui_mouse_event(&mut app, left_click(30, 24), TEST_AREA);

        assert!(app.settings_page().is_search_active());
        assert_eq!(app.settings_page().search_query(), "");
    }

    #[test]
    fn clicking_settings_row_selects_without_activating() {
        let mut app = library_app();
        app.open_settings_overlay();
        // honey: park selection elsewhere so the scanned row takes the
        // first-click select path instead of the toggle path.
        let last = app.settings_page().visible_keys().len().saturating_sub(1);
        app.settings_page_mut().selected = last;
        // honey: scan the real hit path for the first row instead of
        // hard-coding a cell that row heights could shift.
        let popup = settings::overlay_popup(TEST_AREA);
        let content = settings::overlay_content(settings::overlay_body(popup));
        let mut target = None;
        for row in 0..TEST_AREA.height {
            for column in 0..TEST_AREA.width {
                if let Some(settings::SettingsHit::Row(key)) =
                    settings::hit_test(content, app.settings_page(), column, row)
                {
                    target = Some((key, column, row));
                    break;
                }
            }
            if target.is_some() {
                break;
            }
        }
        let (key, column, row) = target.expect("settings row");
        let position = app
            .settings_page()
            .visible_keys()
            .iter()
            .position(|k| *k == key)
            .expect("visible key");

        handle_tui_mouse_event(&mut app, left_click(column, row), TEST_AREA);

        assert_eq!(app.settings_page().selected_index(), position);
        assert!(app.settings_page().modal().is_none());
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn clicking_library_row_selects_without_opening() {
        let mut app = library_app();
        app.library_page_mut().replace_items(vec![
            LibraryTrigger::single(TriggerListItem {
                id: "mouse-aaa".to_string(),
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

        handle_tui_mouse_event(&mut app, left_click(10, 22), TEST_AREA);

        assert_eq!(app.library_page().selected_index(), Some(1));
        assert!(!app.library_page().is_modal_open());
    }

    #[test]
    fn collapsed_left_panel_leaves_one_boundary_line() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = library_app();
        app.library_page_mut()
            .toggle_panel(library::DividerSide::List);
        let full = workspace_full(&app, TEST_AREA);
        let split = library::split_panes(
            full,
            app.library_page().split_ratio(),
            app.library_page().detail_ratio(),
        );
        assert_eq!(split.list.width, 0);
        assert_eq!(split.center.x, full.x);

        let backend = TestBackend::new(TEST_AREA.width, TEST_AREA.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_app(frame, &mut app, TEST_AREA))
            .expect("test draw");
        let buffer = terminal.backend().buffer();
        let rail_divider = full.x.saturating_sub(1);
        assert_eq!(
            buffer[(rail_divider, 1)].symbol(),
            "│",
            "the nav rail boundary stays"
        );
        assert_ne!(
            buffer[(full.x, 1)].symbol(),
            "│",
            "the collapsed pane leaves no second divider"
        );
    }

    #[test]
    fn pressing_divider_starts_drag_without_selecting() {
        let mut app = library_app();
        let full = workspace_full(&app, TEST_AREA);
        let column = library::divider_columns(
            full,
            app.library_page().split_ratio(),
            app.library_page().detail_ratio(),
        )[0];

        handle_tui_mouse_event(&mut app, left_click(column, 10), TEST_AREA);

        assert_eq!(
            app.library_page().divider_drag(),
            Some(library::DividerSide::List)
        );
        assert_eq!(app.library_page().split_ratio(), 2.0 / 7.0);
        assert!(!app.library_page().is_modal_open());
    }

    #[test]
    fn dragging_divider_moves_split_and_release_ends_it() {
        let mut app = library_app();
        let full = workspace_full(&app, TEST_AREA);
        let column = library::divider_columns(
            full,
            app.library_page().split_ratio(),
            app.library_page().detail_ratio(),
        )[0];
        let target = column + 20;
        let expected = library::split_ratio_for_column(full, target);
        handle_tui_mouse_event(&mut app, left_click(column, 10), TEST_AREA);

        handle_tui_mouse_event(
            &mut app,
            mouse_at(target, 10, MouseEventKind::Drag(MouseButton::Left)),
            TEST_AREA,
        );

        let ratio = app.library_page().split_ratio();
        assert!((ratio - expected).abs() < 0.01);
        assert!(app.library_page().divider_drag().is_some());

        handle_tui_mouse_event(
            &mut app,
            mouse_at(target, 10, MouseEventKind::Up(MouseButton::Left)),
            TEST_AREA,
        );

        assert!(app.library_page().divider_drag().is_none());
        assert!((app.library_page().split_ratio() - expected).abs() < 0.01);
    }

    /// App parked mid-drag with a split that differs from the saved one.
    fn dragging_app(ratio: f32) -> App {
        let mut app = library_app();
        app.library_page_mut()
            .set_divider_drag(Some(library::DividerSide::List));
        app.library_page_mut().set_split_ratio(ratio);
        app
    }

    #[test]
    fn layout_to_save_skips_while_main_drag_is_active() {
        let app = dragging_app(0.5);

        assert!(app.library_page().is_dragging_layout());
        assert_eq!(layout_to_save(&app, false), None);
    }

    #[test]
    fn layout_to_save_returns_changed_layout_after_main_release() {
        let mut app = dragging_app(0.5);
        app.library_page_mut().set_divider_drag(None);

        assert_eq!(layout_to_save(&app, false), Some(app.layout()));
    }

    #[test]
    fn layout_to_save_returns_none_when_unchanged() {
        let app = library_app();

        assert_eq!(layout_to_save(&app, false), None);
    }

    #[test]
    fn layout_to_save_force_returns_layout_while_dragging() {
        let app = dragging_app(0.5);

        assert_eq!(layout_to_save(&app, true), Some(app.layout()));
    }

    #[test]
    fn layout_to_save_includes_create_split() {
        let mut app = library_app();
        app.library_page_mut().set_create_split_ratio(0.7);

        let layout = layout_to_save(&app, false).expect("changed layout");

        assert_eq!(layout.create_split, 0.7);
    }

    fn create_dragging_app() -> App {
        let mut app = library_app();
        app.library_page_mut().open_create_modal();
        let body = library::modals::create_body(TEST_AREA);
        let split = match app.library_page().modal() {
            Some(library::LibraryModal::Create(draft)) => draft.split(),
            other => panic!("expected create modal, got {other:?}"),
        };
        let (_, divider, _) = library::modals::create_split(body, split);
        assert!(app.library_page_mut().grab_create_divider(
            TEST_AREA,
            divider,
            body.y.saturating_add(2)
        ));
        app.library_page_mut()
            .drag_create_divider(TEST_AREA, divider.saturating_add(20));
        app
    }

    #[test]
    fn layout_to_save_skips_while_create_drag_is_active() {
        let app = create_dragging_app();

        assert!(app.library_page().is_dragging_layout());
        assert_eq!(layout_to_save(&app, false), None);
        assert!(layout_to_save(&app, true).is_some());
    }

    #[test]
    fn layout_to_save_returns_changed_create_split_after_release() {
        let mut app = create_dragging_app();
        app.library_page_mut().release_create_divider();

        let layout = layout_to_save(&app, false).expect("changed layout");

        assert!(layout.create_split > 0.5);
        assert_eq!(layout.create_split, app.library_page().create_split_ratio());
    }

    #[test]
    fn hovering_empty_property_chip_sets_add_hover() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        let full = workspace_full(&app, TEST_AREA);
        let page = app.library_page();
        let content = library::props::props_content(full, page.split_ratio(), page.detail_ratio());
        let chip_x = content.x + content.width - 1;

        handle_tui_mouse_event(
            &mut app,
            mouse_at(chip_x, content.y + 4, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert_eq!(app.props_hover(), Some(library::props::PropsHit::Allow));

        handle_tui_mouse_event(
            &mut app,
            mouse_at(content.x, content.y + 4, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert_eq!(app.props_hover(), None);
    }

    #[test]
    fn clicking_alias_add_chip_starts_inline_edit() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        let full = workspace_full(&app, TEST_AREA);
        let page = app.library_page();
        let content = library::props::props_content(full, page.split_ratio(), page.detail_ratio());
        let chip_x = content.x + content.width - 1;

        handle_tui_mouse_event(
            &mut app,
            mouse_at(
                chip_x,
                content.y + 8,
                MouseEventKind::Down(MouseButton::Left),
            ),
            TEST_AREA,
        );

        assert!(app.library_page().trigger_edit().is_some());
    }

    #[test]
    fn clicking_a_hotkey_header_opens_the_recorder() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        let full = workspace_full(&app, TEST_AREA);
        let page = app.library_page();
        let content =
            library::detail::center_content(full, page.split_ratio(), page.detail_ratio());

        handle_tui_mouse_event(&mut app, left_click(content.x + 1, content.y), TEST_AREA);

        let recorder = app.hotkey_recorder().expect("recorder open");
        assert_eq!(
            recorder.target,
            library::recorder::HotkeyTarget::PrimaryTrigger
        );
        assert!(recorder.modifiers.is_empty());
        assert_eq!(recorder.main_key, None);
        assert!(app.library_page().trigger_edit().is_none());
    }

    #[test]
    fn clicking_a_hotkey_alias_line_opens_the_recorder_on_that_sibling() {
        let mut app = library_app();
        app.library_page_mut()
            .replace_items(vec![LibraryTrigger::single(TriggerListItem {
                id: "test".to_string(),
                description: None,
                invocations: vec![
                    hotkey_alias("test", "ctrl+k"),
                    hotkey_alias("test", "alt+t"),
                ],
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
        let full = workspace_full(&app, TEST_AREA);
        let page = app.library_page();
        let content = library::props::props_content(full, page.split_ratio(), page.detail_ratio());
        let row = (content.y..content.y + content.height)
            .find(|row| {
                library::props::alias_line_hit(
                    full,
                    page.split_ratio(),
                    page.detail_ratio(),
                    page,
                    content.x,
                    *row,
                ) == Some(0)
            })
            .expect("alias line");

        handle_tui_mouse_event(&mut app, left_click(content.x + 1, row), TEST_AREA);

        let recorder = app.hotkey_recorder().expect("recorder open");
        assert_eq!(
            recorder.target,
            library::recorder::HotkeyTarget::AliasIndex(0)
        );
        assert_eq!(recorder.chord(), None);
    }

    #[test]
    fn recording_captures_the_global_shortcuts() {
        let mut app = library_app();
        app.set_hotkey_recorder(Some(library::recorder::HotkeyRecorderState::new(
            library::recorder::HotkeyTarget::PrimaryTrigger,
        )));

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert!(!app.should_quit());
        assert_eq!(
            app.hotkey_recorder().and_then(|state| state.chord()),
            Some("ctrl+c".to_string())
        );

        handle_tui_key_event(
            &mut app,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
        );
        assert!(!app.palette_open());
        assert_eq!(
            app.hotkey_recorder().and_then(|state| state.chord()),
            Some("ctrl+p".to_string())
        );
    }

    #[test]
    fn esc_discards_the_recording() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.set_hotkey_recorder(Some(library::recorder::HotkeyRecorderState::new(
            library::recorder::HotkeyTarget::PrimaryTrigger,
        )));

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(app.hotkey_recorder().is_none());
        assert_eq!(
            app.library_page()
                .selected_index()
                .and_then(|index| app.library_page().item_at_filtered(index))
                .map(|item| item.trigger().to_string()),
            Some("alt+t".to_string())
        );
    }

    #[test]
    fn recording_ignores_clicks_outside_the_popup() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.set_hotkey_recorder(Some(library::recorder::HotkeyRecorderState::new(
            library::recorder::HotkeyTarget::PrimaryTrigger,
        )));

        handle_tui_mouse_event(&mut app, left_click(0, 0), TEST_AREA);

        assert!(app.hotkey_recorder().is_some());
    }

    #[test]
    fn hovering_divider_sets_hover_and_leaving_clears() {
        let mut app = library_app();
        let full = workspace_full(&app, TEST_AREA);
        let column = library::divider_columns(
            full,
            app.library_page().split_ratio(),
            app.library_page().detail_ratio(),
        )[0];

        handle_tui_mouse_event(
            &mut app,
            mouse_at(column, 10, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert!(app.library_page().divider_hover().is_some());

        handle_tui_mouse_event(&mut app, mouse_at(10, 10, MouseEventKind::Moved), TEST_AREA);
        assert!(app.library_page().divider_hover().is_none());
    }

    fn nav_button(app: &App, item: NavRailItem) -> ratatui::layout::Rect {
        nav_rail::layout(TEST_AREA, app.nav_rail().collapsed)
            .buttons
            .into_iter()
            .find(|(candidate, _)| *candidate == item)
            .map(|(_, rect)| rect)
            .unwrap_or_else(|| panic!("{item:?} missing"))
    }

    #[test]
    fn nav_rail_switches_pages_and_opens_the_palette() {
        let mut app = library_app();
        let home = nav_button(&app, NavRailItem::Home);
        handle_tui_mouse_event(&mut app, left_click(home.x + 2, home.y + 1), TEST_AREA);
        assert_eq!(app.nav_rail().active_page, ActivePage::Home);

        let library = nav_button(&app, NavRailItem::Library);
        handle_tui_mouse_event(
            &mut app,
            left_click(library.x + 2, library.y + 1),
            TEST_AREA,
        );
        assert_eq!(app.nav_rail().active_page, ActivePage::Library);

        let palette = nav_button(&app, NavRailItem::CommandPalette);
        handle_tui_mouse_event(
            &mut app,
            left_click(palette.x + 2, palette.y + 1),
            TEST_AREA,
        );
        assert!(app.palette_open());
    }

    #[test]
    fn home_page_renders_the_dashboard() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = library_app();
        let home = nav_button(&app, NavRailItem::Home);
        handle_tui_mouse_event(&mut app, left_click(home.x + 2, home.y + 1), TEST_AREA);

        let render = |app: &mut App| {
            let backend = TestBackend::new(TEST_AREA.width, TEST_AREA.height);
            let mut terminal = Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| render_app(frame, app, TEST_AREA))
                .expect("test draw");
            terminal
        };

        let terminal = render(&mut app);
        let full = workspace_full(&app, TEST_AREA);
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in full.top()..full.bottom() {
            for x in full.left()..full.right() {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        assert!(text.contains("TIME SAVED"), "home cards missing");
        assert!(text.contains("Recent Activity"), "home activity missing");
        assert!(
            text.contains("No activity yet."),
            "home empty state missing"
        );
        assert!(
            !text.contains("Most Used Triggers"),
            "usage section removed"
        );
        assert!(!text.contains('│'), "home is one column with no divider");

        let library = nav_button(&app, NavRailItem::Library);
        handle_tui_mouse_event(
            &mut app,
            left_click(library.x + 2, library.y + 1),
            TEST_AREA,
        );
        let terminal = render(&mut app);
        let buffer = terminal.backend().buffer();
        assert!(
            (full.top()..full.bottom())
                .any(|y| (full.left()..full.right()).any(|x| buffer[(x, y)].symbol() != " ")),
            "library content did not come back"
        );
    }

    #[test]
    fn home_page_has_no_divider() {
        let mut app = library_app();
        let home = nav_button(&app, NavRailItem::Home);
        handle_tui_mouse_event(&mut app, left_click(home.x + 2, home.y + 1), TEST_AREA);

        let full = workspace_full(&app, TEST_AREA);
        let middle = full.x + full.width / 2;
        let row = full.y + 5;
        handle_tui_mouse_event(&mut app, left_click(middle, row), TEST_AREA);
        handle_tui_mouse_event(
            &mut app,
            mouse_at(
                middle.saturating_sub(8),
                row,
                MouseEventKind::Drag(MouseButton::Left),
            ),
            TEST_AREA,
        );

        assert!(app.library_page().divider_drag().is_none());
        assert!(!app.layout_needs_save(), "home has no draggable pane");
    }

    #[test]
    fn home_page_ignores_library_keys() {
        let mut app = library_app();
        let home = nav_button(&app, NavRailItem::Home);
        handle_tui_mouse_event(&mut app, left_click(home.x + 2, home.y + 1), TEST_AREA);

        handle_tui_key_event(&mut app, plain_key('n'));

        assert!(!app.library_page().is_search_active());
        assert!(app.library_page().search_query().is_empty());
    }

    fn home_app_with_events(count: usize) -> App {
        use taurine_core::db::crud::ActivityCategory;
        use taurine_core::stats::{HomeStats, RecentActivity};

        let mut app = library_app();
        let home = nav_button(&app, NavRailItem::Home);
        handle_tui_mouse_event(&mut app, left_click(home.x + 2, home.y + 1), TEST_AREA);
        let now = library::now_millis() as i64;
        app.home_page_mut().replace_stats(HomeStats {
            recent_activity: (0..count)
                .map(|index| RecentActivity {
                    timestamp: now - index as i64 * 1_000,
                    category: ActivityCategory::Text,
                    trigger: format!("t{index}"),
                    detail: String::new(),
                    content: Some(format!("body {index}")),
                    app: String::new(),
                    word_count: 0,
                })
                .collect(),
            ..HomeStats::default()
        });
        app
    }

    #[test]
    fn home_keys_move_the_activity_selection() {
        let mut app = home_app_with_events(5);

        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        handle_tui_key_event(&mut app, key(KeyCode::Down));
        assert_eq!(app.home_page().selected(), Some(1));
        handle_tui_key_event(&mut app, key(KeyCode::PageDown));
        assert_eq!(
            app.home_page().selected(),
            Some(2),
            "one-row page step before the first draw"
        );
        handle_tui_key_event(&mut app, key(KeyCode::End));
        assert_eq!(app.home_page().selected(), Some(4));
        handle_tui_key_event(&mut app, key(KeyCode::Up));
        assert_eq!(app.home_page().selected(), Some(3));
        handle_tui_key_event(&mut app, key(KeyCode::Home));
        assert_eq!(app.home_page().selected(), Some(0));
    }

    #[test]
    fn home_wheel_moves_the_activity_selection() {
        let mut app = home_app_with_events(3);
        let full = workspace_full(&app, TEST_AREA);
        let (column, row) = (full.x + 4, full.y + 6);

        handle_tui_mouse_event(
            &mut app,
            mouse_at(column, row, MouseEventKind::ScrollDown),
            TEST_AREA,
        );
        assert_eq!(app.home_page().selected(), Some(1));
        handle_tui_mouse_event(
            &mut app,
            mouse_at(column, row, MouseEventKind::ScrollUp),
            TEST_AREA,
        );
        assert_eq!(app.home_page().selected(), Some(0));
        handle_tui_mouse_event(
            &mut app,
            mouse_at(column, row, MouseEventKind::ScrollUp),
            TEST_AREA,
        );
        assert_eq!(app.home_page().selected(), Some(0), "top of the feed stops");
    }

    #[test]
    fn home_keys_copy_the_selected_event() {
        let mut app = home_app_with_events(2);
        library::actions::set_mock_clipboard(None);

        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        handle_tui_key_event(&mut app, key(KeyCode::Char('c')));
        assert_eq!(
            library::actions::read_clipboard().as_deref(),
            Some("body 0")
        );
        assert_eq!(app.notification(), Some("Copied to clipboard"));

        handle_tui_key_event(&mut app, key(KeyCode::Down));
        handle_tui_key_event(&mut app, key(KeyCode::Enter));
        assert_eq!(
            library::actions::read_clipboard().as_deref(),
            Some("body 1")
        );
    }

    #[test]
    fn home_copy_button_click_writes_the_clipboard() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = home_app_with_events(2);
        library::actions::set_mock_clipboard(None);
        let backend = TestBackend::new(TEST_AREA.width, TEST_AREA.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_app(frame, &mut app, TEST_AREA))
            .expect("test draw");

        let buffer = terminal.backend().buffer();
        let mut button = None;
        'rows: for y in TEST_AREA.top()..TEST_AREA.bottom() {
            for x in TEST_AREA.left()..TEST_AREA.right().saturating_sub(5) {
                let label: String = (x..x + 6).map(|x| buffer[(x, y)].symbol()).collect();
                if label == " copy " {
                    button = Some((x, y));
                    break 'rows;
                }
            }
        }
        let (x, y) = button.expect("copy button drawn");

        handle_tui_mouse_event(&mut app, left_click(x + 1, y), TEST_AREA);

        assert_eq!(
            library::actions::read_clipboard().as_deref(),
            Some("body 0")
        );
    }

    #[test]
    fn nav_rail_settings_icon_opens_the_settings_overlay() {
        let mut app = library_app();
        let settings = nav_button(&app, NavRailItem::Settings);
        handle_tui_mouse_event(
            &mut app,
            left_click(settings.x + 2, settings.y + 1),
            TEST_AREA,
        );
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn nav_rail_edge_drag_reopens_after_a_collapsing_drag() {
        let mut app = library_app();
        let divider = nav_rail::layout(TEST_AREA, false)
            .divider
            .expect("expanded divider");
        handle_tui_mouse_event(&mut app, left_click(divider.x, 10), TEST_AREA);
        handle_tui_mouse_event(
            &mut app,
            mouse_at(1, 10, MouseEventKind::Drag(MouseButton::Left)),
            TEST_AREA,
        );
        assert!(app.nav_rail().collapsed);
        assert_eq!(workspace_full(&app, TEST_AREA).x, 0);
        handle_tui_mouse_event(
            &mut app,
            mouse_at(1, 10, MouseEventKind::Up(MouseButton::Left)),
            TEST_AREA,
        );
        assert!(!app.nav_rail().dragging());

        // The parked edge divider drags the rail back open.
        handle_tui_mouse_event(&mut app, left_click(0, 10), TEST_AREA);
        assert!(app.nav_rail().dragging());
        handle_tui_mouse_event(
            &mut app,
            mouse_at(20, 10, MouseEventKind::Drag(MouseButton::Left)),
            TEST_AREA,
        );
        assert!(!app.nav_rail().collapsed);
        assert_eq!(workspace_full(&app, TEST_AREA).x, nav_rail::FULL_WIDTH);
    }

    #[test]
    fn double_clicking_the_nav_divider_collapses_the_rail() {
        let mut app = library_app();
        let divider = nav_rail::layout(TEST_AREA, false)
            .divider
            .expect("expanded divider");
        for _ in 0..2 {
            handle_tui_mouse_event(&mut app, left_click(divider.x, 10), TEST_AREA);
            handle_tui_mouse_event(
                &mut app,
                mouse_at(divider.x, 10, MouseEventKind::Up(MouseButton::Left)),
                TEST_AREA,
            );
        }
        assert!(app.nav_rail().collapsed);
    }

    #[test]
    fn nav_rail_divider_drag_snaps_and_release_ends_the_drag() {
        let mut app = library_app();
        let divider = nav_rail::layout(TEST_AREA, false)
            .divider
            .expect("expanded divider");
        handle_tui_mouse_event(&mut app, left_click(divider.x, 10), TEST_AREA);
        assert!(app.nav_rail().dragging());

        handle_tui_mouse_event(
            &mut app,
            mouse_at(1, 10, MouseEventKind::Drag(MouseButton::Left)),
            TEST_AREA,
        );
        assert!(app.nav_rail().collapsed);

        handle_tui_mouse_event(
            &mut app,
            mouse_at(20, 10, MouseEventKind::Drag(MouseButton::Left)),
            TEST_AREA,
        );
        assert!(!app.nav_rail().collapsed);

        handle_tui_mouse_event(
            &mut app,
            mouse_at(20, 10, MouseEventKind::Up(MouseButton::Left)),
            TEST_AREA,
        );
        assert!(!app.nav_rail().dragging());
    }

    #[test]
    fn nav_rail_hover_tracks_icons_and_clears_over_the_workspace() {
        let mut app = library_app();
        let home = nav_button(&app, NavRailItem::Home);
        handle_tui_mouse_event(
            &mut app,
            mouse_at(home.x, home.y, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert_eq!(app.nav_rail().hovered_item, Some(NavRailItem::Home));

        handle_tui_mouse_event(&mut app, mouse_at(20, 10, MouseEventKind::Moved), TEST_AREA);
        assert_eq!(app.nav_rail().hovered_item, None);
    }

    #[test]
    fn hovering_header_toggle_sets_and_clears_hover() {
        let mut app = library_app();
        seed_single_library_item(&mut app);

        let full = workspace_full(&app, TEST_AREA);
        let page = app.library_page();
        let content =
            library::detail::center_content(full, page.split_ratio(), page.detail_ratio());
        let x = content.x.saturating_add(
            content
                .width
                .saturating_sub(crate::widgets::toggle::TOGGLE_WIDTH),
        );
        let y = content.y;

        handle_tui_mouse_event(&mut app, mouse_at(x, y, MouseEventKind::Moved), TEST_AREA);
        assert_eq!(
            app.toggle_hover(),
            Some(crate::widgets::toggle::ToggleId::TriggerEnabled)
        );

        handle_tui_mouse_event(
            &mut app,
            mouse_at(content.x, y, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert_eq!(app.toggle_hover(), None);
    }

    #[test]
    fn create_divider_hover_tracks_the_line() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();
        let full = workspace_full(&app, TEST_AREA);
        let body = library::modals::create_body(full);
        let (_, divider, _) = library::modals::create_split(body, 0.5);
        let divider_top = body.y + crate::widgets::util::OVERLAY_HEADER_ROWS - 1;

        // The header band does not grab the divider, but the row directly
        // below it does.
        handle_tui_mouse_event(
            &mut app,
            mouse_at(divider, body.y, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft)) if !draft.divider_hover()
        ));

        handle_tui_mouse_event(
            &mut app,
            mouse_at(divider, divider_top, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft)) if draft.divider_hover()
        ));

        handle_tui_mouse_event(
            &mut app,
            mouse_at(divider + 1, divider_top, MouseEventKind::Moved),
            TEST_AREA,
        );
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft)) if !draft.divider_hover()
        ));
    }

    #[test]
    fn dragging_create_divider_resizes_panes_and_release_ends_it() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();
        let full = workspace_full(&app, TEST_AREA);
        let body = library::modals::create_body(full);
        let (_, divider, _) = library::modals::create_split(body, 0.5);
        let content_row = body.y + library::modals::CREATE_TRIGGER_FIELD_ROW;

        handle_tui_mouse_event(&mut app, left_click(divider, content_row), TEST_AREA);
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft)) if draft.divider_drag()
        ));

        handle_tui_mouse_event(
            &mut app,
            mouse_at(
                divider + 10,
                content_row,
                MouseEventKind::Drag(MouseButton::Left),
            ),
            TEST_AREA,
        );

        let split = match app.library_page().modal() {
            Some(library::LibraryModal::Create(draft)) => draft.split(),
            _ => panic!("create modal closed"),
        };
        assert!(split > 0.5);
        let (left, moved_divider, right) = library::modals::create_split(body, split);
        assert!(left.width > 42);
        assert_eq!(moved_divider, divider + 10);
        assert!(right.width < 42);

        handle_tui_mouse_event(
            &mut app,
            mouse_at(
                divider + 10,
                content_row,
                MouseEventKind::Up(MouseButton::Left),
            ),
            TEST_AREA,
        );
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft)) if !draft.divider_drag()
        ));
    }

    #[test]
    fn wheel_scroll_moves_settings_selection() {
        let mut app = library_app();
        app.open_settings_overlay();

        handle_tui_mouse_event(
            &mut app,
            mouse_at(30, 10, MouseEventKind::ScrollDown),
            TEST_AREA,
        );

        assert_eq!(app.settings_page().selected_index(), 1);
    }

    #[test]
    fn wheel_scroll_moves_selection_while_search_focused() {
        let mut app = library_app();
        app.open_settings_overlay();
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
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();
        let selected_before = app.library_page().selected_index();

        handle_tui_mouse_event(&mut app, left_click(30, 10), TEST_AREA);

        assert!(app.library_page().is_modal_open());
        assert_eq!(app.library_page().selected_index(), selected_before);
    }

    #[test]
    fn delete_key_opens_confirm_for_selected_row() {
        let mut app = library_app();
        seed_single_library_item(&mut app);

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));

        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn delete_key_while_settings_overlay_open_stays_in_settings() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.open_settings_overlay();

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));

        assert!(!app.library_page().is_modal_open());
        assert!(app.is_settings_overlay_open());
    }

    #[test]
    fn clicking_no_row_flips_delete_selection() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();
        assert_eq!(app.library_page().delete_selected_yes(), Some(true));

        let no = delete_button_cell(&app, library::modals::DELETE_NO_LABEL);
        handle_tui_mouse_event(&mut app, left_click(no.x, no.y), TEST_AREA);

        assert!(app.library_page().is_modal_open());
        assert_eq!(app.library_page().delete_selected_yes(), Some(false));
    }

    fn delete_button_cell(app: &App, label: &str) -> ratatui::layout::Rect {
        let full = workspace_full(app, TEST_AREA);
        let body = util::overlay_body(library::modals::delete_popup(full));
        let row = ratatui::layout::Rect {
            x: body.x,
            y: body
                .y
                .saturating_add(library::modals::DELETE_BUTTONS_OFFSET),
            width: body.width,
            height: 1,
        };
        let (yes, no) = util::confirm_button_cells(
            row,
            library::modals::DELETE_YES_LABEL,
            library::modals::DELETE_NO_LABEL,
        );
        if label == library::modals::DELETE_YES_LABEL {
            yes
        } else {
            no
        }
    }

    #[test]
    fn clicking_yes_row_selects_yes() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();
        app.library_page_mut().set_delete_selected_yes(false);

        let yes = delete_button_cell(&app, library::modals::DELETE_YES_LABEL);
        handle_tui_mouse_event(&mut app, left_click(yes.x, yes.y), TEST_AREA);

        assert!(app.library_page().is_modal_open());
        assert_eq!(app.library_page().delete_selected_yes(), Some(true));
    }

    #[test]
    fn clicking_outside_delete_modal_cancels() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_mouse_event(&mut app, left_click(0, 0), TEST_AREA);

        assert!(!app.library_page().is_modal_open());
    }

    fn ctrl_n() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_n_opens_create_modal() {
        let mut app = library_app();
        seed_single_library_item(&mut app);

        handle_tui_key_event(&mut app, ctrl_n());

        assert!(app.library_page().is_modal_open());
    }

    #[test]
    fn ctrl_n_blocked_while_library_modal_open() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();

        handle_tui_key_event(&mut app, ctrl_n());

        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::ConfirmDelete(_))
        ));
    }

    #[test]
    fn ctrl_n_blocked_while_settings_overlay_open() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.open_settings_overlay();

        handle_tui_key_event(&mut app, ctrl_n());

        assert!(!app.library_page().is_modal_open());
        assert!(app.is_settings_overlay_open());
    }

    fn ctrl_key(ch: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_i_opens_the_import_modal() {
        let mut app = library_app();

        handle_tui_key_event(&mut app, ctrl_key('i'));

        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Import(_))
        ));
    }

    #[test]
    fn ctrl_e_opens_the_export_modal() {
        let mut app = library_app();

        handle_tui_key_event(&mut app, ctrl_key('e'));

        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Export(_))
        ));
    }

    #[test]
    fn ctrl_f_and_ctrl_d_toggle_the_panes() {
        let mut app = library_app();

        handle_tui_key_event(&mut app, ctrl_key('f'));
        assert_eq!(app.library_page().split_ratio(), library::MIN_SPLIT_RATIO);
        handle_tui_key_event(&mut app, ctrl_key('f'));
        assert_eq!(
            app.library_page().split_ratio(),
            library::DEFAULT_SPLIT_RATIO
        );

        handle_tui_key_event(&mut app, ctrl_key('d'));
        assert_eq!(app.library_page().detail_ratio(), library::MIN_DETAIL_RATIO);
        handle_tui_key_event(&mut app, ctrl_key('d'));
        assert_eq!(
            app.library_page().detail_ratio(),
            library::DEFAULT_DETAIL_RATIO
        );
    }

    #[test]
    fn ctrl_b_toggles_the_navigation_rail() {
        let mut app = library_app();

        handle_tui_key_event(&mut app, ctrl_key('b'));
        assert!(app.nav_rail().collapsed);
        handle_tui_key_event(&mut app, ctrl_key('b'));
        assert!(!app.nav_rail().collapsed);
    }

    #[test]
    fn palette_toggles_panes_and_the_navigation_rail() {
        let mut app = library_app();

        execute_palette_command(&mut app, palette::PaletteCommand::ToggleNavRail);
        assert!(app.nav_rail().collapsed);
        execute_palette_command(&mut app, palette::PaletteCommand::ToggleSearchPane);
        assert_eq!(app.library_page().split_ratio(), library::MIN_SPLIT_RATIO);
        execute_palette_command(&mut app, palette::PaletteCommand::TogglePropertiesPane);
        assert_eq!(app.library_page().detail_ratio(), library::MIN_DETAIL_RATIO);
    }

    #[test]
    fn clicking_create_trigger_field_focuses_it() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();
        // Move focus off the trigger so the click has to bring it back.
        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            match app.library_page().modal() {
                Some(library::LibraryModal::Create(draft)) => draft.focus(),
                other => panic!("expected create modal, got {other:?}"),
            },
            library::LibraryCreateModalField::Description
        );

        // Trigger field row of the create modal on TEST_AREA.
        let full = workspace_full(&app, TEST_AREA);
        let body = crate::widgets::util::overlay_body(library::modals::create_popup(full));
        let trigger_row = body.y + library::modals::CREATE_TRIGGER_FIELD_ROW;
        handle_tui_mouse_event(&mut app, left_click(20, trigger_row), TEST_AREA);

        assert!(app.library_page().is_modal_open());
        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft))
                if draft.focus() == library::LibraryCreateModalField::Trigger
        ));
    }

    #[test]
    fn clicking_create_description_field_focuses_it() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();
        // Move focus to Content so the click has to bring it back.
        for _ in 0..2 {
            handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        assert_eq!(
            match app.library_page().modal() {
                Some(library::LibraryModal::Create(draft)) => draft.focus(),
                other => panic!("expected create modal, got {other:?}"),
            },
            library::LibraryCreateModalField::Content
        );

        let full = workspace_full(&app, TEST_AREA);
        let body = crate::widgets::util::overlay_body(library::modals::create_popup(full));
        let description_row = body.y + library::modals::CREATE_DESCRIPTION_FIELD_ROW;
        handle_tui_mouse_event(&mut app, left_click(20, description_row), TEST_AREA);

        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft))
                if draft.focus() == library::LibraryCreateModalField::Description
        ));
    }

    #[test]
    fn clicking_create_footer_does_nothing() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();

        // The old Create button row is now an inert footer.
        handle_tui_mouse_event(&mut app, left_click(20, 25), TEST_AREA);

        assert!(matches!(
            app.library_page().modal(),
            Some(library::LibraryModal::Create(draft)) if draft.error().is_none()
        ));
    }

    #[test]
    fn escape_closes_create_modal() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert!(!app.library_page().is_modal_open());
    }

    #[test]
    fn clicking_create_scrim_closes_modal() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();

        handle_tui_mouse_event(&mut app, left_click(0, 0), TEST_AREA);

        assert!(!app.library_page().is_modal_open());
    }

    #[test]
    fn clicking_create_esc_hint_closes_modal() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_create_modal();
        let full = workspace_full(&app, TEST_AREA);
        let body = crate::widgets::util::overlay_body(library::modals::create_popup(full));
        let column = body.x.saturating_add(body.width).saturating_sub(1);

        handle_tui_mouse_event(&mut app, left_click(column, body.y), TEST_AREA);

        assert!(!app.library_page().is_modal_open());
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_c_starts_close_animation_from_library() {
        let mut app = library_app();

        handle_tui_key_event(&mut app, ctrl_c());

        assert!(!app.should_quit());
        assert!(app.is_closing());
        assert!(app.transition_active());
    }

    #[test]
    fn ctrl_c_starts_close_animation_while_search_is_active() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, plain_key('/'));

        handle_tui_key_event(&mut app, ctrl_c());

        assert!(!app.should_quit());
        assert!(app.is_closing());
        assert!(app.transition_active());
    }

    #[test]
    fn ctrl_c_starts_close_animation_while_modal_is_open() {
        let mut app = library_app();
        seed_single_library_item(&mut app);
        app.library_page_mut().open_delete_modal_for_selected();
        assert!(app.library_page().is_modal_open());

        handle_tui_key_event(&mut app, ctrl_c());

        assert!(!app.should_quit());
        assert!(app.is_closing());
        assert!(app.transition_active());
    }

    #[test]
    fn second_ctrl_c_quits_immediately() {
        let mut app = library_app();
        handle_tui_key_event(&mut app, ctrl_c());
        assert!(!app.should_quit());

        handle_tui_key_event(&mut app, ctrl_c());

        assert!(app.should_quit());
    }

    #[test]
    fn pressing_n_on_library_types_into_search_without_modal() {
        let mut app = library_app();

        handle_tui_key_event(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        handle_tui_key_event(&mut app, plain_key('n'));

        assert!(!app.library_page().is_modal_open());
        assert!(app.library_page().is_search_active());
        assert_eq!(app.library_page().search_query(), "n");
    }
}
