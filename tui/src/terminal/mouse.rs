use ratatui::layout::Rect;

/// Rectangle of the main TUI content, computed from the terminal area.
/// Single source of truth shared by rendering (`lib.rs` draw closure) and
/// mouse hit-testing so clicks land where the widgets are drawn.
pub(crate) struct FrameLayout {
    pub(crate) page: Rect,
}

pub(crate) fn frame_layout(area: Rect) -> FrameLayout {
    let page = Rect {
        x: area.x.saturating_add(2),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(4),
        height: area.height.saturating_sub(2),
    };
    FrameLayout { page }
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
        let layout = frame_layout(Rect::new(0, 0, 100, 30));

        assert_eq!(layout.page, Rect::new(2, 1, 96, 28));
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
