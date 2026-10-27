use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike, Utc};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use taurine_core::db::crud::ActivityCategory;
use taurine_core::stats::{HomeStats, MostUsedTrigger, RecentActivity};

use crate::theme::Theme;
use crate::widgets::hotkey;
use crate::widgets::library::detail::relative_time;
use crate::widgets::panes::{self, DividerSide, PaneSplit};

pub mod state;

use self::state::{HomePageState, STAT_CARD_COUNT, format_count, format_time_saved};

const PANE_PAD_X: u16 = 2;
const CARD_HEIGHT: u16 = 4;
const CARD_GAP: u16 = 1;
const CARD_MIN_WIDTH: u16 = 16;
const MAX_CARDS_PER_ROW: usize = 2;
const MOST_USED_ROWS: usize = 8;

/// Date header plus the blank row beneath it.
const GROUP_HEADER_LINES: usize = 2;
/// Trigger line, detail line, and the blank row that follows each event.
const ITEM_LINES: usize = 3;

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

/// The home workspace is the library split with the list collapsed:
/// content on the left, recent activity in the props slot.
const HOME_LIST_RATIO: f32 = 0.0;

pub(crate) fn split_home(area: Rect, activity_ratio: f32) -> PaneSplit {
    if area.width < 5 {
        return PaneSplit {
            list: Rect::default(),
            center: area,
            props: Rect::default(),
            list_divider: None,
            props_divider: None,
        };
    }
    panes::split_panes(area, HOME_LIST_RATIO, activity_ratio)
}

pub(crate) fn visible_divider_columns(area: Rect, activity_ratio: f32) -> Vec<(DividerSide, u16)> {
    let split = split_home(area, activity_ratio);
    split
        .list_divider
        .map(|column| (DividerSide::List, column))
        .into_iter()
        .chain(
            split
                .props_divider
                .map(|column| (DividerSide::Props, column)),
        )
        .collect()
}

pub(crate) fn divider_hit(
    area: Rect,
    activity_ratio: f32,
    column: u16,
    row: u16,
) -> Option<DividerSide> {
    if row < area.y || row >= area.y.saturating_add(area.height) {
        return None;
    }
    let split = split_home(area, activity_ratio);
    (split.props_divider == Some(column)).then_some(DividerSide::Props)
}

pub(crate) fn ratio_for_column(area: Rect, column: u16) -> f32 {
    panes::detail_ratio_for_column(area, HOME_LIST_RATIO, column)
}

pub(crate) fn activity_contains(area: Rect, activity_ratio: f32, column: u16, row: u16) -> bool {
    if row < area.y || row >= area.y.saturating_add(area.height) {
        return false;
    }
    let split = split_home(area, activity_ratio);
    split.props.width > 0 && column >= split.props.x && column < split.props.right()
}

pub(crate) fn render_home_content(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    state: &mut HomePageState,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if let Some(error) = state.load_error() {
        let content = padded(area, PANE_PAD_X, 1);
        frame.render_widget(
            Paragraph::new(error).style(
                Style::default()
                    .fg(theme.error)
                    .add_modifier(Modifier::BOLD),
            ),
            content,
        );
        return;
    }

    let split = split_home(area, state.activity_ratio());
    render_center(frame, split.center, theme, state.stats());
    if split.props.width > 0 {
        let rows = activity_rows(split.props);
        let total = activity_line_count(&state.stats().recent_activity);
        state.clamp_activity_scroll(total.saturating_sub(rows.height as usize));
        state.set_activity_page(rows.height as usize);
        render_activity(
            frame,
            split.props,
            theme,
            state.stats(),
            state.activity_scroll(),
        );
    }
}

fn render_center(frame: &mut Frame, area: Rect, theme: &Theme, stats: &HomeStats) {
    let content = padded(area, PANE_PAD_X, 0);
    if content.width < 4 || content.height == 0 {
        return;
    }

    let cards_height = render_cards(frame, content, theme, stats);
    let most_used_y = content
        .y
        .saturating_add(cards_height)
        .saturating_add(u16::from(cards_height > 0));
    if most_used_y < content.bottom() {
        render_most_used(
            frame,
            Rect {
                y: most_used_y,
                height: content.bottom().saturating_sub(most_used_y),
                ..content
            },
            theme,
            stats,
        );
    }
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

/// At most two cards share a row; narrower panes stack them.
fn cards_per_row(width: u16) -> usize {
    ((width + CARD_GAP) / (CARD_MIN_WIDTH + CARD_GAP)).clamp(1, MAX_CARDS_PER_ROW as u16) as usize
}

/// Draws every card that fits and returns the height the grid used.
fn render_cards(frame: &mut Frame, area: Rect, theme: &Theme, stats: &HomeStats) -> u16 {
    if area.width == 0 || area.height < CARD_HEIGHT {
        return 0;
    }
    let cards = stat_cards(stats);
    let per_row = cards_per_row(area.width);
    let card_width = area.width.saturating_sub(CARD_GAP * (per_row as u16 - 1)) / per_row as u16;
    if card_width < 8 {
        return 0;
    }

    let mut used = 0;
    for (index, card) in cards.iter().enumerate() {
        let row = (index / per_row) as u16;
        let column = (index % per_row) as u16;
        let y = area
            .y
            .saturating_add(row.saturating_mul(CARD_HEIGHT + CARD_GAP));
        if y.saturating_add(CARD_HEIGHT) > area.bottom() {
            break;
        }
        render_card(
            frame,
            Rect {
                x: area
                    .x
                    .saturating_add(column.saturating_mul(card_width + CARD_GAP)),
                y,
                width: card_width,
                height: CARD_HEIGHT,
            },
            theme,
            card,
        );
        used = (row + 1)
            .saturating_mul(CARD_HEIGHT)
            .saturating_add(row.saturating_mul(CARD_GAP));
    }
    used
}

fn render_card(frame: &mut Frame, area: Rect, theme: &Theme, card: &StatCard) {
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
    if inner.width == 0 || inner.height < 3 {
        return;
    }

    let line = |offset: u16| Rect {
        x: inner.x,
        y: inner.y.saturating_add(offset),
        width: inner.width,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(card.label.to_uppercase()).style(Style::default().fg(theme.text_muted)),
        line(1),
    );
    frame.render_widget(
        Paragraph::new(card.value.clone())
            .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        line(2),
    );
}

fn render_most_used(frame: &mut Frame, area: Rect, theme: &Theme, stats: &HomeStats) {
    frame.render_widget(
        Paragraph::new("Most Used Triggers")
            .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        },
    );

    let rows = Rect {
        x: area.x,
        y: area.y.saturating_add(2),
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    if rows.width == 0 || rows.height == 0 {
        return;
    }

    let mut triggers = stats
        .most_used_words
        .iter()
        .chain(stats.most_used_hotkeys.iter())
        .collect::<Vec<&MostUsedTrigger>>();
    triggers.sort_by_key(|item| std::cmp::Reverse(item.uses));

    if triggers.is_empty() {
        render_muted(frame, rows, theme, "No usage yet.");
        return;
    }

    for (index, trigger) in triggers
        .iter()
        .take(MOST_USED_ROWS.min(rows.height as usize))
        .enumerate()
    {
        render_most_used_row(
            frame,
            Rect {
                x: rows.x,
                y: rows.y.saturating_add(index as u16),
                width: rows.width,
                height: 1,
            },
            theme,
            trigger,
        );
    }
}

fn render_most_used_row(frame: &mut Frame, area: Rect, theme: &Theme, trigger: &MostUsedTrigger) {
    let uses = format_count(trigger.uses);
    let uses_width = cell_width(&uses) as u16;
    let display_width = area.width.saturating_sub(uses_width.saturating_add(1)) as usize;
    let display = truncate(&trigger.display, display_width);

    let mut spans = vec![Span::styled(
        display.clone(),
        Style::default().fg(theme.text),
    )];
    spans.push(Span::raw(
        " ".repeat(display_width.saturating_sub(cell_width(&display))),
    ));
    spans.push(Span::styled(uses, Style::default().fg(theme.description)));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn activity_content(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(PANE_PAD_X),
        y: area.y,
        width: area.width.saturating_sub(PANE_PAD_X * 2),
        height: area.height,
    }
}

/// Feed viewport: the title sits two rows above, one of them blank.
fn activity_rows(area: Rect) -> Rect {
    let content = activity_content(area);
    Rect {
        y: content.y.saturating_add(3),
        height: content.height.saturating_sub(3),
        ..content
    }
}

fn render_activity(frame: &mut Frame, area: Rect, theme: &Theme, stats: &HomeStats, scroll: usize) {
    let content = activity_content(area);
    if content.width < 4 || content.height < 4 {
        return;
    }

    frame.render_widget(
        Paragraph::new("Recent Activity")
            .style(Style::default().fg(theme.text).add_modifier(Modifier::BOLD)),
        Rect {
            x: content.x,
            y: content.y.saturating_add(1),
            width: content.width,
            height: 1,
        },
    );

    let rows = activity_rows(area);
    if rows.width == 0 || rows.height == 0 {
        return;
    }
    if stats.recent_activity.is_empty() {
        render_muted(frame, rows, theme, "No activity yet.");
        return;
    }

    // honey: grouping dates is an O(n) pass per frame; move the line
    // count into HomeStats if the history ever reaches ~10k events.
    let total = activity_line_count(&stats.recent_activity);
    let viewport = rows.height as usize;
    let start = scroll.min(total.saturating_sub(viewport));
    let end = start.saturating_add(viewport);

    let today = Local::now().date_naive();
    let mut line = 0usize;
    let mut previous: Option<NaiveDate> = None;
    for event in &stats.recent_activity {
        let at = local_datetime(event.timestamp);
        let date = at.date_naive();
        if previous != Some(date) {
            if let Some(row) = visible_row(rows, start, end, line) {
                render_group_header(frame, row, theme, date, today);
            }
            line += GROUP_HEADER_LINES;
            previous = Some(date);
            if line >= end {
                break;
            }
        }
        let when = activity_time(at, today);
        let text_width = activity_text_width(rows.width, &when);
        if let Some(row) = visible_row(rows, start, end, line) {
            render_activity_trigger(frame, row, theme, event, &when, text_width);
        }
        if let Some(row) = visible_row(rows, start, end, line.saturating_add(1)) {
            render_activity_detail(frame, row, theme, &event.detail, text_width);
        }
        line += ITEM_LINES;
        if line >= end {
            break;
        }
    }
}

fn activity_line_count(activity: &[RecentActivity]) -> usize {
    let mut lines = 0usize;
    let mut previous: Option<NaiveDate> = None;
    for event in activity {
        let date = local_datetime(event.timestamp).date_naive();
        if previous != Some(date) {
            lines += GROUP_HEADER_LINES;
            previous = Some(date);
        }
        lines += ITEM_LINES;
    }
    lines
}

fn visible_row(rows: Rect, start: usize, end: usize, line: usize) -> Option<Rect> {
    (line >= start && line < end).then(|| Rect {
        x: rows.x,
        y: rows.y.saturating_add((line - start) as u16),
        width: rows.width,
        height: 1,
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

/// Text stops one cell before the right-aligned time, so nothing runs
/// underneath it.
fn activity_text_width(width: u16, when: &str) -> u16 {
    let when_width = cell_width(when) as u16;
    width.saturating_sub(when_width.saturating_add(1))
}

fn render_activity_trigger(
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
    let used = hotkey::spans_width(&spans);
    spans.push(Span::raw(" ".repeat(trigger_width.saturating_sub(used))));
    spans.push(Span::styled(
        when.to_string(),
        Style::default().fg(theme.description),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_activity_detail(frame: &mut Frame, area: Rect, theme: &Theme, detail: &str, width: u16) {
    if detail.is_empty() || width == 0 {
        return;
    }
    // honey: content can be multi-line; the preview is one flat line.
    let flat: String = detail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    frame.render_widget(
        Paragraph::new(truncate(flat.trim(), width as usize))
            .style(Style::default().fg(theme.description)),
        area,
    );
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

/// Terminal cells a string occupies; wide glyphs count as two.
fn cell_width(text: &str) -> usize {
    Span::raw(text).width()
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
    use ratatui::{Terminal, backend::TestBackend};

    use taurine_core::db::crud::{StreakInfo, TriggerType};
    use taurine_core::stats::RecentActivity;

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
                app: "Windowsterminal".to_string(),
                word_count: 42,
            }],
            most_used_words: vec![MostUsedTrigger {
                display: "gs".to_string(),
                trigger_type: TriggerType::Word,
                uses: 124,
                alias_count: 1,
            }],
            most_used_hotkeys: vec![MostUsedTrigger {
                display: "ralt+m".to_string(),
                trigger_type: TriggerType::Hotkey,
                uses: 98,
                alias_count: 1,
            }],
            top_apps: Vec::new(),
        });
        state
    }

    fn draw(state: &mut HomePageState, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_home_content(frame, area, &DARK_THEME, state))
            .expect("test draw");
        let buffer = terminal.backend().buffer();
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

    fn activity_event(timestamp: i64, trigger: &str, detail: &str) -> RecentActivity {
        RecentActivity {
            timestamp,
            category: ActivityCategory::Text,
            trigger: trigger.to_string(),
            detail: detail.to_string(),
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

    #[test]
    fn home_page_paints_no_blue() {
        let mut state = sample_state();
        let area = Rect::new(0, 0, 120, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_home_content(frame, area, &DARK_THEME, &mut state))
            .expect("test draw");
        let buffer = terminal.backend().buffer();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                let fg = buffer[(x, y)].fg;
                assert_ne!(fg, DARK_THEME.primary, "blue value at {x},{y}");
                assert_ne!(fg, DARK_THEME.accent, "blue accent at {x},{y}");
            }
        }
    }

    #[test]
    fn activity_items_have_a_blank_row_between_them() {
        let mut state = sample_state();
        let mut first = state.stats().recent_activity[0].clone();
        first.detail = "first detail".to_string();
        let mut second = first.clone();
        second.trigger = "zz".to_string();
        second.detail = "second detail".to_string();
        let mut base = state.stats().clone();
        base.recent_activity = vec![first, second];
        state.replace_stats(base);

        let area = Rect::new(0, 0, 120, 40);
        let split = split_home(area, state.activity_ratio());
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_home_content(frame, area, &DARK_THEME, &mut state))
            .expect("test draw");
        let buffer = terminal.backend().buffer();

        let pane = Rect {
            x: split.props.x.saturating_add(PANE_PAD_X),
            y: split.props.y,
            width: split.props.width.saturating_sub(PANE_PAD_X * 2),
            height: split.props.height,
        };
        let header = pane.y.saturating_add(3);
        let first_trigger = header.saturating_add(GROUP_HEADER_LINES as u16);
        let blank = |row: u16| {
            (pane.x..pane.x.saturating_add(pane.width)).all(|x| buffer[(x, row)].symbol() == " ")
        };
        assert!(!blank(header), "date header missing");
        assert!(blank(header.saturating_add(1)), "header gap missing");
        assert!(!blank(first_trigger), "first trigger missing");
        assert!(!blank(first_trigger.saturating_add(1)), "detail missing");
        assert!(
            blank(first_trigger.saturating_add(2)),
            "gap row is not blank"
        );
        assert!(
            !blank(first_trigger.saturating_add(3)),
            "second trigger missing"
        );
    }

    #[test]
    fn home_renders_cards_activity_and_most_used() {
        let mut state = sample_state();
        let text = draw(&mut state, 120, 40);

        assert!(text.contains("TIME SAVED"));
        assert!(text.contains("2h 14m"));
        assert!(text.contains("WORDS SPOKEN"));
        assert!(text.contains("12.4k"));
        assert!(text.contains("Recent Activity"));
        assert!(text.contains("Today"));
        assert!(text.contains("gs"));
        assert!(text.contains("Open taurine tui"), "activity detail missing");
        assert!(
            !text.contains("Windowsterminal"),
            "recent activity hides the source app"
        );
        assert!(!text.contains("TEXT"), "category replaced by the detail");
        assert!(!text.contains("42w"), "word count removed from activity");
        assert!(text.contains("2m ago"));
        assert!(text.contains("Most Used Triggers"));
        assert!(text.contains("ralt+m"));
        assert!(text.contains("124"));
        for border in ["┌", "┐", "└", "┘", "─"] {
            assert!(!text.contains(border), "home page draws no borders");
        }
        assert!(!text.contains("KEY"), "no trigger kind column");
        assert!(!text.contains("█"), "no usage bars");
        assert!(!text.contains("●"), "no carousel dots");
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
    fn activity_detail_truncates_with_an_ellipsis() {
        let mut state = state_with_activity(vec![activity_event(
            today_noon_millis(),
            "gs",
            &"a long description that will not fit in the narrow activity pane ".repeat(4),
        )]);
        let text = draw(&mut state, 80, 30);

        assert!(text.contains('…'), "long detail is truncated");
    }

    #[test]
    fn detail_width_follows_the_time_column() {
        assert_eq!(activity_text_width(30, "2m ago"), 23);
        assert_eq!(activity_text_width(30, "11:30 AM"), 21);
        assert_eq!(activity_text_width(3, "2m ago"), 0);
    }

    #[test]
    fn truncate_counts_terminal_cells_not_chars() {
        assert_eq!(truncate("日本語テキスト", 5), "日本…");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abc", 0), "");
    }

    /// Renders one event and hands back the activity pane buffer.
    fn render_detail(detail: &str) -> (ratatui::buffer::Buffer, Rect, u16, u16) {
        let noon = today_noon_millis();
        let mut state = state_with_activity(vec![activity_event(noon, "gs", detail)]);
        let area = Rect::new(0, 0, 120, 40);
        let split = split_home(area, state.activity_ratio());
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| render_home_content(frame, area, &DARK_THEME, &mut state))
            .expect("test draw");
        let buffer = terminal.backend().buffer().clone();

        let rows = activity_rows(split.props);
        let detail_row = rows
            .y
            .saturating_add(GROUP_HEADER_LINES as u16)
            .saturating_add(1);
        let when = activity_time(local_datetime(noon), Local::now().date_naive());
        let text_width = activity_text_width(rows.width, &when);
        (buffer, rows, detail_row, text_width)
    }

    fn assert_detail_stops_before_the_time_column(detail: &str) {
        let (buffer, rows, detail_row, text_width) = render_detail(detail);
        assert!(text_width > 0);

        let line: String = (rows.x..rows.right())
            .map(|x| buffer[(x, detail_row)].symbol())
            .collect();
        assert!(line.trim_end().ends_with('…'), "detail truncated: {line}");
        assert!(
            (rows.x.saturating_add(text_width)..rows.right())
                .all(|x| buffer[(x, detail_row)].symbol() == " "),
            "detail runs under the time column: {line}"
        );
    }

    #[test]
    fn activity_detail_stops_before_the_time_column() {
        assert_detail_stops_before_the_time_column(&"a long description ".repeat(20));
    }

    #[test]
    fn wide_detail_stops_before_the_time_column() {
        assert_detail_stops_before_the_time_column(&"描述文字内容".repeat(20));
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
    fn activity_feed_scrolls_to_the_end() {
        let noon = today_noon_millis();
        let activity = (0..40)
            .map(|index| {
                activity_event(
                    noon + index as i64 * 1_000,
                    &format!("t{index}"),
                    &format!("detail {index}"),
                )
            })
            .collect();
        let mut state = state_with_activity(activity);

        let first_page = draw(&mut state, 120, 40);
        assert!(first_page.contains("t0"));
        assert!(!first_page.contains("t39"), "oldest event starts offscreen");

        state.scroll_activity_to_end();
        let last_page = draw(&mut state, 120, 40);
        assert!(last_page.contains("t39"), "end reaches the first usage");
        assert!(!last_page.contains("t0 "), "newest event scrolled away");
        let rows =
            activity_rows(split_home(Rect::new(0, 0, 120, 40), state.activity_ratio()).props);
        let expected = GROUP_HEADER_LINES + 40 * ITEM_LINES - rows.height as usize;
        assert_eq!(
            state.activity_scroll(),
            expected,
            "scroll clamps to the last page"
        );

        state.scroll_activity_to_start();
        let first_page = draw(&mut state, 120, 40);
        assert!(first_page.contains("t0"));
    }

    #[test]
    fn activity_line_count_includes_group_headers() {
        use chrono::{Local, TimeZone};

        let old = Local
            .with_ymd_and_hms(2024, 10, 20, 11, 30, 0)
            .single()
            .expect("local time")
            .timestamp_millis();
        let noon = today_noon_millis();
        let activity = vec![
            activity_event(noon + 1_000, "b", "detail"),
            activity_event(noon, "a", "detail"),
            activity_event(old, "c", "detail"),
        ];

        assert_eq!(
            activity_line_count(&activity),
            (GROUP_HEADER_LINES + 2 * ITEM_LINES) + (GROUP_HEADER_LINES + ITEM_LINES)
        );
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
    fn home_uses_the_shared_two_pane_split() {
        let area = Rect::new(0, 0, 120, 40);
        let split = split_home(area, state::DEFAULT_ACTIVITY_RATIO);

        assert!(split.center.width >= 28);
        assert!(split.props.width >= 24);
        assert_eq!(
            split.props_divider,
            Some(split.center.x.saturating_add(split.center.width))
        );
        assert_eq!(visible_divider_columns(area, 0.3).len(), 1);
        assert_eq!(
            divider_hit(
                area,
                0.3,
                split.props_divider.expect("activity divider"),
                area.y.saturating_add(1),
            ),
            Some(DividerSide::Props)
        );
    }

    #[test]
    fn narrow_terminals_keep_the_content_pane() {
        let text = draw(&mut sample_state(), 60, 24);

        assert!(text.contains("TIME SAVED"));
        assert!(text.contains("Most Used Triggers"));
        assert!(text.contains("Recent"));
    }

    #[test]
    fn empty_stats_show_quiet_empty_states() {
        let text = draw(&mut HomePageState::default(), 120, 40);

        assert!(text.contains("No activity yet."));
        assert!(text.contains("No usage yet."));
        assert!(text.contains("0s"));
    }
}
