use ratatui::{Frame, layout::Rect};

use crate::theme::Theme;
use crate::widgets::field::TextField;
use crate::widgets::util;

pub fn render_library_search_bar(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    field: &TextField,
    is_active: bool,
) {
    util::render_search_block(frame, area, theme, field, is_active, "Search triggers…");
}
