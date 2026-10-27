//! Collapsible navigation rail on the far-left edge.
//!
//! The rail is a fixed 5-cell content column plus a 1-cell divider. It
//! never resizes freely: dragging the divider snaps between expanded and
//! collapsed, and double-clicking it toggles. Collapsed gives every cell
//! back to the workspace and parks the divider on the left edge,
//! mirroring how the right pane collapses. Buttons render SVG icons
//! rasterized at startup, with a bold glyph fallback for headless runs.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use ratatui_image::Image;

mod icons;

pub(crate) use icons::RailIcons;

use icons::IconVariants;

use crate::theme::Theme;
use crate::widgets::library::DIVIDER_HOVER_COLOR;
use crate::widgets::library::state::now_millis;
use crate::widgets::panes::DIVIDER_DOUBLE_CLICK_MS;

/// Rail content: 1-cell margin, 4-cell button, 1-cell margin.
pub(crate) const RAIL_WIDTH: u16 = 6;
pub(crate) const DIVIDER_WIDTH: u16 = 1;
/// Expanded rail including the divider. This is also the hard width cap.
pub(crate) const FULL_WIDTH: u16 = RAIL_WIDTH + DIVIDER_WIDTH;
pub(crate) const BUTTON_WIDTH: u16 = 4;
pub(crate) const BUTTON_HEIGHT: u16 = 2;
/// Blank rows between stacked buttons.
const BUTTON_GAP: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivePage {
    Home,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavRailItem {
    Home,
    Library,
    CommandPalette,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavRailState {
    pub(crate) active_page: ActivePage,
    pub(crate) collapsed: bool,
    pub(crate) hovered_item: Option<NavRailItem>,
    pub(crate) hitboxes: Vec<(NavRailItem, Rect)>,
    hovered_divider: bool,
    dragging: bool,
    last_divider_click: Option<u64>,
}

impl Default for NavRailState {
    fn default() -> Self {
        Self {
            active_page: ActivePage::Home,
            collapsed: false,
            hovered_item: None,
            hitboxes: Vec::new(),
            hovered_divider: false,
            dragging: false,
            last_divider_click: None,
        }
    }
}

impl NavRailState {
    pub(crate) fn set_active_page(&mut self, page: ActivePage) {
        self.active_page = page;
    }

    pub(crate) const fn hovered_divider(&self) -> bool {
        self.hovered_divider
    }

    pub(crate) const fn dragging(&self) -> bool {
        self.dragging
    }

    /// Rebuild the hitboxes from the same geometry the render path uses.
    pub(crate) fn refresh_hitboxes(&mut self, area: Rect) {
        self.hitboxes = layout(area, self.collapsed).buttons;
    }

    pub(crate) fn hit(&self, column: u16, row: u16) -> Option<NavRailItem> {
        self.hitboxes
            .iter()
            .find(|(_, rect)| crate::terminal::mouse::contains(*rect, column, row))
            .map(|(item, _)| *item)
    }

    pub(crate) fn hover_at(&mut self, area: Rect, column: u16, row: u16) {
        let layout = layout(area, self.collapsed);
        self.hovered_divider = layout
            .divider
            .is_some_and(|rect| crate::terminal::mouse::contains(rect, column, row));
        self.hitboxes = layout.buttons;
        self.hovered_item = self.hit(column, row);
    }

    pub(crate) fn clear_hover(&mut self) {
        self.hovered_item = None;
        self.hovered_divider = false;
    }

    /// Grabbing the divider starts a snap drag; a double click toggles
    /// the rail instead. Returns true when the press landed on it.
    pub(crate) fn start_divider_drag(&mut self, area: Rect, column: u16, row: u16) -> bool {
        self.start_divider_drag_at(area, column, row, now_millis())
    }

    pub(crate) fn start_divider_drag_at(
        &mut self,
        area: Rect,
        column: u16,
        row: u16,
        now: u64,
    ) -> bool {
        let on_divider = layout(area, self.collapsed)
            .divider
            .is_some_and(|rect| crate::terminal::mouse::contains(rect, column, row));
        if !on_divider {
            return false;
        }
        let double = self
            .last_divider_click
            .is_some_and(|at| now.saturating_sub(at) <= DIVIDER_DOUBLE_CLICK_MS);
        self.last_divider_click = Some(now);
        if double {
            // honey: the second press wins over the drag the first one
            // started, mirroring the pane dividers' double-click reset.
            self.last_divider_click = None;
            self.dragging = false;
            self.toggle();
            return true;
        }
        self.dragging = true;
        true
    }

    /// Flips the rail between expanded and collapsed, dropping hover
    /// state that belongs to the old layout.
    pub(crate) fn toggle(&mut self) {
        self.collapsed = !self.collapsed;
        self.clear_hover();
        self.hitboxes.clear();
    }

    /// Snap to expanded or collapsed from the pointer column. The rail
    /// has one width, so the drag can only pick one of the two states.
    pub(crate) fn drag_to(&mut self, area: Rect, column: u16) {
        if !self.dragging {
            return;
        }
        let collapsed = column < area.x.saturating_add(FULL_WIDTH / 2);
        if collapsed != self.collapsed {
            // honey: a real drag is not a click, so a quick follow-up
            // press must not read as a double click.
            self.collapsed = collapsed;
            self.last_divider_click = None;
            self.clear_hover();
            self.hitboxes.clear();
        }
    }

    pub(crate) fn end_drag(&mut self) {
        self.dragging = false;
    }
}

pub(crate) struct RailLayout {
    pub(crate) buttons: Vec<(NavRailItem, Rect)>,
    pub(crate) divider: Option<Rect>,
}

/// Workspace rect after the rail takes its columns. Collapsed hands the
/// full width back to the panes.
pub(crate) fn workspace(area: Rect, collapsed: bool) -> Rect {
    if collapsed {
        area
    } else {
        Rect {
            x: area.x.saturating_add(FULL_WIDTH),
            width: area.width.saturating_sub(FULL_WIDTH),
            ..area
        }
    }
}

/// Every button rect plus the divider. Buttons that do not fit or would
/// overlap on a short terminal are dropped whole.
pub(crate) fn layout(area: Rect, collapsed: bool) -> RailLayout {
    if collapsed {
        // honey: like the right pane parked at zero, the divider stays
        // on the edge so it can be dragged back open.
        let divider =
            (area.width > 0).then(|| Rect::new(area.x, area.y, DIVIDER_WIDTH, area.height));
        return RailLayout {
            buttons: Vec::new(),
            divider,
        };
    }

    let mut buttons = Vec::new();
    let content_top = area.y.saturating_add(1);
    let content_bottom = area.y.saturating_add(area.height).saturating_sub(1);
    let x = area.x.saturating_add(1);
    let button = |y: u16| Rect::new(x, y, BUTTON_WIDTH, BUTTON_HEIGHT);

    let home_y = content_top.saturating_add(1);
    push_if_fits(
        &mut buttons,
        (NavRailItem::Home, button(home_y)),
        content_bottom,
    );
    let library_y = home_y.saturating_add(BUTTON_HEIGHT + BUTTON_GAP);
    push_if_fits(
        &mut buttons,
        (NavRailItem::Library, button(library_y)),
        content_bottom,
    );

    let settings_y = content_bottom.saturating_sub(BUTTON_HEIGHT);
    let palette_y = settings_y.saturating_sub(BUTTON_HEIGHT + BUTTON_GAP);
    push_if_fits(
        &mut buttons,
        (NavRailItem::CommandPalette, button(palette_y)),
        content_bottom,
    );
    push_if_fits(
        &mut buttons,
        (NavRailItem::Settings, button(settings_y)),
        content_bottom,
    );

    let divider = (area.width >= FULL_WIDTH).then(|| {
        Rect::new(
            area.x.saturating_add(RAIL_WIDTH),
            area.y,
            DIVIDER_WIDTH,
            area.height,
        )
    });
    RailLayout { buttons, divider }
}

fn push_if_fits(
    buttons: &mut Vec<(NavRailItem, Rect)>,
    button: (NavRailItem, Rect),
    content_bottom: u16,
) {
    let rect = button.1;
    if rect.y.saturating_add(rect.height) > content_bottom {
        return;
    }
    if buttons.iter().any(|(_, other)| rects_overlap(other, &rect)) {
        return;
    }
    buttons.push(button);
}

fn rects_overlap(left: &Rect, right: &Rect) -> bool {
    left.x < right.right()
        && right.x < left.right()
        && left.y < right.bottom()
        && right.y < left.bottom()
}

pub(crate) fn render(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &mut NavRailState,
    icons: Option<&RailIcons>,
    dimmed: bool,
) {
    let layout = layout(area, state.collapsed);
    state.hitboxes = layout.buttons.clone();
    for (item, rect) in &layout.buttons {
        let icon = icons.map(|icons| icons.for_item(*item));
        render_button(frame, theme, state, *item, *rect, icon, dimmed);
    }

    if let Some(divider) = layout.divider {
        let color = if state.dragging || state.hovered_divider {
            DIVIDER_HOVER_COLOR
        } else {
            theme.border
        };
        let glyphs: Vec<Line> = (0..divider.height).map(|_| Line::from("│")).collect();
        frame.render_widget(
            Paragraph::new(glyphs).style(Style::default().fg(color)),
            divider,
        );
    }
}

fn render_button(
    frame: &mut Frame,
    theme: &Theme,
    state: &NavRailState,
    item: NavRailItem,
    rect: Rect,
    variants: Option<&IconVariants>,
    dimmed: bool,
) {
    let active = matches!(
        (state.active_page, item),
        (ActivePage::Home, NavRailItem::Home) | (ActivePage::Library, NavRailItem::Library)
    );
    let hovered = state.hovered_item == Some(item);
    let lit = active || hovered;
    // honey: the active page owns a filled 4x2 pill matching the icon
    // canvas exactly. The rail margins stay as equal gutters on both
    // sides instead of the fill running to one edge and gapping the
    // other.
    if active {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.surface)),
            rect,
        );
    }
    if let Some(variants) = variants {
        // honey: the artwork is centered in a transparent, double-sized
        // canvas at load time, so the full 4x2 button area keeps the
        // icon's aspect while showing it at half size.
        let set = variants.select(dimmed);
        let protocol = if active {
            &set.active
        } else if hovered {
            &set.bright
        } else {
            &set.idle
        };
        frame.render_widget(Image::new(protocol).allow_clipping(true), rect);
        return;
    }
    // honey: headless sessions keep the bold glyph fallback.
    let fg = if lit { theme.text } else { theme.text_muted };
    let bg = if active { theme.surface } else { Color::Reset };
    let pill = Span::styled(
        format!(" {} ", icon(item)),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    );
    frame.render_widget(
        Paragraph::new(Line::from(pill)),
        Rect::new(rect.x, rect.y, rect.width.min(3), 1),
    );
}

const fn icon(item: NavRailItem) -> &'static str {
    match item {
        // honey: Nerd Font glyphs; they fill the cell far better than
        // the ASCII fallbacks.
        NavRailItem::Home => "\u{f015}",
        NavRailItem::Library => "\u{f00bb}",
        NavRailItem::CommandPalette => "\u{f120}",
        NavRailItem::Settings => "\u{f013}",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::default::DARK_THEME;
    use ratatui::{Terminal, backend::TestBackend};

    const AREA: Rect = Rect::new(0, 0, 100, 45);

    fn rect_of(layout: &RailLayout, item: NavRailItem) -> Rect {
        layout
            .buttons
            .iter()
            .find(|(candidate, _)| *candidate == item)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("{item:?} missing"))
    }

    fn draw(state: &mut NavRailState) -> Terminal<TestBackend> {
        draw_with(state, None, false)
    }

    fn draw_with(
        state: &mut NavRailState,
        icons: Option<&RailIcons>,
        dimmed: bool,
    ) -> Terminal<TestBackend> {
        let mut terminal =
            Terminal::new(TestBackend::new(AREA.width, AREA.height)).expect("test terminal");
        terminal
            .draw(|frame| render(frame, AREA, &DARK_THEME, state, icons, dimmed))
            .expect("test draw");
        terminal
    }

    fn bg_at(terminal: &Terminal<TestBackend>, x: u16, y: u16) -> ratatui::style::Color {
        terminal.backend().buffer().content[(y * AREA.width + x) as usize].bg
    }

    #[test]
    fn expanded_layout_stacks_top_and_bottom_groups() {
        let layout = layout(AREA, false);
        assert_eq!(rect_of(&layout, NavRailItem::Home), Rect::new(1, 2, 4, 2));
        assert_eq!(
            rect_of(&layout, NavRailItem::Library),
            Rect::new(1, 5, 4, 2)
        );
        assert_eq!(
            rect_of(&layout, NavRailItem::CommandPalette),
            Rect::new(1, 39, 4, 2)
        );
        assert_eq!(
            rect_of(&layout, NavRailItem::Settings),
            Rect::new(1, 42, 4, 2)
        );
        assert!(
            layout
                .buttons
                .iter()
                .all(|(_, rect)| { rect.width == BUTTON_WIDTH && rect.height == BUTTON_HEIGHT })
        );
        assert_eq!(layout.divider, Some(Rect::new(6, 0, 1, 45)));
    }

    #[test]
    fn collapsed_layout_leaves_the_edge_divider() {
        let layout = layout(AREA, true);
        assert!(layout.buttons.is_empty());
        assert_eq!(layout.divider, Some(Rect::new(0, 0, 1, 45)));
    }

    #[test]
    fn workspace_width_tracks_the_rail() {
        assert_eq!(workspace(AREA, false), Rect::new(7, 0, 93, 45));
        assert_eq!(workspace(AREA, true), AREA);
    }

    #[test]
    fn hover_tracks_buttons_and_the_divider() {
        let mut state = NavRailState::default();
        state.hover_at(AREA, 2, 5);
        assert_eq!(state.hovered_item, Some(NavRailItem::Library));
        assert!(!state.hovered_divider());
        state.hover_at(AREA, 6, 20);
        assert_eq!(state.hovered_item, None);
        assert!(state.hovered_divider());
        state.collapsed = true;
        state.hover_at(AREA, 0, 20);
        assert!(state.hovered_divider(), "edge divider stays grabbable");
        state.clear_hover();
        assert_eq!(state.hovered_item, None);
        assert!(!state.hovered_divider());
    }

    #[test]
    fn divider_drag_snaps_between_the_two_widths() {
        let mut state = NavRailState::default();
        assert!(state.start_divider_drag(AREA, 6, 20));
        state.drag_to(AREA, 1);
        assert!(state.collapsed);
        state.drag_to(AREA, 20);
        assert!(!state.collapsed);
        state.end_drag();
        assert!(!state.dragging());
        state.drag_to(AREA, 1);
        assert!(!state.collapsed, "drag needs a fresh grab");

        state.collapsed = true;
        assert!(state.start_divider_drag(AREA, 0, 20));
        state.drag_to(AREA, 20);
        assert!(!state.collapsed, "collapsed edge drags back open");
    }

    #[test]
    fn double_clicking_the_divider_toggles_the_rail() {
        let mut state = NavRailState::default();
        assert!(state.start_divider_drag_at(AREA, 6, 20, 1_000));
        assert!(state.dragging());
        state.end_drag();

        assert!(state.start_divider_drag_at(AREA, 6, 20, 1_100));
        assert!(state.collapsed);
        assert!(!state.dragging(), "double click must not start a drag");
        state.end_drag();

        assert!(state.start_divider_drag_at(AREA, 0, 20, 1_200));
        state.end_drag();
        assert!(state.start_divider_drag_at(AREA, 0, 20, 1_300));
        assert!(!state.collapsed);
        assert!(!state.dragging());
    }

    #[test]
    fn slow_clicks_on_the_divider_start_drags() {
        let mut state = NavRailState::default();
        assert!(state.start_divider_drag_at(AREA, 6, 20, 1_000));
        state.end_drag();
        assert!(state.start_divider_drag_at(AREA, 6, 20, 1_400));
        assert!(!state.collapsed);
        assert!(state.dragging());
    }

    #[test]
    fn rail_opens_on_home() {
        assert_eq!(NavRailState::default().active_page, ActivePage::Home);
    }

    #[test]
    fn active_page_paints_only_its_rail_pill() {
        let mut state = NavRailState::default();
        let terminal = draw(&mut state);
        for (x, y) in [(1, 2), (2, 2), (4, 3)] {
            assert_eq!(
                bg_at(&terminal, x, y),
                DARK_THEME.surface,
                "active Home background missing at {x},{y}"
            );
        }
        for (x, y) in [(1, 5), (2, 5), (1, 39), (1, 42), (0, 2), (5, 2)] {
            assert_eq!(
                bg_at(&terminal, x, y),
                ratatui::style::Color::Reset,
                "inactive icon painted a background at {x},{y}"
            );
        }
    }

    #[test]
    fn hovering_a_second_icon_leaves_the_active_background_alone() {
        let mut state = NavRailState {
            hovered_item: Some(NavRailItem::CommandPalette),
            ..NavRailState::default()
        };
        let terminal = draw(&mut state);
        assert_eq!(bg_at(&terminal, 1, 39), ratatui::style::Color::Reset);
        assert_eq!(bg_at(&terminal, 1, 2), DARK_THEME.surface);
    }

    #[test]
    fn active_glyph_fallback_also_paints_the_fill() {
        let mut state = NavRailState::default();
        let terminal = draw(&mut state);
        assert_eq!(bg_at(&terminal, 1, 2), DARK_THEME.surface);
        assert_eq!(bg_at(&terminal, 2, 3), DARK_THEME.surface);
    }

    #[test]
    fn active_fill_never_spills_past_the_button_margins() {
        use ratatui_image::picker::Picker;

        let icons = RailIcons::load(&Picker::halfblocks(), &DARK_THEME).expect("icons load");
        let mut state = NavRailState::default();
        let terminal = draw_with(&mut state, Some(&icons), false);
        for x in [0, 5] {
            assert_eq!(
                bg_at(&terminal, x, 2),
                ratatui::style::Color::Reset,
                "fill spilled outside the button at {x}"
            );
        }
    }

    #[test]
    fn graphics_replace_every_glyph_fallback() {
        use ratatui_image::picker::Picker;

        let icons = RailIcons::load(&Picker::halfblocks(), &DARK_THEME).expect("icons load");
        let mut state = NavRailState::default();
        let terminal = draw_with(&mut state, Some(&icons), false);
        for (item, glyph, y) in [
            (NavRailItem::Home, '\u{f015}', 2u16),
            (NavRailItem::Library, '\u{f00bb}', 5),
            (NavRailItem::CommandPalette, '\u{f120}', 39),
            (NavRailItem::Settings, '\u{f013}', 42),
        ] {
            let mut painted = String::new();
            for row in y..y + 2 {
                for x in 1..=4 {
                    painted.push_str(
                        terminal.backend().buffer().content[(row * AREA.width + x) as usize]
                            .symbol(),
                    );
                }
            }
            assert!(!painted.contains(glyph), "{item:?} fell back: {painted:?}");
            assert!(!painted.trim().is_empty(), "{item:?} painted nothing");
        }
    }

    #[test]
    fn sixel_renders_every_button() {
        use ratatui_image::picker::{Picker, ProtocolType};

        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Sixel);
        let icons = RailIcons::load(&picker, &DARK_THEME).expect("icons");
        let mut state = NavRailState::default();
        let terminal = draw_with(&mut state, Some(&icons), false);
        for (item, y) in [
            (NavRailItem::Home, 2u16),
            (NavRailItem::Library, 5),
            (NavRailItem::CommandPalette, 39),
            (NavRailItem::Settings, 42),
        ] {
            let cell = &terminal.backend().buffer().content[(y * AREA.width + 1) as usize];
            assert!(
                cell.symbol().starts_with('\u{1b}'),
                "{item:?} image missing"
            );
        }
    }

    #[test]
    fn dimmed_graphics_swap_in_the_prebaked_scrim_variants() {
        use ratatui_image::picker::{Picker, ProtocolType};

        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Sixel);
        let icons = RailIcons::load(&picker, &DARK_THEME).expect("icons");
        assert!(icons.home.dimmed.is_some(), "graphics need a scrim set");

        let library_payload = |dimmed: bool| {
            let mut state = NavRailState::default();
            let terminal = draw_with(&mut state, Some(&icons), dimmed);
            terminal.backend().buffer().content[(5 * AREA.width + 1) as usize]
                .symbol()
                .to_string()
        };
        assert_ne!(
            library_payload(false),
            library_payload(true),
            "dimmed graphics must paint different pixels"
        );
    }

    #[test]
    fn dimmed_halfblocks_keep_the_same_cells() {
        use ratatui_image::picker::Picker;

        let icons = RailIcons::load(&Picker::halfblocks(), &DARK_THEME).expect("icons load");
        assert!(icons.home.dimmed.is_none(), "halfblocks dim as cells");

        let content = |dimmed: bool| {
            let mut state = NavRailState::default();
            let terminal = draw_with(&mut state, Some(&icons), dimmed);
            terminal.backend().buffer().content.clone()
        };
        assert_eq!(content(false), content(true));
    }

    #[test]
    fn icon_pills_are_one_bold_span() {
        let mut state = NavRailState::default();
        let terminal = draw(&mut state);
        let cell = &terminal.backend().buffer().content[5 * AREA.width as usize + 2];
        assert_eq!(cell.symbol(), "\u{f00bb}");
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert_eq!(cell.bg, ratatui::style::Color::Reset);
        assert_eq!(cell.fg, DARK_THEME.text_muted);
    }

    #[test]
    fn collapse_keeps_the_edge_divider_and_no_buttons() {
        let mut state = NavRailState {
            collapsed: true,
            ..NavRailState::default()
        };
        let terminal = draw(&mut state);
        assert!(state.collapsed);
        assert!(state.hitboxes.is_empty());
        assert_eq!(
            terminal.backend().buffer().content[20 * AREA.width as usize].symbol(),
            "│"
        );
    }
}
