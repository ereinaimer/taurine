//! Full-screen launch and exit transitions built on `tachyonfx`.

use ratatui::style::Style;
use tachyonfx::{
    Effect, EffectTimer, Interpolation,
    fx::{self, EvolveSymbolSet},
    pattern::SweepPattern,
};

use crate::theme::Theme;

/// 60 FPS polling while a screen transition is active.
pub(crate) const SCREEN_FX_FRAME: std::time::Duration = std::time::Duration::from_millis(16);

pub(crate) fn launch_effect(theme: &Theme) -> Effect {
    let style = Style::default().fg(theme.surface).bg(theme.background);
    let timer = EffectTimer::from_ms(280, Interpolation::CubicOut);

    fx::evolve_into((EvolveSymbolSet::Shaded, style), timer)
        .with_pattern(SweepPattern::left_to_right(35))
}

pub(crate) fn close_effect(theme: &Theme) -> Effect {
    let style = Style::default().fg(theme.surface).bg(theme.background);
    let timer = EffectTimer::from_ms(200, Interpolation::CubicIn);

    fx::evolve_from((EvolveSymbolSet::Shaded, style), timer)
        .with_pattern(SweepPattern::left_to_right(35))
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect};
    use tachyonfx::Duration;

    use crate::theme::default::DARK_THEME;

    use super::{close_effect, launch_effect};

    fn buffer() -> (Buffer, Rect) {
        let area = Rect::new(0, 0, 20, 5);
        (Buffer::empty(area), area)
    }

    #[test]
    fn launch_effect_runs_then_completes() {
        let mut effect = launch_effect(&DARK_THEME);
        assert!(effect.running());

        let (mut buffer, area) = buffer();
        effect.process(Duration::from_millis(320), &mut buffer, area);

        assert!(!effect.running());
    }

    #[test]
    fn close_effect_runs_then_completes() {
        let mut effect = close_effect(&DARK_THEME);
        assert!(effect.running());

        let (mut buffer, area) = buffer();
        effect.process(Duration::from_millis(240), &mut buffer, area);

        assert!(!effect.running());
    }

    #[test]
    fn close_effect_sweeps_left_to_right() {
        let mut effect = close_effect(&DARK_THEME);
        let area = Rect::new(0, 0, 40, 5);
        let mut buffer = Buffer::empty(area);
        for cell in buffer.content.iter_mut() {
            cell.set_char('x');
        }

        // Midway through, the left edge has started dissolving while the
        // right edge is still untouched.
        effect.process(Duration::from_millis(100), &mut buffer, area);
        assert_ne!(buffer.cell((0, 2)).expect("left cell").symbol(), "x");
        assert_eq!(buffer.cell((39, 2)).expect("right cell").symbol(), "x");
    }
}
