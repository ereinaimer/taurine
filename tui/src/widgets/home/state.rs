use taurine_core::stats::HomeStats;

use crate::widgets::panes::{DividerPointer, DividerSide};

/// Stat cards rendered above the most-used list.
pub(crate) const STAT_CARD_COUNT: usize = 4;

/// Activity pane's default share of the content remainder.
pub(crate) const DEFAULT_ACTIVITY_RATIO: f32 = 0.3;
pub(crate) const MIN_ACTIVITY_RATIO: f32 = 0.0;
pub(crate) const MAX_ACTIVITY_RATIO: f32 = 2.0;

const REFRESH_INTERVAL_MS: u64 = 10_000;

#[derive(Debug)]
pub(crate) struct HomePageState {
    stats: HomeStats,
    load_error: Option<String>,
    activity_ratio: f32,
    activity_scroll: usize,
    activity_page: usize,
    dividers: DividerPointer,
    last_refresh_ms: u64,
    refresh_requested: bool,
}

impl Default for HomePageState {
    fn default() -> Self {
        Self {
            stats: HomeStats::default(),
            load_error: None,
            activity_ratio: DEFAULT_ACTIVITY_RATIO,
            activity_scroll: 0,
            activity_page: 0,
            dividers: DividerPointer::default(),
            last_refresh_ms: 0,
            refresh_requested: false,
        }
    }
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

    pub(crate) fn activity_ratio(&self) -> f32 {
        self.activity_ratio
            .clamp(MIN_ACTIVITY_RATIO, MAX_ACTIVITY_RATIO)
    }

    pub(crate) fn set_activity_ratio(&mut self, ratio: f32) {
        self.activity_ratio = ratio.clamp(MIN_ACTIVITY_RATIO, MAX_ACTIVITY_RATIO);
    }

    pub(crate) const fn activity_scroll(&self) -> usize {
        self.activity_scroll
    }

    /// Viewport height recorded by the last render, used as the
    /// PageUp/PageDown step.
    pub(crate) const fn activity_page(&self) -> usize {
        self.activity_page
    }

    pub(crate) const fn set_activity_page(&mut self, page: usize) {
        self.activity_page = page;
    }

    pub(crate) fn scroll_activity(&mut self, delta: isize) {
        self.activity_scroll = if delta >= 0 {
            self.activity_scroll.saturating_add(delta as usize)
        } else {
            self.activity_scroll.saturating_sub(delta.unsigned_abs())
        };
    }

    pub(crate) fn scroll_activity_to_start(&mut self) {
        self.activity_scroll = 0;
    }

    pub(crate) fn scroll_activity_to_end(&mut self) {
        self.activity_scroll = usize::MAX;
    }

    /// Render clamps the offset to the last full page so overscroll
    /// never strands the view past the end.
    pub(crate) fn clamp_activity_scroll(&mut self, max: usize) {
        self.activity_scroll = self.activity_scroll.min(max);
    }

    pub(crate) const fn dividers(&self) -> &DividerPointer {
        &self.dividers
    }

    pub(crate) const fn divider_drag(&self) -> Option<DividerSide> {
        self.dividers.drag()
    }

    pub(crate) fn set_divider_hover(&mut self, hover: Option<DividerSide>) {
        self.dividers.set_hover(hover);
    }

    pub(crate) fn set_divider_drag(&mut self, drag: Option<DividerSide>) {
        self.dividers.set_drag(drag);
    }

    /// Second click on the divider toggles the activity pane: a pane at
    /// its default width collapses, a collapsed pane restores the
    /// default, and any custom width resets to default. Returns true
    /// when the width changed.
    pub(crate) fn divider_double_click_at(&mut self, side: DividerSide, now: u64) -> bool {
        if !self.dividers.double_click_at(side, now) {
            return false;
        }
        // honey: drag math rarely lands exactly on a constant, so
        // near-enough counts for both ends of the toggle.
        const NEAR: f32 = 0.0001;
        let ratio = self.activity_ratio();
        if ratio <= MIN_ACTIVITY_RATIO + NEAR {
            self.set_activity_ratio(DEFAULT_ACTIVITY_RATIO);
        } else if (ratio - DEFAULT_ACTIVITY_RATIO).abs() <= NEAR {
            self.set_activity_ratio(MIN_ACTIVITY_RATIO);
        } else {
            self.set_activity_ratio(DEFAULT_ACTIVITY_RATIO);
        }
        true
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
    use crate::widgets::panes::DIVIDER_DOUBLE_CLICK_MS;

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
    fn activity_ratio_clamps_to_its_range() {
        let mut state = HomePageState::default();
        assert_eq!(state.activity_ratio(), DEFAULT_ACTIVITY_RATIO);

        state.set_activity_ratio(-1.0);
        assert_eq!(state.activity_ratio(), MIN_ACTIVITY_RATIO);

        state.set_activity_ratio(9.0);
        assert_eq!(state.activity_ratio(), MAX_ACTIVITY_RATIO);
    }

    #[test]
    fn divider_double_click_toggles_default_and_collapsed() {
        let mut state = HomePageState::default();
        let side = DividerSide::Props;

        assert!(!state.divider_double_click_at(side, 1_000));
        assert!(state.divider_double_click_at(side, 1_100));
        assert_eq!(state.activity_ratio(), MIN_ACTIVITY_RATIO);

        assert!(!state.divider_double_click_at(side, 2_000));
        assert!(state.divider_double_click_at(side, 2_100));
        assert_eq!(state.activity_ratio(), DEFAULT_ACTIVITY_RATIO);

        state.set_activity_ratio(0.9);
        assert!(!state.divider_double_click_at(side, 3_000));
        assert!(state.divider_double_click_at(side, 3_000 + DIVIDER_DOUBLE_CLICK_MS));
        assert_eq!(
            state.activity_ratio(),
            DEFAULT_ACTIVITY_RATIO,
            "a custom width resets to default"
        );
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
