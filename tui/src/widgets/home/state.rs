use ratatui::layout::Rect;
use taurine_core::stats::HomeStats;

/// Stat cards rendered above the activity feed.
pub(crate) const STAT_CARD_COUNT: usize = 4;

const REFRESH_INTERVAL_MS: u64 = 10_000;

/// A drawn copy button, recorded so the pointer can find it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CopyHit {
    pub(crate) event: usize,
    pub(crate) area: Rect,
}

#[derive(Debug, Default)]
pub(crate) struct HomePageState {
    stats: HomeStats,
    load_error: Option<String>,
    scroll: usize,
    page: usize,
    copy_hits: Vec<CopyHit>,
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
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub(crate) const fn scroll(&self) -> usize {
        self.scroll
    }

    pub(crate) const fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    /// Viewport height recorded by the last render, used as the page step.
    pub(crate) const fn set_page(&mut self, page: usize) {
        self.page = page;
    }

    pub(crate) fn scroll_by(&mut self, delta: isize) {
        self.scroll = if delta >= 0 {
            self.scroll.saturating_add(delta as usize)
        } else {
            self.scroll.saturating_sub(delta.unsigned_abs())
        };
    }

    /// PageUp/PageDown step, using the viewport height the last frame drew.
    pub(crate) fn scroll_page(&mut self, delta: isize) {
        let page = self.page.max(1) as isize;
        self.scroll_by(delta.saturating_mul(page));
    }

    pub(crate) const fn scroll_to_start(&mut self) {
        self.scroll = 0;
    }

    pub(crate) const fn scroll_to_end(&mut self) {
        self.scroll = usize::MAX;
    }

    pub(crate) fn set_copy_hits(&mut self, hits: Vec<CopyHit>) {
        self.copy_hits = hits;
    }

    /// Event whose copy button covers the pointer.
    pub(crate) fn copy_at(&self, column: u16, row: u16) -> Option<usize> {
        self.copy_hits
            .iter()
            .find(|hit| contains(hit.area, column, row))
            .map(|hit| hit.event)
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
    fn scroll_steps_and_stops_at_the_top() {
        let mut state = HomePageState::default();

        state.scroll_by(3);
        assert_eq!(state.scroll(), 3);
        state.scroll_by(-1);
        assert_eq!(state.scroll(), 2);
        state.scroll_by(-10);
        assert_eq!(state.scroll(), 0, "top of the feed stops");
    }

    #[test]
    fn page_scroll_steps_by_the_viewport() {
        let mut state = HomePageState::default();
        state.set_page(10);

        state.scroll_page(1);
        assert_eq!(state.scroll(), 10);
        state.scroll_page(-1);
        assert_eq!(state.scroll(), 0);
    }

    #[test]
    fn end_jumps_past_every_page_and_home_returns() {
        let mut state = HomePageState::default();

        state.scroll_to_end();
        assert_eq!(state.scroll(), usize::MAX);
        state.scroll_to_start();
        assert_eq!(state.scroll(), 0);
    }

    #[test]
    fn hit_testing_finds_only_copy_buttons() {
        let mut state = HomePageState::default();
        state.set_copy_hits(vec![
            CopyHit {
                event: 3,
                area: Rect::new(70, 11, 6, 1),
            },
            CopyHit {
                event: 4,
                area: Rect::new(70, 20, 6, 1),
            },
        ]);

        assert_eq!(state.copy_at(72, 11), Some(3));
        assert_eq!(state.copy_at(72, 20), Some(4));
        assert_eq!(state.copy_at(4, 11), None, "body text is not the button");
        assert_eq!(state.copy_at(72, 12), None);
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
