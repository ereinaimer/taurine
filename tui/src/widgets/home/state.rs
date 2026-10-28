use ratatui::layout::Rect;
use taurine_core::stats::HomeStats;

use crate::widgets::home::account::{self, AccountInfo};
use crate::widgets::panes::{DividerPointer, DividerSide};

const REFRESH_INTERVAL_MS: u64 = 10_000;
/// How long the copy button shows its check after a successful copy.
pub(crate) const COPY_FEEDBACK_MS: u64 = 1_200;
/// Lines advanced per wheel notch before easing catches up.
const SCROLL_STEP_LINES: usize = 3;
/// Share of the remaining distance covered each animation frame.
const SCROLL_EASE: f32 = 0.3;

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

#[derive(Debug)]
pub(crate) struct HomePageState {
    stats: HomeStats,
    account: AccountInfo,
    load_error: Option<String>,
    selected: Option<usize>,
    hovered: Option<usize>,
    selection_moved: bool,
    page_cards: usize,
    scroll: usize,
    scroll_target: usize,
    scroll_animating: bool,
    max_scroll: usize,
    account_ratio: f32,
    dividers: DividerPointer,
    copied: Option<(usize, u64)>,
    card_hits: Vec<CardHit>,
    copy_hits: Vec<CopyHit>,
    last_refresh_ms: u64,
    refresh_requested: bool,
}

impl Default for HomePageState {
    fn default() -> Self {
        Self {
            stats: HomeStats::default(),
            account: AccountInfo::default(),
            load_error: None,
            selected: None,
            hovered: None,
            selection_moved: false,
            page_cards: 0,
            scroll: 0,
            scroll_target: 0,
            scroll_animating: false,
            max_scroll: 0,
            account_ratio: account::DEFAULT_RATIO,
            dividers: DividerPointer::default(),
            copied: None,
            card_hits: Vec::new(),
            copy_hits: Vec::new(),
            last_refresh_ms: 0,
            refresh_requested: false,
        }
    }
}

impl HomePageState {
    pub(crate) const fn stats(&self) -> &HomeStats {
        &self.stats
    }

    pub(crate) const fn account(&self) -> &AccountInfo {
        &self.account
    }

    pub(crate) fn replace_account(&mut self, account: AccountInfo) {
        self.account = account;
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
        self.copied = None;
        self.scroll_target = self.scroll;
        self.scroll_animating = false;
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

    pub(crate) fn account_ratio(&self) -> f32 {
        self.account_ratio
            .clamp(account::MIN_RATIO, account::MAX_RATIO)
    }

    pub(crate) fn set_account_ratio(&mut self, ratio: f32) {
        self.account_ratio = ratio.clamp(account::MIN_RATIO, account::MAX_RATIO);
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

    /// Double-click toggles the account pane: default width collapses, a
    /// collapsed pane restores the default, and any custom width resets.
    pub(crate) fn divider_double_click_at(&mut self, side: DividerSide, now: u64) -> bool {
        if !self.dividers.double_click_at(side, now) {
            return false;
        }
        // honey: drag math rarely lands exactly on a constant, so
        // near-enough counts for both ends of the toggle.
        const NEAR: f32 = 0.0001;
        let ratio = self.account_ratio();
        if ratio <= account::MIN_RATIO + NEAR {
            self.set_account_ratio(account::DEFAULT_RATIO);
        } else if (ratio - account::DEFAULT_RATIO).abs() <= NEAR {
            self.set_account_ratio(account::MIN_RATIO);
        } else {
            self.set_account_ratio(account::DEFAULT_RATIO);
        }
        true
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

    pub(crate) const fn scroll_animating(&self) -> bool {
        self.scroll_animating
    }

    pub(crate) const fn set_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
        if !self.scroll_animating {
            self.scroll_target = scroll;
        }
    }

    /// Clamp the wheel target and the current position to the viewport.
    pub(crate) fn set_max_scroll(&mut self, max: usize) {
        self.max_scroll = max;
        self.scroll_target = self.scroll_target.min(max);
        self.scroll = self.scroll.min(max);
    }

    /// Snap the scroll, cancelling any in-flight wheel glide.
    pub(crate) fn jump_scroll(&mut self, scroll: usize) {
        self.scroll = scroll;
        self.scroll_target = scroll;
        self.scroll_animating = false;
    }

    /// Queue a wheel notch; the next frames ease toward the target.
    pub(crate) fn smooth_scroll(&mut self, notches: isize) {
        let step = SCROLL_STEP_LINES as isize * notches;
        let target = if step >= 0 {
            self.scroll_target.saturating_add(step as usize)
        } else {
            self.scroll_target.saturating_sub(step.unsigned_abs())
        };
        self.scroll_target = target.min(self.max_scroll);
        self.scroll_animating = self.scroll_target != self.scroll;
    }

    /// One animation frame: move a fraction of the remaining distance.
    pub(crate) fn advance_scroll(&mut self) -> usize {
        if !self.scroll_animating {
            return self.scroll;
        }
        let target = self.scroll_target.min(self.max_scroll);
        if self.scroll == target {
            self.scroll_animating = false;
            return self.scroll;
        }
        let diff = target as isize - self.scroll as isize;
        let step = ((diff as f32) * SCROLL_EASE).round() as isize;
        let step = if step == 0 { diff.signum() } else { step };
        let next = (self.scroll as isize + step).clamp(0, self.max_scroll as isize) as usize;
        self.scroll = next;
        if next == target {
            self.scroll_animating = false;
        }
        next
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

    /// Flash the check mark on a card's copy button after a copy.
    pub(crate) fn mark_copied(&mut self, index: usize, now_ms: u64) {
        self.copied = Some((index, now_ms.saturating_add(COPY_FEEDBACK_MS)));
    }

    /// Card whose copy button should show the check right now.
    pub(crate) fn copied_event(&self, now_ms: u64) -> Option<usize> {
        self.copied
            .filter(|(_, until)| now_ms < *until)
            .map(|(index, _)| index)
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
    fn smooth_scroll_eases_toward_the_target() {
        let mut state = HomePageState::default();
        state.set_max_scroll(100);
        state.smooth_scroll(1);
        assert!(state.scroll_animating());

        let mut last = state.scroll();
        for _ in 0..10 {
            let next = state.advance_scroll();
            assert!(next >= last, "scrolls forward");
            assert!(next - last <= SCROLL_STEP_LINES, "one small step per frame");
            last = next;
            if !state.scroll_animating() {
                break;
            }
        }
        assert_eq!(state.scroll(), SCROLL_STEP_LINES);
        assert!(!state.scroll_animating());

        state.smooth_scroll(-1);
        for _ in 0..10 {
            state.advance_scroll();
            if !state.scroll_animating() {
                break;
            }
        }
        assert_eq!(state.scroll(), 0);
    }

    #[test]
    fn smooth_scroll_clamps_at_the_ends() {
        let mut state = HomePageState::default();
        state.set_max_scroll(2);
        state.smooth_scroll(1);
        while state.scroll_animating() {
            state.advance_scroll();
        }
        assert_eq!(state.scroll(), 2, "max clamps the target");

        state.jump_scroll(2);
        state.smooth_scroll(-1);
        while state.scroll_animating() {
            state.advance_scroll();
        }
        assert_eq!(state.scroll(), 0);
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
    fn account_ratio_clamps_to_its_range() {
        let mut state = HomePageState::default();
        assert_eq!(state.account_ratio(), account::DEFAULT_RATIO);

        state.set_account_ratio(-1.0);
        assert_eq!(state.account_ratio(), account::MIN_RATIO);

        state.set_account_ratio(9.0);
        assert_eq!(state.account_ratio(), account::MAX_RATIO);
    }

    #[test]
    fn divider_double_click_toggles_default_and_collapsed() {
        use crate::widgets::panes::{DIVIDER_DOUBLE_CLICK_MS, DividerSide};

        let mut state = HomePageState::default();
        let side = DividerSide::Props;

        assert!(!state.divider_double_click_at(side, 1_000));
        assert!(state.divider_double_click_at(side, 1_100));
        assert_eq!(state.account_ratio(), account::MIN_RATIO);

        assert!(!state.divider_double_click_at(side, 2_000));
        assert!(state.divider_double_click_at(side, 2_100));
        assert_eq!(state.account_ratio(), account::DEFAULT_RATIO);

        state.set_account_ratio(0.9);
        assert!(!state.divider_double_click_at(side, 3_000));
        assert!(state.divider_double_click_at(side, 3_000 + DIVIDER_DOUBLE_CLICK_MS));
        assert_eq!(
            state.account_ratio(),
            account::DEFAULT_RATIO,
            "a custom width resets to default"
        );
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

    #[test]
    fn copy_feedback_lasts_for_the_flash_window() {
        let mut state = HomePageState::default();
        state.mark_copied(2, 5_000);

        assert_eq!(state.copied_event(5_000), Some(2));
        assert_eq!(state.copied_event(6_199), Some(2));
        assert_eq!(state.copied_event(6_200), None, "flash expires");

        state.replace_stats(HomeStats::default());
        assert_eq!(state.copied_event(5_001), None, "a refresh clears it");
    }
}
