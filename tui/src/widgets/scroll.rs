//! Shared eased scrolling for page content.

/// Lines advanced per wheel notch before easing catches up.
pub(crate) const STEP_LINES: usize = 3;
/// Share of the remaining distance covered each animation frame.
const EASE: f32 = 0.3;

/// A scroll offset that eases toward a wheel target over several frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SmoothScroll {
    scroll: usize,
    target: usize,
    animating: bool,
    max: usize,
}

impl SmoothScroll {
    pub(crate) const fn scroll(&self) -> usize {
        self.scroll
    }

    pub(crate) const fn animating(&self) -> bool {
        self.animating
    }

    /// Viewport bound, refreshed from the last render.
    pub(crate) fn set_max(&mut self, max: usize) {
        self.max = max;
        self.target = self.target.min(max);
        self.scroll = self.scroll.min(max);
    }

    /// Set the offset; an idle scroll stays in step with its target.
    pub(crate) fn set(&mut self, scroll: usize) {
        self.scroll = scroll;
        if !self.animating {
            self.target = scroll;
        }
    }

    /// Snap to an offset and cancel any in-flight glide.
    pub(crate) fn jump(&mut self, scroll: usize) {
        self.scroll = scroll;
        self.target = scroll;
        self.animating = false;
    }

    /// Stop where we are, discarding the pending target.
    pub(crate) fn stop(&mut self) {
        self.jump(self.scroll);
    }

    /// Queue wheel notches; the next frames ease toward the target.
    pub(crate) fn queue(&mut self, notches: isize) {
        let step = STEP_LINES as isize * notches;
        let target = if step >= 0 {
            self.target.saturating_add(step as usize)
        } else {
            self.target.saturating_sub(step.unsigned_abs())
        };
        self.target = target.min(self.max);
        self.animating = self.target != self.scroll;
    }

    /// One animation frame: move a fraction of the remaining distance.
    pub(crate) fn advance(&mut self) -> usize {
        if !self.animating {
            return self.scroll;
        }
        let target = self.target.min(self.max);
        if self.scroll == target {
            self.animating = false;
            return self.scroll;
        }
        let diff = target as isize - self.scroll as isize;
        let step = ((diff as f32) * EASE).round() as isize;
        let step = if step == 0 { diff.signum() } else { step };
        let next = (self.scroll as isize + step).clamp(0, self.max as isize) as usize;
        self.scroll = next;
        if next == target {
            self.animating = false;
        }
        next
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_eases_forward_in_small_steps() {
        let mut scroll = SmoothScroll::default();
        scroll.set_max(100);
        scroll.queue(1);
        assert!(scroll.animating());

        let mut last = scroll.scroll();
        for _ in 0..10 {
            let next = scroll.advance();
            assert!(next >= last, "moves forward");
            assert!(next - last <= STEP_LINES, "one small step");
            last = next;
            if !scroll.animating() {
                break;
            }
        }
        assert_eq!(scroll.scroll(), STEP_LINES);
    }

    #[test]
    fn max_clamps_both_ends() {
        let mut scroll = SmoothScroll::default();
        scroll.set_max(2);
        scroll.queue(1);
        while scroll.animating() {
            scroll.advance();
        }
        assert_eq!(scroll.scroll(), 2);

        scroll.jump(2);
        scroll.queue(-1);
        while scroll.animating() {
            scroll.advance();
        }
        assert_eq!(scroll.scroll(), 0);
    }

    #[test]
    fn jump_cancels_the_glide() {
        let mut scroll = SmoothScroll::default();
        scroll.set_max(100);
        scroll.queue(1);
        scroll.jump(7);
        assert!(!scroll.animating());
        assert_eq!(scroll.scroll(), 7);
        assert_eq!(scroll.advance(), 7);
    }
}
