use ratatui::layout::Rect;
use taurine_core::stats::HomeStats;

const REFRESH_INTERVAL_MS: u64 = 10_000;

/// A drawn activity card, recorded so the pointer can highlight it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardHit {
    pub(crate) event: usize,
    pub(crate) area: Rect,
}

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
    selected: Option<usize>,
    hovered: Option<usize>,
    selection_moved: bool,
    page_cards: usize,
    scroll: usize,
    card_hits: Vec<CardHit>,
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

    /// Replaces the feed and keeps the selection on a real card. A fresh
    /// feed with nothing selected starts on the newest entry.
    pub(crate) fn replace_stats(&mut self, stats: HomeStats) {
        self.stats = stats;
        self.load_error = None;
        self.hovered = None;
        let len = self.stats.recent_activity.len();
        self.selected = match (self.selected, len) {
            (_, 0) => None,
            (Some(index), _) => Some(index.min(len - 1)),
            (None, _) => Some(0),
        };
    }

    pub(crate) fn set_load_error(&mut self, error: String) {
        self.load_error = Some(error);
    }

    pub(crate) const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Card that owns the copy button: the pointer's card when hovering,
    /// otherwise the keyboard selection.
    pub(crate) fn active(&self) -> Option<usize> {
        self.hovered.or(self.selected)
    }

    pub(crate) const fn selection_moved(&self) -> bool {
        self.selection_moved
    }

    pub(crate) const fn clear_selection_moved(&mut self) {
        self.selection_moved = false;
    }

    pub(crate) fn select(&mut self, index: usize) {
        let Some(last) = self.stats.recent_activity.len().checked_sub(1) else {
            self.selected = None;
            return;
        };
        self.selected = Some(index.min(last));
        self.selection_moved = true;
    }

    pub(crate) fn select_by(&mut self, delta: isize) {
        let len = self.stats.recent_activity.len();
        if len == 0 {
            self.selected = None;
            return;
        }
        let current = self.selected.unwrap_or(0) as isize;
        let next = current.saturating_add(delta).clamp(0, len as isize - 1);
        self.selected = Some(next as usize);
        self.selection_moved = true;
    }

    pub(crate) fn select_first(&mut self) {
        if !self.stats.recent_activity.is_empty() {
            self.select(0);
        }
    }

    pub(crate) fn select_last(&mut self) {
        if let Some(last) = self.stats.recent_activity.len().checked_sub(1) {
            self.select(last);
        }
    }

    /// PageUp/PageDown step in cards, using the cards-per-page the last
    /// frame drew.
    pub(crate) fn select_page(&mut self, delta: isize) {
        let step = self.page_cards.max(1) as isize;
        self.select_by(delta.saturating_mul(step));
    }

    pub(crate) const fn set_page_cards(&mut self, cards: usize) {
        self.page_cards = cards;
    }

    pub(crate) const fn scroll(&self) -> usize {
        self.scroll
    }

    pub(crate) const fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
    }

    pub(crate) fn scroll_by(&mut self, delta: isize) {
        self.scroll = if delta >= 0 {
            self.scroll.saturating_add(delta as usize)
        } else {
            self.scroll.saturating_sub(delta.unsigned_abs())
        };
    }

    pub(crate) fn set_card_hits(&mut self, hits: Vec<CardHit>) {
        self.card_hits = hits;
    }

    pub(crate) fn set_copy_hits(&mut self, hits: Vec<CopyHit>) {
        self.copy_hits = hits;
    }

    /// Card under the pointer, used for hover highlighting.
    pub(crate) fn hover_at(&mut self, column: u16, row: u16) {
        self.hovered = self.card_at(column, row);
    }

    pub(crate) const fn clear_hover(&mut self) {
        self.hovered = None;
    }

    pub(crate) fn card_at(&self, column: u16, row: u16) -> Option<usize> {
        self.card_hits
            .iter()
            .find(|hit| contains(hit.area, column, row))
            .map(|hit| hit.event)
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
    use taurine_core::db::crud::ActivityCategory;
    use taurine_core::stats::RecentActivity;

    fn event(index: usize) -> RecentActivity {
        RecentActivity {
            timestamp: index as i64,
            category: ActivityCategory::Text,
            trigger: format!("t{index}"),
            detail: String::new(),
            content: Some(format!("body {index}")),
            app: String::new(),
            word_count: 0,
        }
    }

    fn state_with_events(count: usize) -> HomePageState {
        let mut state = HomePageState::default();
        state.replace_stats(HomeStats {
            recent_activity: (0..count).map(event).collect(),
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
    fn a_fresh_feed_selects_the_newest_card() {
        let state = state_with_events(3);
        assert_eq!(state.selected(), Some(0));

        let mut empty = HomePageState::default();
        empty.replace_stats(HomeStats::default());
        assert_eq!(empty.selected(), None);
    }

    #[test]
    fn selection_moves_and_stops_at_the_ends() {
        let mut state = state_with_events(3);

        state.select_by(1);
        assert_eq!(state.selected(), Some(1));
        state.select_by(10);
        assert_eq!(state.selected(), Some(2), "bottom stops");
        state.select_by(-1);
        assert_eq!(state.selected(), Some(1));
        state.select_first();
        assert_eq!(state.selected(), Some(0));
        state.select_by(-10);
        assert_eq!(state.selected(), Some(0), "top stops");
        state.select_last();
        assert_eq!(state.selected(), Some(2));
        assert!(state.selection_moved());
        state.clear_selection_moved();
        assert!(!state.selection_moved());
    }

    #[test]
    fn page_selection_steps_by_visible_cards() {
        let mut state = state_with_events(20);
        state.set_page_cards(5);

        state.select_page(1);
        assert_eq!(state.selected(), Some(5));
        state.select_page(-1);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn shrinking_feed_clamps_the_selection() {
        let mut state = state_with_events(5);
        state.select_last();

        state.replace_stats(HomeStats {
            recent_activity: (0..2).map(event).collect(),
            ..HomeStats::default()
        });
        assert_eq!(state.selected(), Some(1));

        state.replace_stats(HomeStats::default());
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn active_prefers_the_hovered_card() {
        let mut state = state_with_events(3);
        state.select(2);
        state.set_card_hits(vec![
            CardHit {
                event: 1,
                area: Rect::new(0, 10, 60, 4),
            },
            CardHit {
                event: 2,
                area: Rect::new(0, 20, 60, 4),
            },
        ]);

        assert_eq!(state.active(), Some(2));
        state.hover_at(10, 11);
        assert_eq!(state.active(), Some(1));
        state.clear_hover();
        assert_eq!(state.active(), Some(2));
    }

    #[test]
    fn hit_testing_separates_cards_from_copy_buttons() {
        let mut state = HomePageState::default();
        state.set_card_hits(vec![CardHit {
            event: 3,
            area: Rect::new(2, 10, 60, 4),
        }]);
        state.set_copy_hits(vec![CopyHit {
            event: 3,
            area: Rect::new(55, 12, 6, 1),
        }]);

        assert_eq!(state.card_at(3, 10), Some(3));
        assert_eq!(state.card_at(3, 14), None, "gap row is not a card");
        assert_eq!(state.copy_at(56, 12), Some(3));
        assert_eq!(state.copy_at(3, 12), None, "body text is not the button");
    }
}
