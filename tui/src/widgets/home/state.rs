use ratatui::layout::Rect;
use taurine_core::stats::HomeStats;

/// Stat cards rendered above the activity feed.
pub(crate) const STAT_CARD_COUNT: usize = 4;

const REFRESH_INTERVAL_MS: u64 = 10_000;

/// One drawn activity line, recorded so the pointer can hit it. `copy` is
/// set only on the body line, which carries the copy button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ActivityHit {
    pub(crate) event: usize,
    pub(crate) area: Rect,
    pub(crate) copy: Option<Rect>,
}

#[derive(Debug, Default)]
pub(crate) struct HomePageState {
    stats: HomeStats,
    load_error: Option<String>,
    selected: Option<usize>,
    scroll: usize,
    page: usize,
    hits: Vec<ActivityHit>,
    last_refresh_ms: u64,
    refresh_requested: bool,
}

impl HomePageState {
    pub(crate) const fn stats(&self) -> &HomeStats {
        &self.stats
    }

    pub(crate) fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    pub(crate) fn replace_stats(&mut self, stats: HomeStats) {
        self.stats = stats;
        self.load_error = None;
        self.selected = self.clamp_selection(self.selected.unwrap_or(0));
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub(crate) const fn selected(&self) -> Option<usize> {
        self.selected
    }

    fn clamp_selection(&self, index: usize) -> Option<usize> {
        let len = self.stats.recent_activity.len();
        (len > 0).then(|| index.min(len - 1))
    }

    pub(crate) fn select(&mut self, index: usize) {
        self.selected = self.clamp_selection(index);
    }

    /// Moves the cursor between events; the render keeps it in view.
    pub(crate) fn move_selection(&mut self, delta: isize) {
        let Some(current) = self.selected else {
            self.selected = self.clamp_selection(0);
            return;
        };
        let next = if delta >= 0 {
            current.saturating_add(delta as usize)
        } else {
            current.saturating_sub(delta.unsigned_abs())
        };
        self.selected = self.clamp_selection(next);
    }

    /// PageUp/PageDown step, using the viewport height the last frame drew.
    pub(crate) fn page_selection(&mut self, delta: isize) {
        let page = self.page.max(1) as isize;
        self.move_selection(delta.saturating_mul(page));
    }

    pub(crate) fn select_first(&mut self) {
        self.selected = self.clamp_selection(0);
    }

    pub(crate) fn select_last(&mut self) {
        let len = self.stats.recent_activity.len();
        self.selected = len.checked_sub(1);
    }

    pub(crate) const fn scroll(&self) -> usize {
        self.scroll
    }

    /// Viewport height recorded by the last render, used as the page step.
    pub(crate) const fn set_page(&mut self, page: usize) {
        self.page = page;
    }

    pub(crate) const fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    /// Scrolls the document just enough to show `start..end`, then clamps
    /// to the last full page so the cards can scroll fully out of view.
    pub(crate) fn reveal(&mut self, start: usize, end: usize, viewport: usize, max: usize) {
        if start < self.scroll {
            self.scroll = start;
        } else if end > self.scroll.saturating_add(viewport) {
            self.scroll = end.saturating_sub(viewport);
        }
        self.scroll = self.scroll.min(max);
    }

    pub(crate) fn set_hits(&mut self, hits: Vec<ActivityHit>) {
        self.hits = hits;
    }

    /// Event under the pointer, if any drawn line covers it.
    pub(crate) fn event_at(&self, column: u16, row: u16) -> Option<usize> {
        self.hits
            .iter()
            .find(|hit| contains(hit.area, column, row))
            .map(|hit| hit.event)
    }

    /// Event whose copy button covers the pointer.
    pub(crate) fn copy_at(&self, column: u16, row: u16) -> Option<usize> {
        self.hits.iter().find_map(|hit| {
            let button = hit.copy?;
            contains(button, column, row).then_some(hit.event)
        })
    }

    pub(crate) fn refresh_due(&self, now_ms: u64) -> bool {
        self.refresh_requested || now_ms.saturating_sub(self.last_refresh_ms) >= REFRESH_INTERVAL_MS
    }

    pub(crate) fn mark_refreshed(&mut self, now_ms: u64) {
        self.last_refresh_ms = now_ms;
        self.refresh_requested = false;
    }

    /// Page became visible: request a fresh load on the next tick.
    pub(crate) fn on_show(&mut self) {
        self.refresh_requested = true;
    }
}

fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x && column < area.right() && row >= area.y && row < area.bottom()
}

/// Compact counts for the stat cards: grouped below ten thousand,
/// abbreviated above it so a card never overflows its value line.
pub(crate) fn format_count(n: u64) -> String {
    if n < 10_000 {
        return group_thousands(n);
    }
    if n < 1_000_000 {
        return compact(n as f64 / 1_000.0, "k");
    }
    compact(n as f64 / 1_000_000.0, "M")
}

fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn compact(value: f64, suffix: &str) -> String {
    let rounded = format!("{value:.1}");
    let trimmed = rounded.strip_suffix(".0").unwrap_or(&rounded);
    format!("{trimmed}{suffix}")
}

/// Estimated time saved, from seconds up to whole days.
pub(crate) fn format_time_saved(ms: u64) -> String {
    let secs = ms / 1_000;
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3_600 {
        return format!("{}m", secs / 60);
    }
    if secs < 86_400 {
        let hours = secs / 3_600;
        let minutes = (secs % 3_600) / 60;
        return if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {minutes}m")
        };
    }
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    if hours == 0 {
        format!("{days}d")
    } else {
        format!("{days}d {hours}h")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use taurine_core::stats::RecentActivity;

    fn event(trigger: &str) -> RecentActivity {
        RecentActivity {
            timestamp: 0,
            category: taurine_core::db::crud::ActivityCategory::Text,
            trigger: trigger.to_string(),
            detail: String::new(),
            content: None,
            app: String::new(),
            word_count: 0,
        }
    }

    fn state_with(len: usize) -> HomePageState {
        let mut state = HomePageState::default();
        state.replace_stats(HomeStats {
            recent_activity: (0..len).map(|i| event(&format!("t{i}"))).collect(),
            ..HomeStats::default()
        });
        state
    }

    #[test]
    fn refresh_is_due_after_the_interval() {
        let mut state = HomePageState::default();
        assert!(state.refresh_due(10_000));

        state.mark_refreshed(10_000);
        assert!(!state.refresh_due(19_999));
        assert!(state.refresh_due(20_000));

        state.mark_refreshed(20_000);
        state.on_show();
        assert!(state.refresh_due(20_001));
        state.mark_refreshed(20_001);
        assert!(!state.refresh_due(20_002));
    }

    #[test]
    fn selection_starts_on_the_newest_event_and_clamps() {
        let mut state = state_with(3);
        assert_eq!(state.selected(), Some(0));

        state.move_selection(-1);
        assert_eq!(state.selected(), Some(0), "top of the feed stops");

        state.move_selection(10);
        assert_eq!(state.selected(), Some(2), "bottom of the feed stops");

        state.select_last();
        assert_eq!(state.selected(), Some(2));
        state.select_first();
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn page_selection_steps_by_the_viewport() {
        let mut state = state_with(50);
        state.set_page(10);

        state.page_selection(1);
        assert_eq!(state.selected(), Some(10));
        state.page_selection(-1);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn empty_activity_has_no_selection() {
        let mut state = state_with(0);
        assert_eq!(state.selected(), None);

        state.move_selection(1);
        assert_eq!(state.selected(), None);
        state.select_last();
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn shrinking_the_feed_keeps_the_selection_in_range() {
        let mut state = state_with(10);
        state.select_last();
        assert_eq!(state.selected(), Some(9));

        state.replace_stats(HomeStats {
            recent_activity: vec![event("only")],
            ..HomeStats::default()
        });
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn reveal_scrolls_only_when_the_selection_leaves_the_viewport() {
        let mut state = HomePageState::default();

        state.reveal(5, 8, 10, 100);
        assert_eq!(state.scroll(), 0, "already visible");

        state.reveal(20, 23, 10, 100);
        assert_eq!(state.scroll(), 13, "selection at the bottom edge");

        state.reveal(2, 5, 10, 100);
        assert_eq!(state.scroll(), 2, "selection at the top edge");

        state.reveal(90, 93, 10, 40);
        assert_eq!(state.scroll(), 40, "never past the last page");
    }

    #[test]
    fn hit_testing_finds_rows_and_copy_buttons() {
        let mut state = HomePageState::default();
        let row = Rect::new(0, 10, 80, 1);
        let button = Rect::new(70, 11, 6, 1);
        state.set_hits(vec![
            ActivityHit {
                event: 3,
                area: row,
                copy: None,
            },
            ActivityHit {
                event: 3,
                area: Rect::new(0, 11, 80, 1),
                copy: Some(button),
            },
        ]);

        assert_eq!(state.event_at(4, 10), Some(3));
        assert_eq!(state.event_at(4, 11), Some(3));
        assert_eq!(state.event_at(4, 12), None);
        assert_eq!(state.copy_at(72, 11), Some(3));
        assert_eq!(state.copy_at(4, 11), None, "body text is not the button");
    }

    #[test]
    fn counts_abbreviate_only_above_ten_thousand() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(999), "999");
        assert_eq!(format_count(1_000), "1,000");
        assert_eq!(format_count(9_999), "9,999");
        assert_eq!(format_count(10_000), "10k");
        assert_eq!(format_count(12_400), "12.4k");
        assert_eq!(format_count(1_200_000), "1.2M");
    }

    #[test]
    fn time_saved_scales_from_seconds_to_days() {
        assert_eq!(format_time_saved(0), "0s");
        assert_eq!(format_time_saved(42_000), "42s");
        assert_eq!(format_time_saved(60_000), "1m");
        assert_eq!(format_time_saved(3_600_000), "1h");
        assert_eq!(format_time_saved(5_400_000), "1h 30m");
        assert_eq!(format_time_saved(90_000_000), "1d 1h");
    }
}
