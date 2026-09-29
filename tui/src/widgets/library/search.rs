use ratatui::{Frame, layout::Rect};

use crate::theme::Theme;
use crate::widgets::util;

pub fn render_library_search_bar(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    query: &str,
    is_active: bool,
    cursor: usize,
) {
    util::render_search_block(
        frame,
        area,
        theme,
        query,
        is_active,
        cursor,
        "Search triggers…",
    );
}
