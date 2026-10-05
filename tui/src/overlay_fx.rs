//! Shared overlay open-transition: one tachyonfx evolve-into sweep for
//! every overlay. Overlays report the popup rect they painted through
//! `OverlayTransition`; the run loop plays the sweep over those rects
//! whenever a layer newly appears. Pure state tracking lives here so it
//! stays unit-testable without a terminal.

use ratatui::layout::Rect;
use tachyonfx::{
    Effect, EffectTimer, Interpolation, fx::EvolveSymbolSet, fx::evolve_into,
    pattern::RadialPattern,
};

use crate::terminal::app::App;
use crate::widgets::library::state::{LibraryModal, LibraryModalKind, app_filter_geometry};
use crate::widgets::settings;
use crate::widgets::settings::modals as settings_modals;
use crate::widgets::settings::state::{SettingsModal, SettingsModalKind};

/// Open-sweep length: a beat, never a wait.
pub(crate) const OPEN_FX_MS: u32 = 200;

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
            Self::HeaderMenu(_) | Self::Tags(_) => {
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

/// One appear sweep for every overlay: shaded evolve-into resolving to
/// the painted content at completion. The radial pattern staggers cells
/// center-outwards; without it every cell would hit the solid block
/// glyph on the same frame and flash white.
pub(crate) fn open_effect() -> Effect {
    evolve_into(
        EvolveSymbolSet::Shaded,
        EffectTimer::from_ms(OPEN_FX_MS, Interpolation::QuadOut),
    )
    .with_pattern(RadialPattern::center())
}

/// Visible overlay layers, by discriminant only: hover, scroll, and
/// live reseeds never change these, so same-kind updates stay still.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverlayLayers {
    pub library: Option<LibraryModalKind>,
    pub settings: bool,
    pub settings_editor: Option<SettingsModalKind>,
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

        let mut effect = open_effect();
        assert!(effect.running());
        let area = Rect::new(0, 0, 20, 5);
        let mut buffer = Buffer::empty(area);
        effect.process(
            tachyonfx::Duration::from_millis(OPEN_FX_MS + 50),
            &mut buffer,
            area,
        );
        assert!(!effect.running());
    }

    #[test]
    fn open_sweep_resolves_progressively_without_full_block_flash() {
        use ratatui::buffer::Buffer;

        // honey: mirrors the driver: each frame repaints fresh content,
        // then the sweep processes on top of it.
        let area = Rect::new(0, 0, 30, 10);
        let total = 30 * 10;
        let mut effect = open_effect();
        let mut saw_mixed = false;
        let mut saw_all_block = false;
        let step = tachyonfx::Duration::from_millis(8);
        while effect.running() {
            let mut buffer = Buffer::empty(area);
            for cell in buffer.content.iter_mut() {
                cell.set_char('x');
            }
            effect.process(step, &mut buffer, area);
            let resolved = buffer
                .content
                .iter()
                .filter(|cell| cell.symbol() == "x")
                .count();
            let blocks = buffer
                .content
                .iter()
                .filter(|cell| cell.symbol() == "█")
                .count();
            if resolved > 0 && resolved < total {
                saw_mixed = true;
            }
            if blocks == total {
                saw_all_block = true;
            }
        }
        assert!(saw_mixed, "sweep should resolve cells progressively");
        assert!(
            !saw_all_block,
            "sweep should never turn the whole popup into solid blocks at once"
        );
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
}
