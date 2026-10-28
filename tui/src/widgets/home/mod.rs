use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
};
use taurine_core::db::crud::ActivityCategory;
use taurine_core::stats::{HomeStats, RecentActivity};

use crate::theme::Theme;
use crate::widgets::hotkey;
use crate::widgets::library::detail::relative_time;
use crate::widgets::panes::{self, DividerSide, PaneSplit};
use crate::widgets::wrap::{cell_width, wrap_line};

pub mod account;
pub mod state;

use self::state::{CardHit, CopyHit, HomePageState};

const PANE_PAD_X: u16 = 2;

/// Home is the library split with the list collapsed: activity in the
/// center, account and quota in the props slot.
const HOME_LIST_RATIO: f32 = 0.0;

/// Card chrome: a blank top pad, the tag row, a spacer row under the
/// tag, a blank row after the content, the meta row that carries the
/// clock and copy button, and a blank bottom pad. Payload rows sit in
/// between.
const CARD_TAG_ROW: usize = 1;
const CARD_SPACER_ROW: usize = CARD_TAG_ROW + 1;
const CARD_PAYLOAD_START: usize = CARD_SPACER_ROW + 1;
const CARD_CHROME_LINES: usize = CARD_PAYLOAD_START + 3;
/// One page-background row between cards.
const CARD_GAP_LINES: usize = 1;
/// Smallest card block: chrome, one payload row, and the gap.
const CARD_MIN_BLOCK_LINES: usize = CARD_CHROME_LINES + 1 + CARD_GAP_LINES;
/// Date header plus the blank row beneath it.
const DATE_BLOCK_LINES: usize = 2;
/// Section title plus the blank row beneath it.
const TITLE_LINES: usize = 2;
/// Pinned band while scrolled: title, gap, date, gap.
const STICKY_BAND_LINES: usize = 4;
/// Row the pinned date sits on, beneath the title and its gap.
const STICKY_DATE_ROW: u16 = 2;

/// Cards stop widening here on large screens, so the eye never travels
/// far between the tag on the left and the clock on the right.
const CARD_MAX_WIDTH: u16 = 100;
/// Horizontal breathing room inside a card.
const CARD_PAD_X: u16 = 2;

const COPY_LABEL: &str = " copy ";

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

pub(crate) fn split_home(area: Rect, account_ratio: f32) -> PaneSplit {
    if area.width < 5 {
        return PaneSplit {
            list: Rect::default(),
            center: area,
            props: Rect::default(),
            list_divider: None,
            props_divider: None,
        };
    }
    panes::split_panes(area, HOME_LIST_RATIO, account_ratio)
}

pub(crate) fn visible_divider_columns(area: Rect, account_ratio: f32) -> Vec<(DividerSide, u16)> {
    split_home(area, account_ratio)
        .props_divider
        .map(|column| (DividerSide::Props, column))
        .into_iter()
        .collect()
}

pub(crate) fn divider_hit(
    area: Rect,
    account_ratio: f32,
    column: u16,
    row: u16,
) -> Option<DividerSide> {
    if row < area.y || row >= area.y.saturating_add(area.height) {
        return None;
    }
    let split = split_home(area, account_ratio);
    (split.props_divider == Some(column)).then_some(DividerSide::Props)
}

pub(crate) fn ratio_for_column(area: Rect, column: u16) -> f32 {
    panes::detail_ratio_for_column(area, HOME_LIST_RATIO, column)
}

/// Home is a quiet column of flat activity cards under a section title,
/// with the account pane in the library's props slot. The date header of
/// the group under the viewport's top edge sticks there while that group
/// is on screen.
pub(crate) fn render_home_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &mut HomePageState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let split = split_home(area, state.account_ratio());
    if split.props.width > 0 {
        account::render(
            frame,
            split.props,
            theme,
            state.account(),
            account::now_epoch(),
        );
    }
    let content = padded(split.center, PANE_PAD_X, 0);
    if content.width < 4 || content.height == 0 {
        return;
    }
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        content,
    );
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

    let feed_start = TITLE_LINES;
    let rows = feed_rows(&state.stats().recent_activity, content.width);
    let feed_lines = rows_height(&rows).max(1);
    let viewport = content.height as usize;
    let max_scroll = (feed_start + feed_lines).saturating_sub(viewport);

    let mut scroll = state.scroll().min(max_scroll);
    // Keyboard selection pulls its card fully into view; free scrolling
    // and hover never move the page.
    if state.selection_moved()
        && let Some(selected) = state.selected()
        && let Some((top, fill)) = card_position(&rows, selected)
    {
        let bottom = top + fill;
        if top < scroll {
            // Keep the title and date band's four rows free above the
            // card; the first card simply scrolls home.
            scroll = if selected == 0 {
                0
            } else {
                top.saturating_sub(STICKY_BAND_LINES)
            };
        } else if bottom > scroll + viewport {
            scroll = bottom.saturating_sub(viewport);
        }
    }
    state.clear_selection_moved();
    let scroll = scroll.min(max_scroll);
    state.set_scroll(scroll);
    state.set_page_cards((viewport / CARD_MIN_BLOCK_LINES).max(1));

    let view = FeedViewport {
        content,
        start: feed_start,
        scroll,
    };
    let active = state.active();
    let mut hits = FeedHits::default();
    render_feed(frame, theme, state.stats(), &rows, view, active, &mut hits);
    let sticky = sticky_date(&rows, view);
    let title_pinned = scroll > 0;
    let covered = content
        .y
        .saturating_add(if title_pinned { 2 } else { 0 })
        .saturating_add(if sticky.is_some() { 2 } else { 0 });
    if covered > content.y {
        // honey: the pinned title and date cover the top of the feed, so
        // cards lose those rows and hidden copy buttons are not clickable.
        for hit in &mut hits.cards {
            let bottom = hit.area.bottom();
            hit.area.y = hit.area.y.max(covered);
            hit.area.height = bottom.saturating_sub(hit.area.y);
        }
        hits.cards.retain(|hit| hit.area.height > 0);
        hits.copies.retain(|hit| hit.area.y >= covered);
    }
    state.set_card_hits(hits.cards);
    state.set_copy_hits(hits.copies);

    render_title(frame, theme, content);
    if let Some(date) = sticky {
        render_sticky_date(frame, theme, date, content);
    }
}

/// Pins the title with its gap row so the header never crowds the feed.
fn render_title(frame: &mut Frame, theme: &Theme, content: Rect) {
    let band = Rect {
        height: content.height.min(2),
        ..content
    };
    clear_sticky_row(frame, band, theme);
    frame.render_widget(
        Paragraph::new("Recent Activity")
            .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        Rect { height: 1, ..band },
    );
}

/// Pinned rows float over the feed, so they clear their own strip first;
/// otherwise card text would show through the gaps around the label.
fn clear_sticky_row(frame: &mut Frame, area: Rect, theme: &Theme) {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background)),
        area,
    );
}

enum FeedRow {
    Date(NaiveDate),
    Card(CardLayout),
}

/// One wrapped activity card: the source event and every payload row.
struct CardLayout {
    index: usize,
    lines: Vec<String>,
}

impl CardLayout {
    /// Rows the card fill occupies: chrome plus every payload row.
    fn fill_height(&self) -> usize {
        self.lines.len() + CARD_CHROME_LINES
    }

    /// Fill plus the blank page-background row that follows it.
    fn block_height(&self) -> usize {
        self.fill_height() + CARD_GAP_LINES
    }
}

/// One entry per drawn block, in document order.
fn feed_rows(activity: &[RecentActivity], width: u16) -> Vec<FeedRow> {
    let card_width = width.min(CARD_MAX_WIDTH);
    let text_width = payload_width(card_width);
    let mut rows = Vec::with_capacity(activity.len() + 4);
    let mut previous: Option<NaiveDate> = None;
    for (index, event) in activity.iter().enumerate() {
        let date = local_datetime(event.timestamp).date_naive();
        if previous != Some(date) {
            rows.push(FeedRow::Date(date));
            previous = Some(date);
        }
        rows.push(FeedRow::Card(CardLayout {
            index,
            lines: wrap_payload(event_body(event), text_width),
        }));
    }
    rows
}

/// Wraps a payload to the card's text column. Every word survives; only
/// a word wider than the column is split, never through a glyph.
fn wrap_payload(body: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for source in body.lines() {
        for (row, _) in wrap_line(source, width, width) {
            lines.push(row);
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Cells available for payload text: the card's full inner width. The
/// clock and copy button live on the meta row below a blank row, so the
/// payload never needs a reserved right column.
fn payload_width(card_width: u16) -> usize {
    card_width
        .saturating_sub(CARD_PAD_X.saturating_mul(2))
        .max(1) as usize
}

/// Drawn height of the whole feed, headers included.
fn rows_height(rows: &[FeedRow]) -> usize {
    rows.iter()
        .map(|row| match row {
            FeedRow::Date(_) => DATE_BLOCK_LINES,
            FeedRow::Card(card) => card.block_height(),
        })
        .sum()
}

/// Document line and fill height of a card, used to scroll the selection
/// fully into view.
fn card_position(rows: &[FeedRow], selected: usize) -> Option<(usize, usize)> {
    let mut line = TITLE_LINES;
    for row in rows {
        match row {
            FeedRow::Date(_) => line += DATE_BLOCK_LINES,
            FeedRow::Card(card) => {
                if card.index == selected {
                    return Some((line, card.fill_height()));
                }
                line += card.block_height();
            }
        }
    }
    None
}

/// Where the feed starts in the document and how far the page is scrolled.
#[derive(Debug, Clone, Copy)]
struct FeedViewport {
    content: Rect,
    start: usize,
    scroll: usize,
}

/// Pointer targets recorded by one frame.
#[derive(Debug, Default)]
struct FeedHits {
    cards: Vec<CardHit>,
    copies: Vec<CopyHit>,
}

fn render_feed(
    frame: &mut Frame,
    theme: &Theme,
    stats: &HomeStats,
    rows: &[FeedRow],
    view: FeedViewport,
    active: Option<usize>,
    hits: &mut FeedHits,
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

    let width = card_width(content);
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
                line += DATE_BLOCK_LINES;
            }
            FeedRow::Card(card) => {
                let view = CardView {
                    content,
                    scroll,
                    line,
                    width,
                    active: active == Some(card.index),
                    theme,
                };
                render_card(frame, &stats.recent_activity[card.index], card, view, hits);
                line += card.block_height();
            }
        }
    }
}

/// Where one card sits on the page and whether it owns the focus.
#[derive(Debug, Clone, Copy)]
struct CardView<'a> {
    content: Rect,
    scroll: usize,
    line: usize,
    width: u16,
    active: bool,
    theme: &'a Theme,
}

fn render_card(
    frame: &mut Frame,
    event: &RecentActivity,
    card: &CardLayout,
    view: CardView<'_>,
    hits: &mut FeedHits,
) {
    let CardView {
        content,
        scroll,
        line,
        width,
        active,
        theme,
    } = view;
    let bg = if active {
        theme.card.active_bg
    } else {
        theme.card.bg
    };
    let fill = card.fill_height();

    let top = line as isize - scroll as isize;
    let visible_top = top.max(0);
    let visible_bottom = (top + fill as isize).min(content.height as isize);
    if visible_bottom > visible_top {
        let area = Rect {
            x: content.x,
            y: content.y.saturating_add(visible_top as u16),
            width,
            height: (visible_bottom - visible_top) as u16,
        };
        frame.render_widget(Block::default().style(Style::default().bg(bg)), area);
        hits.cards.push(CardHit {
            event: card.index,
            area,
        });
    }

    let inner_x = content.x.saturating_add(CARD_PAD_X);
    let inner_width = width.saturating_sub(CARD_PAD_X.saturating_mul(2));
    if inner_width == 0 {
        return;
    }

    if let Some(row) = visible_line(content, scroll, line + CARD_TAG_ROW) {
        render_tag_line(
            frame,
            Rect {
                x: inner_x,
                width: inner_width,
                ..row
            },
            theme,
            event,
            bg,
        );
    }

    let text_width = payload_width(width) as u16;
    for (offset, text) in card.lines.iter().enumerate() {
        if let Some(row) = visible_line(content, scroll, line + CARD_PAYLOAD_START + offset)
            && !text.is_empty()
        {
            frame.render_widget(
                Paragraph::new(text.clone()).style(Style::default().fg(theme.text)),
                Rect {
                    x: inner_x,
                    width: text_width,
                    ..row
                },
            );
        }
    }

    let meta_row = line + CARD_PAYLOAD_START + card.lines.len() + 1;
    if let Some(row) = visible_line(content, scroll, meta_row) {
        let area = Rect {
            x: inner_x,
            width: inner_width,
            ..row
        };
        if let Some(button) = render_meta_line(frame, area, theme, event, active) {
            hits.copies.push(CopyHit {
                event: card.index,
                area: button,
            });
        }
    }
}

/// Bottom meta row: the relative clock on the left, and on the active
/// card the copy button right beside it. Nothing is right-aligned.
fn render_meta_line(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    event: &RecentActivity,
    active: bool,
) -> Option<Rect> {
    if area.width == 0 {
        return None;
    }
    let when = truncate(&event_time(event), area.width as usize);
    let time_width = cell_width(&when) as u16;
    frame.render_widget(
        Paragraph::new(when).style(Style::default().fg(theme.description)),
        Rect {
            width: time_width,
            ..area
        },
    );
    if !active {
        return None;
    }
    let copy_width = cell_width(COPY_LABEL) as u16;
    let x = area.x.saturating_add(time_width).saturating_add(1);
    if x.saturating_add(copy_width) > area.right() {
        return None;
    }
    let button = Rect {
        x,
        y: area.y,
        width: copy_width,
        height: 1,
    };
    render_copy_button(frame, button, theme);
    Some(button)
}

/// Line 1: the source tag on the left. The clock moved to the meta row.
fn render_tag_line(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    event: &RecentActivity,
    card_bg: Color,
) {
    let spans = tag_spans(event, area.width as usize, theme, card_bg);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn tag_spans(
    event: &RecentActivity,
    budget: usize,
    theme: &Theme,
    card_bg: Color,
) -> Vec<Span<'static>> {
    if event.category == ActivityCategory::Voice {
        return vec![pill("voice", budget, theme)];
    }
    if is_hotkey_activity(event) {
        return hotkey::render_hotkey_spans(
            &event.trigger,
            budget,
            theme.keycap.bg,
            theme.text,
            card_bg,
        );
    }
    vec![pill(&event.trigger, budget, theme)]
}

/// Flat keycap-style tag: one space of padding, never a border.
fn pill(label: &str, budget: usize, theme: &Theme) -> Span<'static> {
    if budget < 3 {
        return Span::raw(truncate(label, budget));
    }
    let text = truncate(label, budget.saturating_sub(2));
    Span::styled(
        format!(" {text} "),
        Style::default().bg(theme.keycap.bg).fg(theme.text),
    )
}

fn visible_line(content: Rect, scroll: usize, line: usize) -> Option<Rect> {
    let offset = line.checked_sub(scroll)?;
    (offset < content.height as usize).then(|| Rect {
        y: content.y.saturating_add(offset as u16),
        height: 1,
        ..content
    })
}

fn card_width(content: Rect) -> u16 {
    content.width.min(CARD_MAX_WIDTH)
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

/// Date header that should stick just below the pinned title because its
/// group is the one under the viewport's top edge.
fn sticky_date(rows: &[FeedRow], view: FeedViewport) -> Option<NaiveDate> {
    let FeedViewport { start, scroll, .. } = view;
    if scroll == 0 {
        return None;
    }
    // The title and its gap own rows 0-1, so the header pins on row 2.
    let pinned_row = scroll.saturating_add(STICKY_DATE_ROW as usize);
    let mut line = start;
    let mut sticky = None;
    for row in rows {
        if line > pinned_row {
            break;
        }
        match row {
            FeedRow::Date(date) => {
                sticky = Some(*date);
                line += DATE_BLOCK_LINES;
            }
            FeedRow::Card(card) => line += card.block_height(),
        }
    }
    sticky
}

/// Pins the date of the group just under the title, so the feed always
/// says which day it is showing while the cards scroll away.
fn render_sticky_date(frame: &mut Frame, theme: &Theme, date: NaiveDate, content: Rect) {
    let y = content.y.saturating_add(STICKY_DATE_ROW);
    if y >= content.bottom() {
        return;
    }
    let band = Rect {
        y,
        height: content.bottom().saturating_sub(y).min(2),
        ..content
    };
    clear_sticky_row(frame, band, theme);
    render_group_header(
        frame,
        Rect { height: 1, ..band },
        theme,
        date,
        Local::now().date_naive(),
    );
}

/// Feed cards read relative times; an unparsable timestamp falls back to
/// the wall clock.
fn event_time(event: &RecentActivity) -> String {
    let at = local_datetime(event.timestamp);
    relative_time(at.timestamp()).unwrap_or_else(|| clock_time(at))
}

fn clock_time(at: DateTime<Local>) -> String {
    let (is_pm, hour) = at.hour12();
    format!(
        "{hour}:{:02} {}",
        at.minute(),
        if is_pm { "PM" } else { "AM" }
    )
}

fn render_copy_button(frame: &mut Frame, area: Rect, theme: &Theme) {
    frame.render_widget(
        Paragraph::new(COPY_LABEL).style(Style::default().bg(theme.keycap.bg).fg(theme.text)),
        area,
    );
}

/// The text an event produced. Voice triggers record the snippet they
/// inserted, but the feed wants what was actually said, so their trigger
/// phrase wins; dictations carry the transcript in content.
pub(crate) fn event_body(event: &RecentActivity) -> &str {
    if event.category == ActivityCategory::Voice
        && event.trigger != "Dictation"
        && !event.trigger.trim().is_empty()
    {
        return &event.trigger;
    }
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

    use super::*;
    use crate::theme::default::DARK_THEME;
    use crate::widgets::library;

    fn sample_state() -> HomePageState {
        let mut state = HomePageState::default();
        state.set_account_ratio(0.0);
        state.replace_stats(HomeStats {
            recent_activity: vec![RecentActivity {
                timestamp: library::now_millis() as i64 - 120_000,
                category: ActivityCategory::Text,
                trigger: "gs".to_string(),
                detail: "Open taurine tui".to_string(),
                content: Some("tautui --home".to_string()),
                app: "Windowsterminal".to_string(),
                word_count: 42,
            }],
            ..HomeStats::default()
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

    fn row_text(buffer: &Buffer, width: u16, y: u16) -> String {
        (0..width).map(|x| buffer[(x, y)].symbol()).collect()
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
        state.set_account_ratio(0.0);
        state.replace_stats(HomeStats {
            recent_activity: activity,
            ..HomeStats::default()
        });
        state
    }

    fn content_area(width: u16, height: u16) -> Rect {
        let center = split_home(Rect::new(0, 0, width, height), 0.0).center;
        padded(center, PANE_PAD_X, 0)
    }

    /// Document line the nth single-line card starts on.
    fn card_top(n: usize) -> usize {
        TITLE_LINES + DATE_BLOCK_LINES + n * CARD_MIN_BLOCK_LINES
    }

    /// Document row of a card's meta line for a payload of `lines` rows.
    fn meta_row(card: usize, lines: usize) -> u16 {
        (card_top(card) + CARD_PAYLOAD_START + lines + 1) as u16
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
    fn home_opens_on_the_activity_feed_without_cards() {
        let mut state = sample_state();
        let text = draw(&mut state, 120, 40);

        assert!(
            text.lines()
                .next()
                .is_some_and(|line| line.trim() == "Recent Activity"),
            "the page starts on Recent Activity: {:?}",
            text.lines().next()
        );
        assert!(!text.contains("TIME SAVED"), "stat cards removed");
        assert!(!text.contains("WORDS SPOKEN"), "stat cards removed");
        assert!(text.contains("Today"));
        assert!(text.contains(" gs "), "trigger tag missing");
        assert!(text.contains("tautui --home"), "activity body missing");
        assert!(
            !text.contains("Open taurine tui"),
            "the stored content wins over the description"
        );
        assert!(!text.contains("Most Used Triggers"), "section removed");
        assert!(!text.contains("Windowsterminal"), "source app is hidden");
        assert!(text.contains("2m ago"));
        for border in ["┌", "┐", "└", "┘", "─", "│"] {
            assert!(!text.contains(border), "home page draws no borders");
        }
    }

    #[test]
    fn cards_are_flat_fills_separated_by_one_blank_row() {
        let noon = today_noon_millis();
        let mut state = state_with_activity(vec![
            activity_event(noon, "aa", "first body"),
            activity_event(noon - 1_000, "zz", "second body"),
        ]);
        let buffer = render_buffer(&mut state, 120, 40);
        let content = content_area(120, 40);

        let fill = |row: usize| buffer[(content.x, row as u16)].bg;
        assert_eq!(
            fill(card_top(0)),
            DARK_THEME.card.active_bg,
            "the selected card starts focused"
        );
        assert_eq!(
            fill(card_top(1) - 1),
            DARK_THEME.background,
            "one page-background row between cards"
        );
        assert_eq!(fill(card_top(1)), DARK_THEME.card.bg);
    }

    #[test]
    fn cards_keep_a_spacer_row_between_the_tag_and_payload() {
        let noon = today_noon_millis();
        let mut state = state_with_activity(vec![
            activity_event(noon, "aa", "first body"),
            activity_event(noon - 1_000, "zz", "second body"),
        ]);
        let buffer = render_buffer(&mut state, 100, 40);
        let content = content_area(100, 40);
        let tag_row = (card_top(0) + CARD_TAG_ROW) as u16;
        let spacer = (card_top(0) + CARD_SPACER_ROW) as u16;
        let payload = (card_top(0) + CARD_PAYLOAD_START) as u16;
        let meta = meta_row(0, 1);
        let when = event_time(&state.stats().recent_activity[0]);

        assert!(row_text(&buffer, 100, tag_row).contains(" aa "));
        assert!(
            row_text(&buffer, 100, spacer).trim().is_empty(),
            "spacer row is blank"
        );
        assert_eq!(
            buffer[(content.x, spacer)].bg,
            DARK_THEME.card.active_bg,
            "spacer keeps the card fill"
        );
        let payload_line = row_text(&buffer, 100, payload);
        assert!(payload_line.contains("first body"));
        assert!(!payload_line.contains(COPY_LABEL));
        let meta_line = row_text(&buffer, 100, meta);
        assert_eq!(
            meta_line.trim(),
            format!("{when}  copy"),
            "clock and copy share the meta row"
        );
        let bottom_pad = meta + 1;
        assert!(
            row_text(&buffer, 100, bottom_pad).trim().is_empty(),
            "a blank bottom padding row follows the meta row"
        );
        assert_eq!(
            buffer[(content.x, bottom_pad)].bg,
            DARK_THEME.card.active_bg,
            "bottom padding keeps the card fill"
        );
        assert_eq!(
            buffer[(content.x, bottom_pad + 1)].bg,
            DARK_THEME.background,
            "the page gap follows the bottom padding"
        );
    }

    #[test]
    fn cards_stop_widening_on_large_screens() {
        let mut state = sample_state();
        let buffer = render_buffer(&mut state, 140, 30);
        let content = content_area(140, 30);
        let inside = buffer[(content.x + CARD_MAX_WIDTH - 1, card_top(0) as u16)].bg;
        let outside = buffer[(content.x + CARD_MAX_WIDTH, card_top(0) as u16)].bg;

        assert_eq!(inside, DARK_THEME.card.active_bg, "selected card fill");
        assert_eq!(outside, DARK_THEME.background, "page background");
    }

    #[test]
    fn only_the_active_card_shows_a_copy_button() {
        let noon = today_noon_millis();
        let mut state = state_with_activity(vec![
            activity_event(noon, "aa", "first body"),
            activity_event(noon - 1_000, "zz", "second body"),
        ]);

        let first = draw(&mut state, 100, 30);
        assert_eq!(first.matches(COPY_LABEL).count(), 1);
        let lines: Vec<&str> = first.lines().collect();
        assert!(
            lines[meta_row(0, 1) as usize].contains(COPY_LABEL),
            "copy sits beside the clock"
        );
        assert!(
            lines[card_top(0) + CARD_PAYLOAD_START].contains("first body"),
            "payload sits above the meta row"
        );
        assert!(!lines[card_top(0) + CARD_PAYLOAD_START].contains(COPY_LABEL));

        state.select(1);
        let second = draw(&mut state, 100, 30);
        assert_eq!(second.matches(COPY_LABEL).count(), 1);
        let lines: Vec<&str> = second.lines().collect();
        assert!(lines[meta_row(1, 1) as usize].contains(COPY_LABEL));
        assert!(lines[card_top(1) + CARD_PAYLOAD_START].contains("second body"));
    }

    #[test]
    fn selected_card_uses_the_focus_fill() {
        let noon = today_noon_millis();
        let mut state = state_with_activity(vec![
            activity_event(noon, "aa", "first body"),
            activity_event(noon - 1_000, "zz", "second body"),
        ]);

        let buffer = render_buffer(&mut state, 100, 30);
        let content = content_area(100, 30);
        assert_eq!(
            buffer[(content.x, card_top(0) as u16)].bg,
            DARK_THEME.card.active_bg
        );
        assert_eq!(
            buffer[(content.x, card_top(1) as u16)].bg,
            DARK_THEME.card.bg
        );
    }

    #[test]
    fn hover_focuses_a_card_without_moving_the_selection() {
        let noon = today_noon_millis();
        let mut state = state_with_activity(vec![
            activity_event(noon, "aa", "first body"),
            activity_event(noon - 1_000, "zz", "second body"),
        ]);
        let content = content_area(100, 30);
        let _ = render_buffer(&mut state, 100, 30);

        state.hover_at(content.x + 1, (card_top(1) + 1) as u16);
        assert_eq!(state.active(), Some(1));
        assert_eq!(state.selected(), Some(0));

        let buffer = render_buffer(&mut state, 100, 30);
        assert_eq!(
            buffer[(content.x, card_top(0) as u16)].bg,
            DARK_THEME.card.bg,
            "selection loses the focus fill while hovering elsewhere"
        );
        assert_eq!(
            buffer[(content.x, card_top(1) as u16)].bg,
            DARK_THEME.card.active_bg
        );
        let text = draw(&mut state, 100, 30);
        assert_eq!(text.matches(COPY_LABEL).count(), 1);
    }

    #[test]
    fn tag_line_carries_tags_and_meta_line_carries_the_clock() {
        let mut state = sample_state();
        let buffer = render_buffer(&mut state, 120, 40);
        let when = event_time(&state.stats().recent_activity[0]);

        let tag_line = row_text(&buffer, 120, (card_top(0) + CARD_TAG_ROW) as u16);
        assert!(tag_line.contains(" gs "), "tag missing: {tag_line:?}");
        assert!(
            !tag_line.contains(&when),
            "clock moved below the content: {tag_line:?}"
        );

        let meta_line = row_text(&buffer, 120, meta_row(0, 1));
        assert_eq!(
            meta_line.trim(),
            format!("{when}  copy"),
            "clock and copy share the left-aligned meta row"
        );
    }

    #[test]
    fn voice_card_wraps_the_whole_payload() {
        let body = "remind me to call mom tomorrow morning and ask about the weekend plans";
        let mut state = state_with_activity(vec![voice_event(today_noon_millis(), body)]);
        let text = draw(&mut state, 70, 40);
        let content = content_area(70, 40);
        let rows = feed_rows(&state.stats().recent_activity, content.width);
        let lines = match &rows[1] {
            FeedRow::Card(card) => card.lines.clone(),
            FeedRow::Date(_) => panic!("second row is the card"),
        };

        assert!(text.contains(" voice "), "voice tag missing:\n{text}");
        assert!(!text.contains("Dictation"), "raw trigger label hidden");
        assert!(!text.contains('…'), "dictation is never truncated:\n{text}");
        assert!(lines.len() > 1, "long dictation wraps");
        assert_eq!(lines.join(" "), body, "every word survives wrapping");
        for line in &lines {
            assert!(
                text.contains(line.as_str()),
                "wrapped row missing: {line:?}"
            );
        }
    }

    #[test]
    fn voice_trigger_shows_the_spoken_phrase_not_the_snippet_output() {
        let mut event = voice_event(today_noon_millis(), "unlimited voice output");
        event.trigger = "hello voice".to_string();
        let mut state = state_with_activity(vec![event]);
        let text = draw(&mut state, 80, 30);

        assert!(
            text.contains("hello voice"),
            "spoken phrase missing:\n{text}"
        );
        assert!(
            !text.contains("unlimited voice output"),
            "snippet output leaked into the feed:\n{text}"
        );
        assert!(text.contains(" voice "), "voice tag missing");
    }

    #[test]
    fn wrapped_payload_uses_the_full_card_width() {
        let long = "a very long payload that will wrap across the card instead of being \
                    cut off, and keeps going for a while longer so it cannot fit on one line"
            .repeat(2);
        let mut state = state_with_activity(vec![activity_event(today_noon_millis(), "gs", &long)]);
        let buffer = render_buffer(&mut state, 100, 40);
        let content = content_area(100, 40);
        let inner_width = card_width(content) - CARD_PAD_X * 2;
        let rows = feed_rows(&state.stats().recent_activity, content.width);
        let line_count = match &rows[1] {
            FeedRow::Card(card) => card.lines.len(),
            FeedRow::Date(_) => panic!("second row is the card"),
        };
        assert!(line_count > 1, "payload wraps onto several rows");
        assert_eq!(
            payload_width(card_width(content)) as u16,
            inner_width,
            "payload gets the full inner width"
        );

        let when = event_time(&state.stats().recent_activity[0]);
        let meta_line = row_text(&buffer, 100, meta_row(0, line_count));
        assert_eq!(
            meta_line.trim(),
            format!("{when}  copy"),
            "clock and copy sit together under the content"
        );
        assert!(!draw(&mut state, 100, 40).contains('…'), "no truncation");
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
        assert!(
            text.lines()
                .nth(card_top(0) + CARD_TAG_ROW)
                .is_some_and(|line| line.contains(" ralt ") && line.contains(" m ")),
            "keycaps sit on the tag line"
        );
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
        assert!(text.contains("ago"), "relative timestamp missing");
        assert!(!text.contains("Today"), "old group is not today");
        assert!(text.contains("open taurine tui"));
    }

    #[test]
    fn copy_button_hit_test_finds_the_event() {
        let mut state = sample_state();
        let _ = draw(&mut state, 120, 40);
        let content = content_area(120, 40);
        let when = event_time(&state.stats().recent_activity[0]);
        let time_width = cell_width(&when) as u16;
        let button_x = content.x + CARD_PAD_X + time_width + 1;
        let row = meta_row(0, 1);

        assert_eq!(state.copy_at(button_x + 1, row), Some(0));
        assert_eq!(state.copy_at(content.x + 1, row), None);
        assert_eq!(state.card_at(content.x + 1, row), Some(0));
    }

    #[test]
    fn title_and_date_stay_pinned_while_scrolling() {
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
        assert_eq!(first.lines().next().map(str::trim), Some("Recent Activity"));
        assert_eq!(first.lines().nth(2).map(str::trim), Some("Today"));
        assert!(!first.contains("old body"), "older day starts offscreen");

        state.set_scroll(TITLE_LINES + 40);
        let middle = draw(&mut state, 100, 30);
        let rows: Vec<&str> = middle.lines().collect();
        assert_eq!(rows[0].trim(), "Recent Activity", "the title stays pinned");
        assert!(rows[1].trim().is_empty(), "gap after the title");
        assert_eq!(rows[2].trim(), "Today", "the date pins beneath the gap");
        assert!(rows[3].trim().is_empty(), "gap after the date");

        state.select_last();
        state.set_scroll(usize::MAX);
        let last = draw(&mut state, 100, 30);
        let rows: Vec<&str> = last.lines().collect();
        assert_eq!(rows[0].trim(), "Recent Activity");
        assert!(rows[1].trim().is_empty());
        assert_eq!(
            rows[2].trim(),
            "October 20 2024",
            "the next day's header takes over beneath the title"
        );
        assert!(rows[3].trim().is_empty());
        assert!(last.contains("old body 9"), "oldest event reachable");
        assert!(last.contains(" copy "), "active card keeps its copy button");
    }

    #[test]
    fn pinned_rows_block_pointer_hits() {
        let noon = today_noon_millis();
        let activity = (0..10)
            .map(|index| {
                activity_event(
                    noon + index as i64 * 1_000,
                    &format!("t{index}"),
                    &format!("body {index}"),
                )
            })
            .collect();
        let mut state = state_with_activity(activity);
        state.set_scroll(6);
        let _ = draw(&mut state, 100, 30);
        let content = content_area(100, 30);

        for row in 0..4 {
            assert_eq!(
                state.card_at(content.x + 1, content.y + row),
                None,
                "pinned band row {row} is not a card"
            );
        }
    }

    #[test]
    fn selecting_a_card_scrolls_it_into_view() {
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

        state.select_last();
        let text = draw(&mut state, 120, 20);
        assert!(text.contains("body 39"), "selection scrolls into view");
        assert_eq!(
            text.lines().next().map(str::trim),
            Some("Recent Activity"),
            "the title stays pinned"
        );

        state.select_first();
        let text = draw(&mut state, 120, 20);
        assert!(text.contains("body 0"));
        assert!(text.contains("Recent Activity"));
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
        state.clear_selection_moved();

        let first_page = draw(&mut state, 120, 30);
        assert!(first_page.contains("t0"));
        assert!(!first_page.contains("t39"), "oldest event starts offscreen");
        assert_eq!(state.scroll(), 0);

        state.set_scroll(usize::MAX);
        let last_page = draw(&mut state, 120, 30);
        assert!(last_page.contains("t39"), "end reaches the oldest event");
        assert!(!last_page.contains("t0 "), "newest event scrolled away");

        let content = content_area(120, 30);
        let total =
            TITLE_LINES + rows_height(&feed_rows(&state.stats().recent_activity, content.width));
        assert_eq!(
            state.scroll(),
            total.saturating_sub(30),
            "scroll clamps to the last page"
        );

        state.set_scroll(0);
        let first_page = draw(&mut state, 120, 30);
        assert!(first_page.contains("t0"));
        assert!(
            first_page.contains("Recent Activity"),
            "title scrolls back in"
        );
    }

    #[test]
    fn parked_account_pane_sits_flush_on_the_rail() {
        let area = Rect::new(7, 0, 93, 30);
        let split = split_home(area, account::MAX_RATIO);

        assert_eq!(split.center.width, 0, "activity fully minimized");
        assert_eq!(split.props.x, area.x, "account pane sits flush on the rail");
        assert_eq!(
            split.props.width, area.width,
            "account pane fills the workspace"
        );
        assert_eq!(
            split.props_divider, None,
            "the rail divider is the single boundary"
        );
    }

    #[test]
    fn home_splits_activity_and_account_panes() {
        use taurine_core::db::crud::UserTier;

        let mut state = sample_state();
        state.set_account_ratio(0.3);
        state.replace_account(account::AccountInfo {
            authenticated: true,
            email: Some("user@example.com".to_string()),
            tier: UserTier::Free,
            quota: Some(account::QuotaWindow {
                remaining_percentage: 74.5,
                reset_epoch: account::now_epoch() + 5 * 86_400 + 11 * 3_600,
            }),
            device: "DESKTOP-TAURINE".to_string(),
        });
        let text = draw(&mut state, 120, 40);
        let split = split_home(Rect::new(0, 0, 120, 40), state.account_ratio());

        assert!(split.props.width > 0, "account pane is open");
        assert!(
            split.props.x > split.center.x,
            "account pane is on the right"
        );
        assert!(split.props_divider.is_some(), "one draggable gutter");
        assert!(
            text.contains("Recent Activity"),
            "activity stays on the left"
        );
        assert!(text.contains("ACCOUNT"));
        assert!(text.contains("user@example.com"));
        assert!(text.contains("Free plan"));
        assert!(text.contains("WEEKLY QUOTA"));
        assert!(text.contains("74.5% left"));
        assert!(text.contains("Resets in 5d 11h"));
    }

    #[test]
    fn activity_test_layout_uses_a_collapsed_account_pane() {
        let state = sample_state();
        assert_eq!(state.account_ratio(), 0.0);
        let split = split_home(Rect::new(0, 0, 100, 30), state.account_ratio());
        assert_eq!(split.props.width, 0);
    }

    #[test]
    fn empty_stats_show_quiet_empty_states() {
        let text = draw(&mut HomePageState::default(), 120, 40);

        assert!(text.contains("Recent Activity"));
        assert!(text.contains("No activity yet."));
        assert!(!text.contains("No usage yet."), "most used section removed");
    }

    #[test]
    fn truncate_counts_terminal_cells_not_chars() {
        assert_eq!(truncate("日本語テキスト", 5), "日本…");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abc", 0), "");
    }
}
