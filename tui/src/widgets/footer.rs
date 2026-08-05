use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::{terminal::app::App, theme::Theme};

pub struct FooterWidget<'a> {
    pub theme: &'a Theme,
    pub app: &'a App,
}

impl Widget for FooterWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = footer_text(self.theme, self.app);
        let footer_line = Line::from(vec![Span::styled(
            text,
            Style::default()
                .fg(self.theme.text_muted)
                .add_modifier(Modifier::DIM),
        )]);

        Paragraph::new(footer_line)
            .alignment(ratatui::layout::Alignment::Left)
            .render(area, buf);
    }
}

fn footer_text(_theme: &Theme, app: &App) -> String {
    let nav_hint = if app.nav_visible() {
        "Ctrl+B Hide"
    } else {
        "Ctrl+B Nav"
    };
    let page_footer = match app.active_page() {
        crate::terminal::app::Page::Home => {
            crate::terminal::control::home_footer_label(app.daemon_status())
        }
        crate::terminal::app::Page::Library => app.library_page().footer_text(),
        crate::terminal::app::Page::Settings => app.settings_page().footer_text(),
    };
    if page_footer.is_empty() {
        nav_hint.to_string()
    } else if page_footer.contains("Ctrl+B") {
        page_footer.to_string()
    } else {
        format!("{nav_hint}   {page_footer}")
    }
}
