use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use taurine_core::db::crud::ActivityCategory;
use taurine_core::stats::{HomeStats, RecentActivity};

use crate::theme::Theme;
use crate::widgets::hotkey;
use crate::widgets::library::detail::relative_time;
use crate::widgets::wrap::{cell_width, wrap_line};

pub mod state;

use self::state::{CopyHit, HomePageState, STAT_CARD_COUNT, format_count, format_time_saved};

const PANE_PAD_X: u16 = 2;
const CARD_HEIGHT: u16 = 4;
const CARD_GAP: u16 = 1;
const CARD_MIN_WIDTH: u16 = 16;
const MAX_CARDS_PER_ROW: usize = 2;

/// Date header plus the blank row beneath it.
const GROUP_HEADER_LINES: usize = 2;
/// Trigger line, body line, and the blank row that follows each event.
const ITEM_LINES: usize = 3;
/// Section title plus the blank row beneath it.
const TITLE_LINES: usize = 2;
/// Blank row between the cards and the section title.
const CARDS_GAP_LINES: usize = 1;

const COPY_LABEL: &str = " copy ";

/// Blank cells kept between the left text column and the right-hand
/// clock/button column, so content never runs into either.
const CONTENT_GAP: u16 = 6;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Home is one scrollable document: cards, then the activity feed. The
/// date header of the group under the viewport's top edge sticks there.
pub(crate) fn render_home_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &mut HomePageState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let content = padded(area, PANE_PAD_X, 0);
    if content.width < 4 || content.height == 0 {
        return;
    }
    if let Some(error) = state.load_error() {
        frame.render_widget(
            Paragraph::new(error).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            padded(area, PANE_PAD_X, 1),
        );
        return;
    }

    let cards_height = cards_height(content.width);
    let feed_start =
        cards_height as usize + usize::from(cards_height > 0) * CARDS_GAP_LINES + TITLE_LINES;
    let today = Local::now().date_naive();
    let rows = feed_rows(&state.stats().recent_activity, content.width, today);
    let feed_lines = rows_height(&rows).max(1);
    let viewport = content.height as usize;
    let max_scroll = (feed_start + feed_lines).saturating_sub(viewport);

    let scroll = state.scroll().min(max_scroll);
    state.set_scroll(scroll);
    state.set_page(viewport);

    render_cards(frame, content, theme, state.stats(), cards_height, scroll);
    render_title(frame, content, theme, cards_height, scroll);

    let view = FeedViewport {
        content,
        start: feed_start,
        scroll,
    };
    let mut hits = Vec::new();
    render_feed(frame, theme, state.stats(), &rows, view, &mut hits);
    if scroll > feed_start {
        // honey: the pinned date covers the first feed row, so clicks
        // there must not reach the event underneath it.
        hits.retain(|hit| hit.area.y != content.y);
    }
    state.set_copy_hits(hits);

    render_sticky_date(frame, theme, &rows, view);
}

/// At most two cards share a row; narrower panes stack them.
fn cards_per_row(width: u16) -> usize {
    ((width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).clamp(1, MAX_CARDS_PER_ROW as u16) as usize
}

fn card_width(width: u16, per_row: usize) -> u16 {
    width.saturating_sub(CARD_GAP * (per_row as u16 - 1)) / per_row as u16
}

/// Height of the whole card grid, including the gaps between rows.
fn cards_height(width: u16) -> u16 {
    let per_row = cards_per_row(width);
    if card_width(width, per_row) < 8 {
        return 0;
    }
    let rows = STAT_CARD_COUNT.div_ceil(per_row) as u16;
    rows.saturating_mul(CARD_HEIGHT)
        .saturating_add(rows.saturating_sub(1).saturating_mul(CARD_GAP))
}

struct StatCard {
    label: &'static str,
    value: String,
}

fn stat_cards(stats: &HomeStats) -> [StatCard; STAT_CARD_COUNT] {
    let streak = stats.streak;
    [
        StatCard {
            label: "Time Saved",
            value: format_time_saved(stats.time_saved_ms),
        },
        StatCard {
            label: "Words Spoken",
            value: format_count(stats.words_spoken),
        },
        StatCard {
            label: "Words Expanded",
            value: format_count(stats.words_expanded),
        },
        StatCard {
            label: "Day Streak",
            value: streak_days(streak.current_streak),
        },
    ]
}

fn streak_days(days: u32) -> String {
    if days == 1 {
        "1 day".to_string()
    } else {
        format!("{days} days")
    }
}

fn render_cards(
    frame: &mut Frame,
    content: Rect,
    theme: &Theme,
    stats: &HomeStats,
    cards_height: u16,
    scroll: usize,
) {
    if cards_height == 0 {
        return;
    }
    let per_row = cards_per_row(content.width);
    let width = card_width(content.width, per_row);
    let cards = stat_cards(stats);

    for (index, card) in cards.iter().enumerate() {
        let row = (index / per_row) as u16;
        let column = (index % per_row) as u16;
        let top = row.saturating_mul(CARD_HEIGHT + CARD_GAP);
        let shifted_top = top as i32 - scroll as i32;
        let visible_top = shifted_top.max(0);
        let visible_bottom = (shifted_top + CARD_HEIGHT as i32).min(content.height as i32);
        if visible_bottom <= visible_top {
            continue;
        }
        render_card(
            frame,
            Rect {
                x: content
                    .x
                    .saturating_add(column.saturating_mul(width + CARD_GAP)),
                y: content.y.saturating_add(visible_top as u16),
                width,
                height: (visible_bottom - visible_top) as u16,
            },
            (visible_top - shifted_top) as u16,
            theme,
            card,
        );
    }
}

/// `clipped` rows were scrolled off the card's top, so its label and
/// value lines shift up by that much.
fn render_card(frame: &mut Frame, area: Rect, clipped: u16, theme: &Theme, card: &StatCard) {
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface)),
        area,
    );
    let inner = Rect {
        x: area.x.saturating_add(2),
        y: area.y,
        width: area.width.saturating_sub(4),
        height: area.height,
    };
    if inner.width == 0 {
        return;
    }

    let line = |offset: u16| -> Option<Rect> {
        let row = offset.checked_sub(clipped)?;
        (row < area.height).then(|| Rect {
            x: inner.x,
            y: area.y.saturating_add(row),
            width: inner.width,
            height: 1,
        })
    };
    if let Some(area) = line(1) {
        frame.render_widget(
            Paragraph::new(card.label.to_uppercase()).style(Style::default().fg(theme.text_muted)),
            area,
        );
    }
    if let Some(area) = line(2) {
        frame.render_widget(
            Paragraph::new(card.value.clone())
                .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
            area,
        );
    }
}

fn render_title(frame: &mut Frame, content: Rect, theme: &Theme, cards_height: u16, scroll: usize) {
    let line = cards_height as usize + usize::from(cards_height > 0) * CARDS_GAP_LINES;
    if let Some(area) = visible_line(content, scroll, line) {
        frame.render_widget(
            Paragraph::new("Recent Activity")
                .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
            area,
        );
    }
}

enum FeedRow {
    Date(NaiveDate),
    Item(ItemLayout),
}

struct ItemLayout {
    index: usize,
    kind: ItemKind,
}

enum ItemKind {
    /// Voice dictation: the content itself, wrapped, with no label line.
    Voice(Vec<String>),
    /// Everything else: a trigger line and a one-line body preview.
    Label(String),
}

impl ItemLayout {
    /// Drawn height, including the blank row after the item.
    fn height(&self) -> usize {
        match &self.kind {
            ItemKind::Voice(lines) => lines.len().max(1) + 1,
            ItemKind::Label(_) => ITEM_LINES,
        }
    }
}

/// One entry per drawn block, in document order.
fn feed_rows(activity: &[RecentActivity], width: u16, today: NaiveDate) -> Vec<FeedRow> {
    // honey: wrapped lines are rebuilt every frame; the feed is capped at
    // 30 days, so this stays cheap. Cache by width if it ever grows.
    let mut rows = Vec::with_capacity(activity.len() + 4);
    let mut previous: Option<NaiveDate> = None;
    for (index, event) in activity.iter().enumerate() {
        let at = local_datetime(event.timestamp);
        let date = at.date_naive();
        if previous != Some(date) {
            rows.push(FeedRow::Date(date));
            previous = Some(date);
        }
        let kind = if is_dictation(event) {
            let when = activity_time(at, today);
            let (first, rest) = voice_budgets(width, &when);
            let mut lines = wrap_text(event_body(event), first, rest);
            // The clock owns the first line and the copy button the last,
            // so even an empty or one-line dictation gets a second line
            // for the button and the clock always sits flush right.
            while lines.len() < 2 {
                lines.push(String::new());
            }
            ItemKind::Voice(lines)
        } else {
            ItemKind::Label(event_body(event).to_string())
        };
        rows.push(FeedRow::Item(ItemLayout { index, kind }));
    }
    rows
}

/// Drawn height of the whole feed, headers included.
fn rows_height(rows: &[FeedRow]) -> usize {
    rows.iter()
        .map(|row| match row {
            FeedRow::Date(_) => GROUP_HEADER_LINES,
            FeedRow::Item(item) => item.height(),
        })
        .sum()
}

fn is_dictation(event: &RecentActivity) -> bool {
    event.category == ActivityCategory::Voice
}

/// Line budgets for a dictation. The clock is flush right on the first
/// line, so text stops a gap short of it; later lines clear the copy
/// column the same way.
fn voice_budgets(width: u16, when: &str) -> (usize, usize) {
    let copy = cell_width(COPY_LABEL) as u16;
    let time = cell_width(when) as u16;
    let clock = width.saturating_sub(time.saturating_add(1));
    let first = clock.saturating_sub(CONTENT_GAP);
    let rest = width
        .saturating_sub(copy.saturating_add(1))
        .saturating_sub(CONTENT_GAP);
    (first as usize, rest as usize)
}

/// Wraps text into lines of at most `first` cells on the first line and
/// `rest` on every later line. The shared wrapper keeps words whole; only
/// a word longer than a line is split, so the whole dictation is shown.
fn wrap_text(text: &str, first: usize, rest: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for source in text.lines() {
        let budget = if lines.is_empty() { first } else { rest };
        for (row, _) in wrap_line(source, budget, rest) {
            lines.push(row);
        }
    }
    lines
}

/// Where the feed starts in the document and how far the page is scrolled.
#[derive(Debug, Clone, Copy)]
struct FeedViewport {
    content: Rect,
    start: usize,
    scroll: usize,
}

fn render_feed(
    frame: &mut Frame,
    theme: &Theme,
    stats: &HomeStats,
    rows: &[FeedRow],
    view: FeedViewport,
    hits: &mut Vec<CopyHit>,
) {
    let FeedViewport {
        content,
        start,
        scroll,
    } = view;
    let viewport_end = scroll.saturating_add(content.height as usize);
    let today = Local::now().date_naive();

    if rows.is_empty() {
        if let Some(area) = visible_line(content, scroll, start) {
            render_muted(frame, area, theme, "No activity yet.");
        }
        return;
    }

    let mut line = start;
    for row in rows {
        if line >= viewport_end {
            break;
        }
        match row {
            FeedRow::Date(date) => {
                if let Some(area) = visible_line(content, scroll, line) {
                    render_group_header(frame, area, theme, *date, today);
                }
                line += GROUP_HEADER_LINES;
            }
            FeedRow::Item(item) => {
                let event = &stats.recent_activity[item.index];
                let when = activity_time(local_datetime(event.timestamp), today);
                match &item.kind {
                    ItemKind::Voice(lines) => {
                        let last = lines.len().saturating_sub(1);
                        for (offset, text) in lines.iter().enumerate() {
                            if let Some(area) = visible_line(content, scroll, line + offset) {
                                render_voice_line(frame, area, theme, text, &when, offset == 0);
                            }
                        }
                        if let Some(area) = visible_line(content, scroll, line + last)
                            && let Some(button) = copy_button_rect(area)
                        {
                            render_copy_button(frame, button, theme);
                            hits.push(CopyHit {
                                event: item.index,
                                area: button,
                            });
                        }
                        line += item.height();
                    }
                    ItemKind::Label(body) => {
                        if let Some(area) = visible_line(content, scroll, line) {
                            render_trigger(
                                frame,
                                area,
                                theme,
                                event,
                                &when,
                                activity_text_width(area.width, &when),
                            );
                        }
                        if let Some(area) = visible_line(content, scroll, line + 1)
                            && let Some(button) = render_label_body(frame, area, theme, body)
                        {
                            hits.push(CopyHit {
                                event: item.index,
                                area: button,
                            });
                        }
                        line += item.height();
                    }
                }
            }
        }
    }
}

fn visible_line(content: Rect, scroll: usize, line: usize) -> Option<Rect> {
    let offset = line.checked_sub(scroll)?;
    (offset < content.height as usize).then(|| Rect {
        y: content.y.saturating_add(offset as u16),
        height: 1,
        ..content
    })
}

fn local_datetime(timestamp_ms: i64) -> DateTime<Local> {
    Utc.timestamp_millis_opt(timestamp_ms)
        .single()
        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH)
        .with_timezone(&Local)
}

fn render_group_header(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    date: NaiveDate,
    today: NaiveDate,
) {
    let label = if date == today {
        "Today".to_string()
    } else {
        format!(
            "{} {} {}",
            MONTHS[date.month0() as usize],
            date.day(),
            date.year()
        )
    };
    frame.render_widget(
        Paragraph::new(label).style(
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::BOLD),
        ),
        area,
    );
}

/// Pins the date of the group at the viewport's top edge, so the feed
/// always says which day it is showing while the cards scroll away.
fn render_sticky_date(frame: &mut Frame, theme: &Theme, rows: &[FeedRow], view: FeedViewport) {
    let FeedViewport {
        content,
        start,
        scroll,
    } = view;
    if scroll <= start {
        return;
    }
    let mut line = start;
    let mut sticky = None;
    for row in rows {
        if line > scroll {
            break;
        }
        match row {
            FeedRow::Date(date) => {
                if line < scroll {
                    sticky = Some(*date);
                }
                line += GROUP_HEADER_LINES;
            }
            FeedRow::Item(item) => line += item.height(),
        }
    }
    let Some(date) = sticky else {
        return;
    };
    let area = Rect {
        y: content.y,
        height: 1,
        ..content
    };
    // honey: the header floats over the feed, so it clears its own row
    // first; otherwise the line underneath would show through the gaps.
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );
    render_group_header(frame, area, theme, date, Local::now().date_naive());
}

/// Today's events read as relative times; older groups carry the clock
/// time under their date header.
fn activity_time(at: DateTime<Local>, today: NaiveDate) -> String {
    if at.date_naive() == today {
        relative_time(at.timestamp()).unwrap_or_else(|| clock_time(at))
    } else {
        clock_time(at)
    }
}

fn clock_time(at: DateTime<Local>) -> String {
    let (is_pm, hour) = at.hour12();
    format!(
        "{hour}:{:02} {}",
        at.minute(),
        if is_pm { "PM" } else { "AM" }
    )
}

/// Text stops a gap before the right-aligned time, so nothing runs
/// underneath it.
fn activity_text_width(width: u16, when: &str) -> u16 {
    let when_width = cell_width(when) as u16;
    width
        .saturating_sub(when_width.saturating_add(1))
        .saturating_sub(CONTENT_GAP)
}

fn render_trigger(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    event: &RecentActivity,
    when: &str,
    text_width: u16,
) {
    let trigger_width = text_width as usize;
    let mut spans = if is_hotkey_activity(event) {
        hotkey::render_hotkey_spans(
            &event.trigger,
            trigger_width,
            theme.keycap.bg,
            theme.text,
            theme.background,
        )
    } else {
        vec![Span::styled(
            truncate(&event.trigger, trigger_width),
            Style::default().fg(theme.text),
        )]
    };
    // Pad to the clock's fixed cell, not to the text budget, so the gap
    // never shifts the clock off the right edge.
    let clock_x = area
        .width
        .saturating_sub(cell_width(when) as u16)
        .saturating_sub(1);
    let used = hotkey::spans_width(&spans);
    spans.push(Span::raw(
        " ".repeat((clock_x as usize).saturating_sub(used)),
    ));
    spans.push(Span::styled(
        when.to_string(),
        Style::default().fg(theme.description),
    ));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// One wrapped dictation line: the clock ends the first line at the same
/// right edge the copy button uses on the last.
fn render_voice_line(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    text: &str,
    when: &str,
    first: bool,
) {
    let time = cell_width(when) as u16;
    let time_x = area.right().saturating_sub(1).saturating_sub(time);
    // Draw into exactly the budget the wrapper used, so a wrapped row is
    // never clipped mid-word.
    let (first_width, rest_width) = voice_budgets(area.width, when);
    let text_width = if first { first_width } else { rest_width } as u16;

    if text_width > 0 {
        frame.render_widget(
            Paragraph::new(text.to_string()).style(Style::default().fg(theme.description)),
            Rect {
                width: text_width,
                ..area
            },
        );
    }
    if first && time > 0 && time_x >= area.x && time_x.saturating_add(time) <= area.right() {
        frame.render_widget(
            Paragraph::new(when.to_string()).style(Style::default().fg(theme.description)),
            Rect {
                x: time_x,
                width: time,
                ..area
            },
        );
    }
}

/// Draws the event's one-line preview and its copy button; returns the
/// button's rect.
fn render_label_body(frame: &mut Frame, area: Rect, theme: &Theme, body: &str) -> Option<Rect> {
    if body.trim().is_empty() || area.width == 0 {
        return None;
    }
    let button = copy_button_rect(area)?;
    let text_width = button.x.saturating_sub(area.x).saturating_sub(CONTENT_GAP);
    if text_width > 0 {
        // honey: content can be multi-line; the preview is one flat line.
        let flat: String = body
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect();
        frame.render_widget(
            Paragraph::new(truncate(flat.trim(), text_width as usize))
                .style(Style::default().fg(theme.description)),
            Rect {
                width: text_width,
                ..area
            },
        );
    }
    render_copy_button(frame, button, theme);
    Some(button)
}

fn copy_button_rect(area: Rect) -> Option<Rect> {
    let width = cell_width(COPY_LABEL) as u16;
    (area.width > width).then(|| Rect {
        x: area.right().saturating_sub(width),
        width,
        ..area
    })
}

fn render_copy_button(frame: &mut Frame, area: Rect, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(COPY_LABEL).style(Style::default().bg(theme.keycap.bg).fg(theme.text)),
        area,
    );
}

/// The text an event produced, with the trigger description as the
/// fallback for events recorded before content capture.
pub(crate) fn event_body(event: &RecentActivity) -> &str {
    match event.content.as_deref() {
        Some(content) if !content.trim().is_empty() => content,
        _ => &event.detail,
    }
}

fn is_hotkey_activity(event: &RecentActivity) -> bool {
    event.category == ActivityCategory::Hotkey || hotkey::is_hotkey_chord(&event.trigger)
}

fn render_muted(frame: &mut Frame, area: Rect, theme: &Theme, text: &str) {
    frame.render_widget(
        Paragraph::new(text.to_string()).style(Style::default().fg(theme.description)),
        Rect { height: 1, ..area },
    );
}

fn padded(area: Rect, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(x),
        y: area.y.saturating_add(y),
        width: area.width.saturating_sub(x.saturating_mul(2)),
        height: area.height.saturating_sub(y),
    }
}

/// Truncate to `width` cells so a wide glyph can never push the line past
/// its column budget.
fn truncate(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if cell_width(text) <= width {
        return text.to_string();
    }
    let budget = width.saturating_sub(1);
    let mut used: usize = 0;
    let mut end = 0;
    for (index, ch) in text.char_indices() {
        let cell = cell_width(&text[index..index + ch.len_utf8()]);
        if used.saturating_add(cell) > budget {
            break;
        }
        used += cell;
        end = index + ch.len_utf8();
    }
    let mut out = text[..end].to_string();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    use taurine_core::db::crud::StreakInfo;

    use super::*;
    use crate::theme::default::DARK_THEME;
    use crate::widgets::library;

    fn sample_state() -> HomePageState {
        let mut state = HomePageState::default();
        state.replace_stats(HomeStats {
            words_spoken: 12_400,
            words_expanded: 48_200,
            time_saved_ms: 8_040_000,
            expansions_run: 1_204,
            streak: StreakInfo {
                current_streak: 7,
                longest_streak: 21,
            },
            recent_activity: vec![RecentActivity {
                timestamp: library::now_millis() as i64 - 120_000,
                category: ActivityCategory::Text,
                trigger: "gs".to_string(),
                detail: "Open taurine tui".to_string(),
                content: Some("tautui --home".to_string()),
                app: "Windowsterminal".to_string(),
                word_count: 42,
            }],
            most_used_words: Vec::new(),
            most_used_hotkeys: Vec::new(),
            top_apps: Vec::new(),
        });
        state
    }

    fn render_buffer(state: &mut HomePageState, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_home_content(frame, area, &DARK_THEME, state))
            .expect("test draw");
        terminal.backend().buffer().clone()
    }

    fn draw(state: &mut HomePageState, width: u16, height: u16) -> String {
        let buffer = render_buffer(state, width, height);
        let mut text = String::new();
        for y in 0..height {
            for x in 0..width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    /// Local noon today, so grouping never straddles midnight.
    fn today_noon_millis() -> i64 {
        use chrono::{Local, TimeZone};

        let noon = Local::now()
            .date_naive()
            .and_hms_opt(12, 0, 0)
            .expect("valid noon");
        Local
            .from_local_datetime(&noon)
            .single()
            .expect("noon is unambiguous")
            .timestamp_millis()
    }

    fn activity_event(timestamp: i64, trigger: &str, body: &str) -> RecentActivity {
        RecentActivity {
            timestamp,
            category: ActivityCategory::Text,
            trigger: trigger.to_string(),
            detail: String::new(),
            content: Some(body.to_string()),
            app: String::new(),
            word_count: 0,
        }
    }

    fn voice_event(timestamp: i64, body: &str) -> RecentActivity {
        RecentActivity {
            timestamp,
            category: ActivityCategory::Voice,
            trigger: "Dictation".to_string(),
            detail: String::new(),
            content: Some(body.to_string()),
            app: String::new(),
            word_count: 0,
        }
    }

    fn state_with_activity(activity: Vec<RecentActivity>) -> HomePageState {
        let mut state = HomePageState::default();
        state.replace_stats(HomeStats {
            recent_activity: activity,
            ..HomeStats::default()
        });
        state
    }

    /// Content pane the page draws into, and the first feed line inside it.
    fn content_area(width: u16, height: u16) -> (Rect, usize) {
        let content = padded(Rect::new(0, 0, width, height), PANE_PAD_X, 0);
        let cards = cards_height(content.width);
        let feed_start = cards as usize + usize::from(cards > 0) * CARDS_GAP_LINES + TITLE_LINES;
        (content, feed_start)
    }

    #[test]
    fn home_page_paints_no_blue() {
        let mut state = sample_state();
        let buffer = render_buffer(&mut state, 120, 40);
        for y in 0..40 {
            for x in 0..120 {
                let fg = buffer[(x, y)].fg;
                assert_ne!(fg, DARK_THEME.primary, "blue value at {x},{y}");
                assert_ne!(fg, DARK_THEME.accent, "blue accent at {x},{y}");
            }
        }
    }

    #[test]
    fn home_renders_cards_and_activity_without_most_used() {
        let mut state = sample_state();
        let text = draw(&mut state, 120, 40);

        assert!(text.contains("TIME SAVED"));
        assert!(text.contains("2h 14m"));
        assert!(text.contains("WORDS SPOKEN"));
        assert!(text.contains("12.4k"));
        assert!(text.contains("Recent Activity"));
        assert!(text.contains("Today"));
        assert!(text.contains("gs"));
        assert!(text.contains("tautui --home"), "activity body missing");
        assert!(
            !text.contains("Open taurine tui"),
            "the stored content wins over the description"
        );
        assert!(!text.contains("Most Used Triggers"), "section removed");
        assert!(!text.contains("Windowsterminal"), "source app is hidden");
        assert!(!text.contains("42w"), "word count removed from activity");
        assert!(text.contains("2m ago"));
        for border in ["┌", "┐", "└", "┘", "─"] {
            assert!(!text.contains(border), "home page draws no borders");
        }
        assert!(!text.contains("█"), "no usage bars");
        assert!(!text.contains("●"), "no carousel dots");
    }

    #[test]
    fn activity_rows_have_no_selection_background() {
        let mut state = sample_state();
        let (_, feed_start) = content_area(120, 40);
        let buffer = render_buffer(&mut state, 120, 40);

        for y in feed_start as u16..40 {
            for x in 0..120 {
                assert_ne!(
                    buffer[(x, y)].bg,
                    DARK_THEME.surface,
                    "highlighted activity cell at {x},{y}"
                );
            }
        }
    }

    #[test]
    fn activity_items_have_a_blank_row_between_them() {
        let noon = today_noon_millis();
        let mut state = state_with_activity(vec![
            activity_event(noon + 1_000, "aa", "first body"),
            activity_event(noon, "zz", "second body"),
        ]);

        let text = draw(&mut state, 120, 40);
        let (content, feed_start) = content_area(120, 40);
        let first_trigger = content.y as usize + feed_start + GROUP_HEADER_LINES;
        let lines: Vec<&str> = text.lines().collect();
        let blank = |row: usize| lines[row].trim().is_empty();

        assert!(!blank(first_trigger), "first trigger missing");
        assert!(!lines[first_trigger].contains("first body"));
        assert!(
            lines[first_trigger + 1].contains("first body"),
            "body missing"
        );
        assert!(blank(first_trigger + 2), "gap row is not blank");
        assert!(
            lines[first_trigger + 3].contains("zz"),
            "second trigger missing"
        );
        assert!(lines[first_trigger + 4].contains("second body"));
    }

    #[test]
    fn older_activity_is_grouped_under_its_date() {
        use chrono::{Local, TimeZone};

        let at = Local
            .with_ymd_and_hms(2024, 10, 20, 11, 30, 0)
            .single()
            .expect("local time");
        let mut state = state_with_activity(vec![activity_event(
            at.timestamp_millis(),
            "tautui",
            "open taurine tui",
        )]);
        let text = draw(&mut state, 120, 40);

        assert!(text.contains("October 20 2024"), "date header missing");
        assert!(text.contains("11:30 AM"), "clock time missing");
        assert!(!text.contains("Today"), "old group is not today");
        assert!(text.contains("open taurine tui"));
    }

    #[test]
    fn body_falls_back_to_the_description_when_content_is_missing() {
        let mut event = activity_event(today_noon_millis(), "gs", "");
        event.content = None;
        event.detail = "Open taurine tui".to_string();
        let mut state = state_with_activity(vec![event]);
        let text = draw(&mut state, 120, 40);

        assert!(text.contains("Open taurine tui"));
    }

    #[test]
    fn label_body_truncates_with_an_ellipsis() {
        let mut state = state_with_activity(vec![activity_event(
            today_noon_millis(),
            "gs",
            &"a long description that will not fit in the pane ".repeat(8),
        )]);
        let text = draw(&mut state, 80, 30);

        assert!(text.contains('…'), "long body is truncated");
    }

    #[test]
    fn body_stops_before_the_copy_button() {
        let mut state = state_with_activity(vec![activity_event(
            today_noon_millis(),
            "gs",
            &"a long description ".repeat(40),
        )]);
        let buffer = render_buffer(&mut state, 120, 40);

        let (content, feed_start) = content_area(120, 40);
        let body_row = content.y + (feed_start + GROUP_HEADER_LINES + 1) as u16;
        let button_x = content.right() - cell_width(COPY_LABEL) as u16;

        let line: String = (content.x..button_x)
            .map(|x| buffer[(x, body_row)].symbol())
            .collect();
        assert!(line.trim_end().ends_with('…'), "body truncated: {line}");
        let button: String = (button_x..content.right())
            .map(|x| buffer[(x, body_row)].symbol())
            .collect();
        assert_eq!(button, COPY_LABEL, "copy button keeps its column");
    }

    #[test]
    fn wide_body_stays_left_of_the_copy_button() {
        let mut state = state_with_activity(vec![activity_event(
            today_noon_millis(),
            "gs",
            &"描述文字内容".repeat(30),
        )]);
        let buffer = render_buffer(&mut state, 120, 40);

        let (content, feed_start) = content_area(120, 40);
        let body_row = content.y + (feed_start + GROUP_HEADER_LINES + 1) as u16;
        let button_x = content.right() - cell_width(COPY_LABEL) as u16;
        let button: String = (button_x..content.right())
            .map(|x| buffer[(x, body_row)].symbol())
            .collect();
        assert_eq!(button, COPY_LABEL);
        assert!(
            (content.x..button_x).any(|x| buffer[(x, body_row)].symbol() == "…"),
            "wide body is truncated before the button"
        );
    }

    #[test]
    fn voice_dictation_shows_the_content_without_a_label() {
        let at = today_noon_millis();
        let when = activity_time(local_datetime(at), Local::now().date_naive());
        let mut state = state_with_activity(vec![voice_event(
            at,
            "remind me to call mom tomorrow morning",
        )]);
        let text = draw(&mut state, 120, 40);

        assert!(!text.contains("Dictation"), "voice events carry no label");
        assert!(text.contains("remind me to call mom tomorrow morning"));
        assert!(text.contains(&when), "clock time missing: {when}");
        assert!(text.contains(" copy "), "copy button missing");
    }

    #[test]
    fn voice_dictation_wraps_the_whole_text() {
        let at = today_noon_millis();
        let when = activity_time(local_datetime(at), Local::now().date_naive());
        let content = "the quick brown fox jumps over the lazy dog and keeps running through \
                       the field until it reaches the river bank where it stops";
        let mut state = state_with_activity(vec![voice_event(at, content)]);
        let text = draw(&mut state, 60, 40);

        assert!(!text.contains('…'), "dictation is never truncated");
        assert!(text.contains("river bank"), "tail missing:\n{text}");

        let (content_area, feed_start) = content_area(60, 40);
        let (first, rest) = voice_budgets(content_area.width, &when);
        let wrapped = wrap_text(content, first, rest);
        assert!(wrapped.len() > 1, "dictation wraps onto more lines");
        assert_eq!(wrapped.join(" "), content, "every word survives wrapping");

        let rows: Vec<&str> = text.lines().collect();
        let first_row = feed_start + GROUP_HEADER_LINES;
        assert!(
            rows[first_row].contains(&when),
            "clock rides the first line: {:?}",
            rows[first_row]
        );
        assert!(
            rows[first_row].contains("quick"),
            "dictation starts on the first line: {:?}",
            rows[first_row]
        );
        let last_row = first_row + wrapped.len() - 1;
        assert_eq!(
            rows[last_row].matches(COPY_LABEL).count(),
            1,
            "copy button follows the last line: {:?}",
            rows[last_row]
        );
    }

    #[test]
    fn voice_copy_button_clears_the_text_column() {
        let content = "a very long dictation that has to wrap many times so the copy button \
                       sits on the final line of the item without covering any words";
        let mut state = state_with_activity(vec![voice_event(today_noon_millis(), content)]);
        let buffer = render_buffer(&mut state, 60, 40);
        let (content_area, _) = content_area(60, 40);

        // Every visible copy button ends at the content's right edge.
        let button_x = content_area.right() - cell_width(COPY_LABEL) as u16;
        let mut seen = 0;
        for y in 0..40u16 {
            let button: String = (button_x..content_area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if button == COPY_LABEL {
                seen += 1;
                let text: String = (content_area.x..button_x)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                assert!(
                    !text.contains("copy"),
                    "text runs under the copy button at row {y}"
                );
            }
        }
        assert_eq!(seen, 1, "exactly one copy button per dictation");
    }

    fn row_text(buffer: &Buffer, content: Rect, y: u16) -> String {
        (content.x..content.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect()
    }

    #[test]
    fn clock_ends_at_the_right_edge_of_every_item() {
        let at = today_noon_millis();
        let when = activity_time(local_datetime(at), Local::now().date_naive());

        for event in [
            activity_event(at, "gs", "git status"),
            voice_event(at, "a short dictation"),
        ] {
            let mut state = state_with_activity(vec![event]);
            let buffer = render_buffer(&mut state, 80, 30);
            let (content, _) = content_area(80, 30);

            let row = (content.y..content.bottom())
                .find(|&y| row_text(&buffer, content, y).contains(&when))
                .expect("clock row");
            let line = row_text(&buffer, content, row);
            assert!(
                line.trim_end().ends_with(&when),
                "clock is not last on its line: {line:?}"
            );
            let clock_end = content.x + line.trim_end().len() as u16 - 1;
            assert_eq!(
                clock_end,
                content.right() - 2,
                "clock is not flush right: {line:?}"
            );
        }
    }

    #[test]
    fn content_keeps_a_gap_before_the_right_column() {
        let at = today_noon_millis();
        let when = activity_time(local_datetime(at), Local::now().date_naive());
        let long = "content that is long enough to reach the right side of the pane and keep going";
        let mut state =
            state_with_activity(vec![activity_event(at, "gs", long), voice_event(at, long)]);
        let buffer = render_buffer(&mut state, 100, 40);
        let (content, _) = content_area(100, 40);
        let clock_x = content.right() - 1 - cell_width(&when) as u16;
        let button_x = content.right() - cell_width(COPY_LABEL) as u16;

        let mut clock_rows = 0;
        let mut button_rows = 0;
        for y in content.y..content.bottom() {
            let line = row_text(&buffer, content, y);
            let column = if line.contains(COPY_LABEL) {
                button_rows += 1;
                button_x
            } else if line.contains(&when) {
                clock_rows += 1;
                clock_x
            } else {
                continue;
            };
            let last = (content.x..column)
                .rev()
                .find(|&x| !buffer[(x, y)].symbol().trim().is_empty());
            if let Some(last) = last {
                assert!(
                    last + CONTENT_GAP < column,
                    "content runs into the right column on row {y}: {line:?}"
                );
            }
        }
        assert!(clock_rows >= 2, "both event kinds draw a clock");
        assert!(button_rows >= 2, "both event kinds draw a copy button");
    }

    #[test]
    fn one_line_dictation_puts_the_copy_under_the_clock() {
        let at = today_noon_millis();
        let when = activity_time(local_datetime(at), Local::now().date_naive());
        let mut state = state_with_activity(vec![voice_event(at, "short note")]);
        let buffer = render_buffer(&mut state, 80, 30);
        let (content, feed_start) = content_area(80, 30);
        let first = content.y + (feed_start + GROUP_HEADER_LINES) as u16;

        let text_line = row_text(&buffer, content, first);
        assert!(text_line.contains("short note"), "{text_line:?}");
        assert!(text_line.contains(&when), "{text_line:?}");

        let copy_line = row_text(&buffer, content, first + 1);
        assert!(copy_line.contains(COPY_LABEL), "{copy_line:?}");
        assert!(!copy_line.contains(&when), "clock stays on the first line");
    }

    #[test]
    fn copy_button_hit_test_finds_the_event() {
        let mut state = state_with_activity(vec![activity_event(
            today_noon_millis(),
            "gs",
            "tautui --home",
        )]);
        let _ = draw(&mut state, 120, 40);
        let (content, feed_start) = content_area(120, 40);
        let body_row = content.y + (feed_start + GROUP_HEADER_LINES + 1) as u16;
        let button_x = content.right() - cell_width(COPY_LABEL) as u16;

        assert_eq!(state.copy_at(button_x + 1, body_row), Some(0));
        assert_eq!(state.copy_at(content.x + 1, body_row), None);
    }

    #[test]
    fn hotkey_activity_renders_as_keycaps() {
        let mut event = activity_event(today_noon_millis(), "ralt+m", "mouse back");
        event.category = ActivityCategory::Hotkey;
        let mut state = state_with_activity(vec![event]);
        let text = draw(&mut state, 120, 40);

        assert!(text.contains(" ralt "), "first keycap missing");
        assert!(text.contains(" m "), "second keycap missing");
        assert!(!text.contains("ralt+m"), "chord is split into keycaps");
        assert!(text.contains("mouse back"));
    }

    #[test]
    fn cards_scroll_up_and_the_date_stays_pinned() {
        use chrono::{Local, TimeZone};

        let old = Local
            .with_ymd_and_hms(2024, 10, 20, 11, 30, 0)
            .single()
            .expect("local time")
            .timestamp_millis();
        let noon = today_noon_millis();
        let mut activity: Vec<RecentActivity> = (0..30)
            .map(|index| {
                activity_event(
                    noon + index as i64 * 1_000,
                    &format!("t{index}"),
                    &format!("today body {index}"),
                )
            })
            .collect();
        activity.extend((0..10).map(|index| {
            activity_event(
                old + index as i64 * 1_000,
                &format!("o{index}"),
                &format!("old body {index}"),
            )
        }));
        let mut state = state_with_activity(activity);

        let first = draw(&mut state, 100, 30);
        assert!(first.contains("TIME SAVED"), "cards start at the top");
        assert!(!first.contains("old body"), "older day starts offscreen");

        let (_, feed_start) = content_area(100, 30);
        state.set_scroll(feed_start + 40);
        let middle = draw(&mut state, 100, 30);
        assert!(!middle.contains("TIME SAVED"), "cards scrolled away");
        assert!(
            middle
                .lines()
                .next()
                .is_some_and(|line| line.trim() == "Today"),
            "today's header pins while its group is on screen: {:?}",
            middle.lines().next()
        );

        state.scroll_to_end();
        let last = draw(&mut state, 100, 30);
        assert!(
            last.lines()
                .next()
                .is_some_and(|line| line.trim() == "October 20 2024"),
            "the next day's header takes over at the top: {:?}",
            last.lines().next()
        );
        assert!(last.contains("old body 9"), "oldest event reachable");
        assert!(last.contains("copy"), "copy button drawn");
    }

    #[test]
    fn scrolling_reaches_the_oldest_event_and_back() {
        let noon = today_noon_millis();
        let activity = (0..40)
            .map(|index| {
                activity_event(
                    noon + index as i64 * 1_000,
                    &format!("t{index}"),
                    &format!("body {index}"),
                )
            })
            .collect();
        let mut state = state_with_activity(activity);

        let first_page = draw(&mut state, 120, 30);
        assert!(first_page.contains("t0"));
        assert!(!first_page.contains("t39"), "oldest event starts offscreen");
        assert_eq!(state.scroll(), 0);

        state.scroll_to_end();
        let last_page = draw(&mut state, 120, 30);
        assert!(last_page.contains("t39"), "end reaches the oldest event");
        assert!(!last_page.contains("t0 "), "newest event scrolled away");

        let (content, feed_start) = content_area(120, 30);
        let total = feed_start
            + rows_height(&feed_rows(
                &state.stats().recent_activity,
                content.width,
                Local::now().date_naive(),
            ));
        assert_eq!(
            state.scroll(),
            total.saturating_sub(30),
            "scroll clamps to the last page"
        );

        state.scroll_to_start();
        let first_page = draw(&mut state, 120, 30);
        assert!(first_page.contains("t0"));
        assert!(first_page.contains("TIME SAVED"), "cards scroll back in");
    }

    #[test]
    fn wrap_text_keeps_every_word() {
        let text = "alpha beta gamma delta epsilon zeta eta theta";
        let lines = wrap_text(text, 12, 10);

        assert_eq!(lines.join(" "), text, "no word is dropped");
        for (index, line) in lines.iter().enumerate() {
            let budget = if index == 0 { 12 } else { 10 };
            assert!(
                cell_width(line) <= budget,
                "{line:?} exceeds {budget} cells"
            );
        }
    }

    #[test]
    fn wrap_text_splits_words_that_do_not_fit() {
        let lines = wrap_text("abcdefghijklmnop", 5, 4);

        assert_eq!(lines.concat(), "abcdefghijklmnop");
        assert!(cell_width(&lines[0]) <= 5);
        assert!(lines[1..].iter().all(|line| cell_width(line) <= 4));
    }

    #[test]
    fn wrap_text_counts_terminal_cells() {
        let lines = wrap_text("日本語テキスト", 4, 4);

        assert_eq!(lines.concat(), "日本語テキスト");
        assert!(lines.iter().all(|line| cell_width(line) <= 4));
    }

    #[test]
    fn truncate_counts_terminal_cells_not_chars() {
        assert_eq!(truncate("日本語テキスト", 5), "日本…");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn detail_width_follows_the_time_column() {
        assert_eq!(activity_text_width(30, "2m ago"), 17);
        assert_eq!(activity_text_width(30, "11:30 AM"), 15);
        assert_eq!(activity_text_width(3, "2m ago"), 0);
    }

    #[test]
    fn cards_show_at_most_two_per_row() {
        let text = draw(&mut sample_state(), 100, 40);

        for label in ["TIME SAVED", "WORDS SPOKEN", "WORDS EXPANDED", "DAY STREAK"] {
            assert!(text.contains(label), "{label} missing from the grid");
        }
        assert!(!text.contains("EXPANSIONS"), "all-time expansions removed");
        for detail in ["estimated", "dictated", "from snippets", "best 21"] {
            assert!(!text.contains(detail), "{detail} still under a card");
        }

        assert_eq!(cards_per_row(200), 2, "never more than two cards per row");
        assert_eq!(cards_per_row(100), 2);
        assert_eq!(cards_per_row(20), 1, "narrow panes stack the cards");

        // The first two cards share a row; the third starts the next one.
        let rows: Vec<usize> = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("TIME SAVED") || line.contains("WORDS EXPANDED"))
            .map(|(row, _)| row)
            .collect();
        assert_eq!(rows.len(), 2);
        assert!(rows[0] < rows[1], "third card wraps below the first row");
        assert!(
            text.lines().nth(rows[0]).is_some_and(|line| {
                line.contains("TIME SAVED") && line.contains("WORDS SPOKEN")
            })
        );
    }

    #[test]
    fn empty_stats_show_quiet_empty_states() {
        let text = draw(&mut HomePageState::default(), 120, 40);

        assert!(text.contains("No activity yet."));
        assert!(!text.contains("No usage yet."), "most used section removed");
        assert!(text.contains("0s"));
    }
}
