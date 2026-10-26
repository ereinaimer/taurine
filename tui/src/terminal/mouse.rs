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

pub(crate) const fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

/// Library page stretched back to the full terminal width: the page
/// margins are vertical breathing room, but dividers must travel edge
/// to edge. Panes keep their own inner padding so text never touches
/// the terminal border.
pub(crate) fn library_full_area(page: Rect) -> Rect {
    Rect {
        x: page.x.saturating_sub(2),
        y: page.y,
        width: page.width.saturating_add(4),
        height: page.height,
    }
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
    fn contains_rejects_outside_cells() {
        let area = Rect::new(10, 5, 20, 10);
        assert!(contains(area, 10, 5));
        assert!(contains(area, 29, 14));
        assert!(!contains(area, 30, 5));
        assert!(!contains(area, 10, 15));
        assert!(!contains(area, 9, 5));
    }
}
