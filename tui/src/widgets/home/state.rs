use ratatui::layout::Rect;
use taurine_core::stats::HomeStats;

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
}
