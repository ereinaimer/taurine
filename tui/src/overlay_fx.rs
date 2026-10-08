//! Shared overlay transitions: unified single-color alpha fade for every
//! overlay open and close. Overlays report the popup rect they painted
//! through `OverlayTransition`; the run loop plays the alpha fade over
//! those rects whenever a layer appears or dismisses. Pure state tracking
//! lives here so it stays unit-testable without a terminal.

use ratatui::layout::Rect;
use tachyonfx::{Effect, EffectTimer, Interpolation, fx::fade_from};

use crate::terminal::app::App;
use crate::theme::Theme;
use crate::widgets::library::state::{LibraryModal, LibraryModalKind, app_filter_geometry};
use crate::widgets::settings::modals as settings_modals;
use crate::widgets::settings::{self, SettingsModal, SettingsModalKind};
use std::sync::atomic::{AtomicBool, Ordering};

static CURSOR_SUPPRESSED: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_cursor_suppressed() -> bool {
    CURSOR_SUPPRESSED.load(Ordering::Relaxed)
}

pub(crate) fn set_cursor_suppressed(suppressed: bool) {
    CURSOR_SUPPRESSED.store(suppressed, Ordering::Relaxed);
}

pub(crate) struct CursorSuppressGuard;

impl CursorSuppressGuard {
    pub(crate) fn new() -> Self {
        set_cursor_suppressed(true);
        Self
    }
}

impl Drop for CursorSuppressGuard {
    fn drop(&mut self) {
        set_cursor_suppressed(false);
    }
}

/// Shared overlay transition length: fast 200ms alpha fade for both open and close.
pub(crate) const OVERLAY_FX_MS: u32 = 200;

/// Geometry both an overlay render and its sweep agree on.
pub(crate) struct FxAreas {
    pub frame: Rect,
    pub library_full: Rect,
}

/// Shared open-transition logic: every overlay reports the popup rect
/// it painted, so one sweep animates them all. `None` only for overlays
/// that never open (kept for the shortcut rework).
pub(crate) trait OverlayTransition {
    fn transition_rect(&self, areas: &FxAreas) -> Option<Rect>;
}

impl OverlayTransition for LibraryModal {
    fn transition_rect(&self, areas: &FxAreas) -> Option<Rect> {
        match self {
            // honey: unreachable until the shortcut rework lands; no
            // appear sweep for dialogs that never open.
            Self::Export(_)
            | Self::ExportResult(_)
            | Self::Import(_)
            | Self::ImportResult(_)
            | Self::ConfirmImportRunVariables(_)
            | Self::ConfirmDelete(_) => None,
            Self::HeaderMenu(_) | Self::Tags(_) | Self::Create(_) => {
                Some(crate::widgets::util::overlay_popup(areas.library_full))
            }
            Self::AppFilter(menu) => Some(app_filter_geometry(areas.library_full, menu).0),
        }
    }
}

impl OverlayTransition for SettingsModal {
    fn transition_rect(&self, areas: &FxAreas) -> Option<Rect> {
        match self {
            Self::Input(_) => Some(settings_modals::input_popup(areas.frame)),
            Self::Select(_) => Some(crate::widgets::util::overlay_popup(areas.frame)),
            Self::ConfirmReset(_) => Some(settings_modals::confirm_popup(areas.frame)),
            Self::HotkeyCapture(_) => Some(settings_modals::hotkey_popup(areas.frame)),
        }
    }
}

/// Shared single-color terminal alpha fade: smoothly interpolates cell RGB values
/// to/from the theme background color using alpha blending, with zero character
/// modifications or multi-color gradients.
pub(crate) fn overlay_effect(theme: &Theme) -> Effect {
    fade_from(
        theme.background,
        theme.background,
        EffectTimer::from_ms(OVERLAY_FX_MS, Interpolation::Linear),
    )
}

/// One appear transition for every overlay: unified single-color alpha fade.
pub(crate) fn open_effect(theme: &Theme) -> Effect {
    overlay_effect(theme)
}

/// One dismissal transition for every overlay: unified single-color alpha fade.
pub(crate) fn close_effect(theme: &Theme) -> Effect {
    overlay_effect(theme)
}

/// Visible overlay layers, by discriminant only: hover, scroll, and
/// live reseeds never change these, so same-kind updates stay still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverlayLayers {
    pub library: Option<LibraryModalKind>,
    pub settings: bool,
    pub settings_editor: Option<SettingsModalKind>,
}

impl OverlayLayers {
    pub(crate) const fn is_any_open(&self) -> bool {
        self.library.is_some() || self.settings || self.settings_editor.is_some()
    }
}

pub(crate) fn layers(app: &App) -> OverlayLayers {
    OverlayLayers {
        library: app.library_page().modal().map(LibraryModal::kind),
        settings: app.is_settings_overlay_open(),
        settings_editor: app.settings_page().modal().map(SettingsModal::kind),
    }
}

/// True when a layer newly appeared: opens and dialog switches sweep,
/// closes and same-kind updates never do.
pub(crate) fn appeared(previous: &OverlayLayers, next: &OverlayLayers) -> bool {
    if next == previous {
        return false;
    }
    (next.library.is_some() && previous.library != next.library)
        || (next.settings && !previous.settings)
        || (next.settings_editor.is_some() && previous.settings_editor != next.settings_editor)
}

/// True when an overlay layer closed: dismissals sweep inward;
/// opens and same-kind updates never do.
pub(crate) fn disappeared(previous: &OverlayLayers, next: &OverlayLayers) -> bool {
    if next == previous {
        return false;
    }
    (previous.library.is_some() && next.library.is_none())
        || (previous.settings && !next.settings)
        || (previous.settings_editor.is_some() && next.settings_editor.is_none())
}

/// Returns the popup rects of the overlays that just closed between
/// `previous` and `next`.
pub(crate) fn closed_rects(
    previous: &OverlayLayers,
    next: &OverlayLayers,
    previous_rects: &[Rect],
) -> Vec<Rect> {
    if previous == next {
        return Vec::new();
    }
    if previous.settings_editor.is_some()
        && next.settings_editor.is_none()
        && next.settings
        && let Some(top) = previous_rects.last()
    {
        return vec![*top];
    }
    previous_rects.to_vec()
}

/// Popup rects of every currently visible overlay, in paint order.
pub(crate) fn transition_rects(app: &App, frame: Rect, library_full: Rect) -> Vec<Rect> {
    let areas = FxAreas {
        frame,
        library_full,
    };
    let mut rects = Vec::new();
    if let Some(modal) = app.library_page().modal() {
        rects.extend(modal.transition_rect(&areas));
    }
    if app.is_settings_overlay_open() {
        rects.push(settings::overlay_popup(frame, app.settings_page()));
    }
    if let Some(modal) = app.settings_page().modal() {
        rects.extend(modal.transition_rect(&areas));
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::default::DARK_THEME;
    use crate::widgets::library::state::{
        AppFilterSide, LibraryAppFilterState, LibraryTagsModalState,
    };

    fn tags_modal() -> LibraryModal {
        LibraryModal::Tags(LibraryTagsModalState::new(
            "id".to_string(),
            0,
            vec!["work".to_string()],
        ))
    }

    fn closed() -> OverlayLayers {
        OverlayLayers {
            library: None,
            settings: false,
            settings_editor: None,
        }
    }

    #[test]
    fn opening_any_layer_sweeps() {
        let open_library = OverlayLayers {
            library: Some(LibraryModalKind::Tags),
            ..closed()
        };
        assert!(appeared(&closed(), &open_library));
        let open_settings = OverlayLayers {
            settings: true,
            ..closed()
        };
        assert!(appeared(&closed(), &open_settings));
        let open_editor = OverlayLayers {
            settings: true,
            settings_editor: Some(SettingsModalKind::Select),
            ..closed()
        };
        assert!(appeared(&open_settings, &open_editor));
    }

    #[test]
    fn closes_and_same_kind_updates_stay_still() {
        let open = OverlayLayers {
            library: Some(LibraryModalKind::Tags),
            ..closed()
        };
        assert!(!appeared(&open, &closed()));
        assert!(!appeared(&open, &open));
        assert!(!appeared(&closed(), &closed()));
    }

    #[test]
    fn dialog_switches_sweep() {
        let tags = OverlayLayers {
            library: Some(LibraryModalKind::Tags),
            ..closed()
        };
        let apps = OverlayLayers {
            library: Some(LibraryModalKind::AppFilter),
            ..closed()
        };
        assert!(appeared(&tags, &apps));
    }

    #[test]
    fn open_effect_runs_then_completes() {
        use ratatui::buffer::Buffer;

        let mut effect = open_effect(&DARK_THEME);
        assert!(effect.running());
        let area = Rect::new(0, 0, 20, 5);
        let mut buffer = Buffer::empty(area);
        effect.process(
            tachyonfx::Duration::from_millis(OVERLAY_FX_MS + 50),
            &mut buffer,
            area,
        );
        assert!(!effect.running());
    }

    #[test]
    fn open_effect_fades_in_content() {
        use ratatui::buffer::Buffer;
        use ratatui::style::Color;

        let area = Rect::new(0, 0, 20, 5);
        let mut effect = open_effect(&DARK_THEME);

        let make_buffer = || {
            let mut buf = Buffer::empty(area);
            for cell in buf.content.iter_mut() {
                cell.set_char('x');
                cell.set_fg(Color::Rgb(255, 255, 255));
                cell.set_bg(Color::Rgb(30, 30, 30));
            }
            buf
        };

        // At t=0: cells start faded from the theme background color
        let mut buffer = make_buffer();
        effect.process(tachyonfx::Duration::ZERO, &mut buffer, area);
        assert_eq!(buffer.content[0].fg, DARK_THEME.background);
        assert_eq!(buffer.content[0].bg, DARK_THEME.background);

        // Frame 1 (16ms): alpha blending begins immediately
        let mut buf1 = make_buffer();
        effect.process(tachyonfx::Duration::from_millis(16), &mut buf1, area);
        assert_ne!(buf1.content[0].fg, DARK_THEME.background);

        // At completion: cells have smoothly interpolated back to their target colors
        let mut buffer = make_buffer();
        effect.process(
            tachyonfx::Duration::from_millis(OVERLAY_FX_MS),
            &mut buffer,
            area,
        );
        assert_eq!(buffer.content[0].fg, Color::Rgb(255, 255, 255));
        assert_eq!(buffer.content[0].bg, Color::Rgb(30, 30, 30));
        assert!(!effect.running());
    }

    #[test]
    fn open_and_close_effects_are_symmetrical() {
        assert_eq!(OVERLAY_FX_MS, 200);
    }

    #[test]
    fn transition_rects_follow_visible_modals() {
        let area = Rect::new(0, 0, 100, 30);
        let full = Rect::new(0, 0, 100, 30);
        let mut app = App::default();
        assert!(transition_rects(&app, area, full).is_empty());

        app.library_page_mut().modal = Some(tags_modal());
        let rects = transition_rects(&app, area, full);
        assert_eq!(rects, vec![crate::widgets::util::overlay_popup(full)]);

        let menu = LibraryAppFilterState::new(
            "id".to_string(),
            0,
            AppFilterSide::Allow,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let expected = app_filter_geometry(full, &menu).0;
        app.library_page_mut().modal = Some(LibraryModal::AppFilter(menu));
        assert_eq!(transition_rects(&app, area, full), vec![expected]);
    }

    #[test]
    fn close_effect_runs_then_completes() {
        use ratatui::buffer::Buffer;

        let area = Rect::new(0, 0, 20, 5);
        let mut effect = close_effect(&DARK_THEME);
        assert!(effect.running());
        let mut buffer = Buffer::empty(area);
        effect.process(
            tachyonfx::Duration::from_millis(OVERLAY_FX_MS + 50),
            &mut buffer,
            area,
        );
        assert!(!effect.running());
    }

    #[test]
    fn close_sweep_fades_in_revealed_content() {
        use ratatui::buffer::Buffer;
        use ratatui::style::Color;

        let area = Rect::new(0, 0, 20, 5);
        let mut effect = close_effect(&DARK_THEME);

        let make_buffer = || {
            let mut buf = Buffer::empty(area);
            for cell in buf.content.iter_mut() {
                cell.set_char('A');
                cell.set_fg(Color::Rgb(255, 255, 255));
                cell.set_bg(Color::Rgb(30, 30, 30));
            }
            buf
        };

        // At t=0: cells start faded from the theme background color
        let mut buffer = make_buffer();
        effect.process(tachyonfx::Duration::ZERO, &mut buffer, area);
        assert_eq!(buffer.content[0].fg, DARK_THEME.background);
        assert_eq!(buffer.content[0].bg, DARK_THEME.background);

        // At completion: cells have smoothly interpolated back to their target colors
        let mut buffer = make_buffer();
        effect.process(
            tachyonfx::Duration::from_millis(OVERLAY_FX_MS),
            &mut buffer,
            area,
        );
        assert_eq!(buffer.content[0].fg, Color::Rgb(255, 255, 255));
        assert_eq!(buffer.content[0].bg, Color::Rgb(30, 30, 30));
        assert!(!effect.running());
    }

    #[test]
    fn disappeared_detects_layer_dismissals() {
        let open_library = OverlayLayers {
            library: Some(LibraryModalKind::Tags),
            ..closed()
        };
        assert!(disappeared(&open_library, &closed()));
        let open_settings = OverlayLayers {
            settings: true,
            ..closed()
        };
        assert!(disappeared(&open_settings, &closed()));
        let open_editor = OverlayLayers {
            settings: true,
            settings_editor: Some(SettingsModalKind::Input),
            ..closed()
        };
        assert!(disappeared(&open_editor, &open_settings));

        // Opens and same-kind updates never trigger disappeared
        assert!(!disappeared(&closed(), &open_library));
        assert!(!disappeared(&open_library, &open_library));
        assert!(!disappeared(&closed(), &closed()));
    }

    #[test]
    fn closed_rects_isolates_editor_or_full_layer() {
        let rect1 = Rect::new(0, 0, 50, 20);
        let rect2 = Rect::new(10, 5, 30, 10);
        let open_settings = OverlayLayers {
            settings: true,
            ..closed()
        };
        let open_editor = OverlayLayers {
            settings: true,
            settings_editor: Some(SettingsModalKind::Input),
            ..closed()
        };

        // Editor modal closing only dissolves the editor popup, keeping settings intact
        assert_eq!(
            closed_rects(&open_editor, &open_settings, &[rect1, rect2]),
            vec![rect2]
        );
        // Entire settings overlay closing dissolves all its rects
        assert_eq!(
            closed_rects(&open_settings, &closed(), &[rect1]),
            vec![rect1]
        );
    }

    #[test]
    fn close_sweep_begins_immediately_on_first_frame() {
        use ratatui::buffer::Buffer;
        use ratatui::style::Color;

        let area = Rect::new(0, 0, 40, 18);
        let mut effect = close_effect(&DARK_THEME);

        let make_buffer = || {
            let mut buf = Buffer::empty(area);
            for cell in buf.content.iter_mut() {
                cell.set_char('x');
                cell.set_fg(Color::Rgb(255, 255, 255));
                cell.set_bg(Color::Rgb(30, 30, 30));
            }
            buf
        };

        // Frame 0 (at t=0): cells start faded to background color.
        let mut buf0 = make_buffer();
        effect.process(tachyonfx::Duration::ZERO, &mut buf0, area);
        assert_eq!(buf0.content[0].fg, DARK_THEME.background);

        // Frame 1 (16ms): transition immediately begins interpolating towards target colors at the leading edge.
        let mut buf1 = make_buffer();
        effect.process(tachyonfx::Duration::from_millis(16), &mut buf1, area);
        let leading = ratatui::layout::Position::new(0, 0);
        assert_ne!(
            buf1.cell(leading).unwrap().fg,
            DARK_THEME.background,
            "first animation frame must immediately interpolate colors"
        );
    }
}
