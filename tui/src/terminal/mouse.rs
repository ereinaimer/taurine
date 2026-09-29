use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Rectangles of the main TUI chrome, computed from the terminal area.
/// Single source of truth shared by rendering (`lib.rs` draw closure) and
/// mouse hit-testing so clicks land where the widgets are drawn.
pub(crate) struct FrameLayout {
    pub(crate) header: Rect,
    pub(crate) footer: Rect,
    pub(crate) nav: Option<Rect>,
    pub(crate) page: Rect,
}

pub(crate) fn frame_layout(area: Rect, nav_visible: bool) -> FrameLayout {
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
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

    if nav_visible {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .split(sections[2]);
        FrameLayout {
            header: sections[0],
            footer: sections[4],
            nav: Some(body[0]),
            page: body[2],
        }
    } else {
        FrameLayout {
            header: sections[0],
            footer: sections[4],
            nav: None,
            page: sections[2],
        }
    }
}

/// Content area inside the bordered page block (1-cell border each side).
pub(crate) fn page_inner(page: Rect) -> Rect {
    Rect {
        x: page.x.saturating_add(1),
        y: page.y.saturating_add(1),
        width: page.width.saturating_sub(2),
        height: page.height.saturating_sub(2),
    }
}

pub(crate) const fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_draw_chain_on_standard_terminal() {
        let layout = frame_layout(Rect::new(0, 0, 100, 30), true);

        assert_eq!(layout.header, Rect::new(2, 1, 96, 1));
        assert_eq!(layout.nav, Some(Rect::new(2, 3, 22, 24)));
        assert_eq!(layout.page, Rect::new(25, 3, 73, 24));
        assert_eq!(layout.footer, Rect::new(2, 28, 96, 1));
    }

    #[test]
    fn hidden_nav_gives_full_width_page() {
        let layout = frame_layout(Rect::new(0, 0, 100, 30), false);

        assert_eq!(layout.nav, None);
        assert_eq!(layout.page, Rect::new(2, 3, 96, 24));
    }

    #[test]
    fn page_inner_strips_one_cell_border() {
        assert_eq!(
            page_inner(Rect::new(25, 3, 73, 24)),
            Rect::new(26, 4, 71, 22)
        );
    }

    #[test]
    fn contains_rejects_outside_cells() {
        let area = Rect::new(10, 5, 20, 10);
        assert!(contains(area, 10, 5));
        assert!(contains(area, 29, 14));
        assert!(!contains(area, 30, 5));
        assert!(!contains(area, 10, 15));
        assert!(!contains(area, 9, 5));
    }
}
