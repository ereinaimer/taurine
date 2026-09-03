// Licensed under the Aimer Software License (ASL).
// See LICENSE for details.

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use taurine_core::ai::AiProvider;

use super::{AddField, AddModalState, AiWizardState, EditModelModalState, ModalView};
use crate::overlay::ui::actions::{fill_bg, render_action_buttons_overlay};
use crate::overlay::ui::rows::{
    desc_area, padded, render_desc, row_input, row_key_value, row_password,
};
use crate::theme::builtin::DARK_THEME;
use crate::widgets::library::ButtonSelection;

pub(crate) fn render_ai_wizard(frame: &mut Frame, state: &AiWizardState) {
    fill_bg(frame);
    let inner = padded(frame.area());

    match &state.modal {
        ModalView::None => render_dashboard(frame, inner, state),
        ModalView::Add(add_state) => render_add_view(frame, inner, add_state),
        ModalView::EditModel(edit_state) => render_edit_view(frame, inner, edit_state),
        ModalView::DeleteConfirm(provider) => render_delete_view(frame, inner, *provider),
    }
}

fn render_dashboard(frame: &mut Frame, area: Rect, state: &AiWizardState) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(1), // Subtitle / active provider
            Constraint::Length(1), // Spacing
            Constraint::Min(4),    // Provider list
            Constraint::Length(1), // Status feedback
            Constraint::Length(1), // Footer key hints
        ])
        .split(area);

    let title_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(" AI Configuration").style(title_style),
        sections[0],
    );

    let active_text = match state.providers.iter().find(|p| p.is_active) {
        Some(p) => format!(" active: {} ({})", p.provider.display_name(), p.model),
        None => " no active provider".to_string(),
    };
    frame.render_widget(
        Paragraph::new(active_text).style(Style::default().fg(DARK_THEME.text_muted)),
        sections[1],
    );

    if state.providers.is_empty() {
        let empty_msg = vec![
            Line::styled(
                "  No AI providers configured.",
                Style::default().fg(DARK_THEME.text_muted),
            ),
            Line::styled(
                "  Press n to add a provider.",
                Style::default().fg(DARK_THEME.text_muted),
            ),
        ];
        frame.render_widget(Paragraph::new(empty_msg), sections[3]);
    } else {
        let items: Vec<Line> = state
            .providers
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let is_sel = i == state.selected_index;
                let prefix = if is_sel { "> " } else { "  " };
                let bg = if is_sel {
                    DARK_THEME.surface
                } else {
                    DARK_THEME.background
                };

                let name_style = if is_sel {
                    Style::default()
                        .fg(DARK_THEME.text)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DARK_THEME.text).bg(bg)
                };

                let model_style = if is_sel {
                    Style::default()
                        .fg(DARK_THEME.text_muted)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DARK_THEME.text_muted).bg(bg)
                };

                let active_tag = if p.is_active { " [active]" } else { "" };
                let tag_style = if is_sel {
                    Style::default()
                        .fg(DARK_THEME.text)
                        .bg(bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DARK_THEME.text_muted).bg(bg)
                };

                Line::from(vec![
                    Span::styled(prefix, name_style),
                    Span::styled(format!("{:<20}", p.provider.display_name()), name_style),
                    Span::styled(format!("{:<30}", p.model), model_style),
                    Span::styled(active_tag, tag_style),
                ])
            })
            .collect();
        frame.render_widget(Paragraph::new(items), sections[3]);
    }

    if let Some((msg, _)) = &state.status_message {
        frame.render_widget(
            Paragraph::new(format!(" {msg}")).style(Style::default().fg(DARK_THEME.text)),
            sections[4],
        );
    }

    let footer_hints = " [j/k] navigate  [n] add  [Enter] edit model  [d] remove  [q] quit";
    frame.render_widget(
        Paragraph::new(footer_hints).style(
            Style::default()
                .fg(DARK_THEME.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[5],
    );
}

fn render_add_view(frame: &mut Frame, area: Rect, state: &AddModalState) {
    let is_custom = state.selected_provider() == AiProvider::Custom;

    let mut constraints = vec![
        Constraint::Length(1), // Title
        Constraint::Length(1), // Spacing
        Constraint::Length(2), // Provider
    ];
    if is_custom {
        constraints.push(Constraint::Length(2)); // Endpoint
    }
    constraints.push(Constraint::Length(2)); // ApiKey
    constraints.push(Constraint::Length(2)); // Model
    constraints.push(Constraint::Length(1)); // Error / feedback
    constraints.push(Constraint::Length(1)); // Action buttons
    constraints.push(Constraint::Length(1)); // Footer hints
    constraints.push(Constraint::Min(0));

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let title_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(" Add AI Provider").style(title_style),
        sections[0],
    );

    let mut idx = 2;

    // 1. Provider row
    let (p_area, p_desc) = desc_area(sections[idx]);
    let p_focused = state.focus == AddField::Provider;
    let provider_label = format!("< {} >", state.selected_provider().display_name());
    row_key_value(frame, p_area, " Provider", &provider_label, p_focused);
    render_desc(frame, p_desc, "use j/k to cycle providers", p_focused);
    idx += 1;

    // 2. Endpoint row (if Custom)
    if is_custom {
        let (ep_area, ep_desc) = desc_area(sections[idx]);
        let ep_focused = state.focus == AddField::Endpoint;
        row_input(
            frame,
            ep_area,
            &state.endpoint,
            state.endpoint.chars().count(),
            ep_focused,
        );
        render_desc(
            frame,
            ep_desc,
            "custom openai-compatible endpoint url",
            ep_focused,
        );
        idx += 1;
    }

    // 3. API Key row
    let (key_area, key_desc) = desc_area(sections[idx]);
    let key_focused = state.focus == AddField::ApiKey;
    let masked_key: String = "*".repeat(state.api_key.chars().count());
    let display_key = if state.api_key.is_empty() && !key_focused {
        "<enter key>".to_string()
    } else {
        masked_key
    };
    row_password(
        frame,
        key_area,
        " API Key",
        &display_key,
        key_focused,
        false,
        false,
    );
    render_desc(
        frame,
        key_desc,
        "api key stored securely in keyring",
        key_focused,
    );
    idx += 1;

    // 4. Model row
    let (m_area, m_desc) = desc_area(sections[idx]);
    let m_focused = state.focus == AddField::Model;
    row_input(
        frame,
        m_area,
        &state.model,
        state.model.chars().count(),
        m_focused,
    );
    render_desc(frame, m_desc, "model identifier for provider", m_focused);
    idx += 1;

    // 5. Error message or feedback
    if let Some(ref err) = state.error_msg {
        frame.render_widget(
            Paragraph::new(format!(" {err}")).style(
                Style::default()
                    .fg(DARK_THEME.text_muted)
                    .add_modifier(Modifier::BOLD),
            ),
            sections[idx],
        );
    }
    idx += 1;

    // 6. Action buttons
    let confirm_focused = state.focus == AddField::Confirm;
    render_action_buttons_overlay(
        frame,
        sections[idx],
        "Cancel",
        "Save",
        confirm_focused,
        ButtonSelection::Confirm,
    );
    idx += 1;

    // 7. Footer hints
    let footer_hints =
        " [Tab/Shift+Tab] cycle fields  [j/k] change provider  [Enter] save  [Esc] cancel";
    frame.render_widget(
        Paragraph::new(footer_hints).style(
            Style::default()
                .fg(DARK_THEME.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[idx],
    );
}

fn render_edit_view(frame: &mut Frame, area: Rect, state: &EditModelModalState) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Provider row
            Constraint::Length(2), // Model input
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Footer hints
            Constraint::Min(0),
        ])
        .split(area);

    let title_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(format!(
            " Configure {} Model",
            state.provider.display_name()
        ))
        .style(title_style),
        sections[0],
    );

    let (p_area, p_desc) = desc_area(sections[2]);
    row_key_value(
        frame,
        p_area,
        " Provider",
        state.provider.display_name(),
        false,
    );
    render_desc(frame, p_desc, "selected provider", false);

    let (m_area, m_desc) = desc_area(sections[3]);
    row_input(
        frame,
        m_area,
        &state.model,
        state.model.chars().count(),
        true,
    );
    render_desc(frame, m_desc, "press Enter to save model name", true);

    frame.render_widget(
        Paragraph::new(" [Enter] save  [Esc] cancel").style(
            Style::default()
                .fg(DARK_THEME.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[5],
    );
}

fn render_delete_view(frame: &mut Frame, area: Rect, provider: AiProvider) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Title
            Constraint::Length(1), // Spacing
            Constraint::Length(2), // Prompt
            Constraint::Length(1), // Spacing
            Constraint::Length(1), // Footer hints
            Constraint::Min(0),
        ])
        .split(area);

    let title_style = Style::default()
        .fg(DARK_THEME.text)
        .add_modifier(Modifier::BOLD);
    frame.render_widget(
        Paragraph::new(" Remove Provider").style(title_style),
        sections[0],
    );

    let prompt = format!(
        " Remove credentials for '{}' from OS keyring? [y/N]",
        provider.display_name()
    );
    frame.render_widget(
        Paragraph::new(prompt).style(Style::default().fg(DARK_THEME.text)),
        sections[2],
    );

    frame.render_widget(
        Paragraph::new(" [y] confirm delete  [n/Esc] cancel").style(
            Style::default()
                .fg(DARK_THEME.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        sections[4],
    );
}
