use std::time::{Duration, Instant};

use tachyonfx::Effect;

use crate::layout_store::TuiLayout;
use crate::screen_fx;
use crate::theme::Theme;
use crate::theme::default::DARK_THEME;
use crate::widgets::library::LibraryPageState;
use crate::widgets::library::props::PropsHit;
use crate::widgets::library::recorder::HotkeyRecorderState;
use crate::widgets::nav_rail::{NavRailState, RailIcons};
use crate::widgets::palette::CommandPaletteState;
use crate::widgets::settings::state::SettingsPageState;
use crate::widgets::toggle::{ToggleAnimations, ToggleId};

pub(crate) struct App {
    library_page: LibraryPageState,
    settings_page: SettingsPageState,
    settings_overlay_open: bool,
    should_quit: bool,
    notification: Option<String>,
    current_theme: &'static Theme,
    toggle_animations: ToggleAnimations,
    toggle_hover: Option<ToggleId>,
    props_hover: Option<PropsHit>,
    saved_layout: TuiLayout,
    palette: Option<CommandPaletteState>,
    hotkey_recorder: Option<HotkeyRecorderState>,
    nav_rail: NavRailState,
    rail_icons: Option<RailIcons>,
    /// Cached daemon pause flag: the palette labels its toggle from it,
    /// refreshed when the palette opens and after every transition.
    daemon_paused: bool,
    transition_effect: Option<Effect>,
    last_frame_instant: Instant,
    is_closing: bool,
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
            toggle_animations: ToggleAnimations::default(),
            toggle_hover: None,
            props_hover: None,
            saved_layout: TuiLayout::default(),
            palette: None,
            hotkey_recorder: None,
            nav_rail: NavRailState::default(),
            rail_icons: None,
            daemon_paused: false,
            transition_effect: None,
            last_frame_instant: Instant::now(),
            is_closing: false,
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
        self.nav_rail.clear_hover();
        self.toggle_hover = None;
        self.props_hover = None;
    }

    pub(crate) fn close_settings_overlay(&mut self) {
        self.settings_overlay_open = false;
        self.settings_page.clear_modal();
        self.settings_page.clear_inline_edit();
        self.settings_page.cancel_slider_drag();
        self.toggle_hover = None;
        self.props_hover = None;
    }

    pub(crate) const fn palette(&self) -> Option<&CommandPaletteState> {
        self.palette.as_ref()
    }

    pub(crate) const fn palette_open(&self) -> bool {
        self.palette.is_some()
    }

    pub(crate) fn palette_mut(&mut self) -> Option<&mut CommandPaletteState> {
        self.palette.as_mut()
    }

    /// Opens the palette fresh at the first command, or closes it when
    /// Ctrl+P toggles an open palette.
    pub(crate) fn toggle_palette(&mut self) {
        self.palette = if self.palette.is_some() {
            None
        } else {
            Some(CommandPaletteState::default())
        };
        self.nav_rail.clear_hover();
        self.toggle_hover = None;
        self.props_hover = None;
    }

    pub(crate) fn close_palette(&mut self) {
        self.palette = None;
    }

    pub(crate) fn hotkey_recorder(&self) -> Option<&HotkeyRecorderState> {
        self.hotkey_recorder.as_ref()
    }

    pub(crate) fn hotkey_recorder_mut(&mut self) -> Option<&mut HotkeyRecorderState> {
        self.hotkey_recorder.as_mut()
    }

    pub(crate) fn set_hotkey_recorder(&mut self, recorder: Option<HotkeyRecorderState>) {
        self.hotkey_recorder = recorder;
        self.toggle_hover = None;
        self.props_hover = None;
    }

    pub(crate) const fn nav_rail(&self) -> &NavRailState {
        &self.nav_rail
    }

    pub(crate) fn nav_rail_mut(&mut self) -> &mut NavRailState {
        &mut self.nav_rail
    }

    /// Rasterize the rail glyphs once, after the terminal can answer the
    /// graphics probe. Failure keeps the glyph fallback.
    pub(crate) fn load_rail_icons(&mut self) {
        use ratatui_image::picker::Picker;

        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        let font = picker.font_size();
        // honey: crossterm cannot report pixel dimensions on Windows, so
        // the picker's CSI 16 t answer is the only cell-size source.
        match crossterm::terminal::window_size() {
            Ok(size) => tracing::info!(
                cols = size.columns,
                rows = size.rows,
                px_w = size.width,
                px_h = size.height,
                "terminal window metrics"
            ),
            Err(error) => {
                tracing::debug!(%error, "terminal window pixel metrics unavailable");
            }
        }
        tracing::info!(
            protocol = ?picker.protocol_type(),
            cell_w = font.width,
            cell_h = font.height,
            "navigation rail graphics"
        );
        if font.width == 0 || font.height == 0 {
            tracing::warn!("terminal reported a zero-size cell; keeping the glyph fallback");
            return;
        }
        match RailIcons::load(&picker, self.current_theme) {
            Ok(icons) => self.rail_icons = Some(icons),
            Err(error) => {
                tracing::warn!(%error, "failed to rasterize the navigation rail icons");
            }
        }
    }

    /// Both rail halves at once so rendering can borrow the state and the
    /// protocols together.
    pub(crate) fn nav_rail_graphics(&mut self) -> (&mut NavRailState, Option<&RailIcons>) {
        (&mut self.nav_rail, self.rail_icons.as_ref())
    }

    pub(crate) const fn daemon_paused(&self) -> bool {
        self.daemon_paused
    }

    pub(crate) fn set_daemon_paused(&mut self, paused: bool) {
        self.daemon_paused = paused;
    }

    /// Re-reads the pause flag from the daemon before the palette draws
    /// its toggle. Offline or unreachable keeps the last known value.
    pub(crate) fn refresh_daemon_paused(&mut self) {
        if let Some(paused) = taurine_core::rpc::daemon_paused() {
            self.daemon_paused = paused;
        }
    }

    pub(crate) const fn toggle_animations(&self) -> &ToggleAnimations {
        &self.toggle_animations
    }

    pub(crate) fn start_toggle_anim(&mut self, id: ToggleId, from_on: bool) {
        self.toggle_animations.start(id, from_on);
    }

    pub(crate) fn advance_toggle_animations(&mut self, now: Instant) -> bool {
        self.toggle_animations.has_active_animations(now)
    }

    pub(crate) const fn toggle_hover(&self) -> Option<ToggleId> {
        self.toggle_hover
    }

    pub(crate) fn set_toggle_hover(&mut self, hover: Option<ToggleId>) {
        self.toggle_hover = hover;
    }

    pub(crate) const fn props_hover(&self) -> Option<PropsHit> {
        self.props_hover
    }

    pub(crate) fn set_props_hover(&mut self, hover: Option<PropsHit>) {
        self.props_hover = hover;
    }

    /// Current pane layout, normalized so a stray non-finite ratio can
    /// never make the page look permanently dirty.
    pub(crate) fn layout(&self) -> TuiLayout {
        TuiLayout {
            list_ratio: self.library_page.split_ratio(),
            detail_ratio: self.library_page.detail_ratio(),
            create_split: self.library_page.create_split_ratio(),
        }
        .sanitized()
    }

    /// Restores a stored layout, clamped first so a corrupt row can never
    /// park a pane; the saved copy is the clamped one so the page does not
    /// immediately look dirty.
    pub(crate) fn apply_layout(&mut self, layout: TuiLayout) {
        let layout = layout.sanitized();
        self.library_page.set_split_ratio(layout.list_ratio);
        self.library_page.set_detail_ratio(layout.detail_ratio);
        self.library_page
            .set_create_split_ratio(layout.create_split);
        self.saved_layout = layout;
    }

    pub(crate) fn layout_needs_save(&self) -> bool {
        self.layout() != self.saved_layout
    }

    pub(crate) fn mark_layout_saved(&mut self) {
        self.saved_layout = self.layout();
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

    pub(crate) const fn is_closing(&self) -> bool {
        self.is_closing
    }

    pub(crate) const fn transition_active(&self) -> bool {
        self.transition_effect.is_some()
    }

    pub(crate) fn begin_launch_transition(&mut self) {
        self.is_closing = false;
        self.transition_effect = Some(screen_fx::launch_effect(self.current_theme));
        self.last_frame_instant = Instant::now();
    }

    pub(crate) fn begin_close_transition(&mut self) {
        if self.is_closing {
            return;
        }
        self.is_closing = true;
        self.transition_effect = Some(screen_fx::close_effect(self.current_theme));
        self.last_frame_instant = Instant::now();
    }

    pub(crate) fn frame_elapsed(&mut self) -> Duration {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_frame_instant);
        self.last_frame_instant = now;
        elapsed
    }

    pub(crate) fn transition_effect_mut(&mut self) -> Option<&mut Effect> {
        self.transition_effect.as_mut()
    }

    pub(crate) fn clear_finished_transition(&mut self) {
        if self.transition_effect.as_ref().is_some_and(Effect::done) {
            self.transition_effect = None;
        }
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
    fn begin_launch_transition_starts_effect() {
        let mut app = App::default();
        assert!(!app.transition_active());

        app.begin_launch_transition();

        assert!(app.transition_active());
        assert!(!app.is_closing());
    }

    #[test]
    fn begin_close_transition_starts_closing_effect() {
        let mut app = App::default();

        app.begin_close_transition();

        assert!(app.is_closing());
        assert!(app.transition_active());
        assert!(!app.should_quit());
    }

    #[test]
    fn defaults_settings_page_to_first_row() {
        let app = App::default();
        assert_eq!(app.settings_page().selected_index(), 0);
    }

    #[test]
    fn toggle_animation_registry_tracks_multiple_switches() {
        use std::time::Duration;

        use crate::widgets::toggle::TOGGLE_ANIM_DURATION;

        let mut app = App::default();
        let start = Instant::now();
        app.toggle_animations
            .start_at(ToggleId::TriggerEnabled, false, start);
        app.toggle_animations.start_at(
            ToggleId::Setting("start_on_boot"),
            true,
            start + Duration::from_millis(40),
        );

        assert!(app.advance_toggle_animations(start));
        assert!(app.advance_toggle_animations(start + TOGGLE_ANIM_DURATION));
        assert!(!app.advance_toggle_animations(start + Duration::from_millis(130)));
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

    fn sample_layout() -> TuiLayout {
        TuiLayout {
            list_ratio: 0.31,
            detail_ratio: 0.82,
            create_split: 0.63,
        }
    }

    #[test]
    fn app_default_layout_matches_page_defaults() {
        let app = App::default();

        assert_eq!(app.layout(), TuiLayout::default());
        assert!(!app.layout_needs_save());
    }

    #[test]
    fn apply_layout_sets_all_main_ratios_and_marks_clean() {
        let mut app = App::default();

        app.apply_layout(sample_layout());

        assert_eq!(app.library_page().split_ratio(), 0.31);
        assert_eq!(app.library_page().detail_ratio(), 0.82);
        assert_eq!(app.library_page().create_split_ratio(), 0.63);
        assert_eq!(app.layout(), sample_layout());
        assert!(!app.layout_needs_save());
    }

    #[test]
    fn apply_layout_clamps_saved_values() {
        let mut app = App::default();

        app.apply_layout(TuiLayout {
            list_ratio: 9.0,
            detail_ratio: -1.0,
            create_split: 5.0,
        });

        let clamped = TuiLayout {
            list_ratio: crate::widgets::library::MAX_SPLIT_RATIO,
            detail_ratio: crate::widgets::library::MIN_DETAIL_RATIO,
            create_split: crate::widgets::library::CREATE_MAX_SPLIT,
        };
        assert_eq!(app.layout(), clamped);
        assert!(!app.layout_needs_save());
    }

    #[test]
    fn main_divider_change_marks_layout_dirty() {
        let mut app = App::default();
        assert!(!app.layout_needs_save());

        app.library_page_mut().set_split_ratio(0.5);

        assert!(app.layout_needs_save());
    }

    #[test]
    fn mark_layout_saved_clears_dirty() {
        let mut app = App::default();
        app.library_page_mut().set_detail_ratio(0.9);
        assert!(app.layout_needs_save());

        app.mark_layout_saved();

        assert!(!app.layout_needs_save());
    }

    #[test]
    fn apply_layout_sets_create_split_for_next_modal() {
        let mut app = App::default();
        app.apply_layout(TuiLayout {
            create_split: 0.7,
            ..TuiLayout::default()
        });

        app.library_page_mut().open_create_modal();

        match app.library_page().modal() {
            Some(crate::widgets::library::LibraryModal::Create(draft)) => {
                assert!((draft.split() - 0.7).abs() < f32::EPSILON);
            }
            other => panic!("expected create modal, got {other:?}"),
        }
    }
}
