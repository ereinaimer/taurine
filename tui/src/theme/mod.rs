use ratatui::style::Color;

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderTheme {
    pub bg: Color,
    pub text: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonTheme {
    pub active_bg: Color,
    pub inactive_bg: Color,
    pub text: Color,
}

/// Keycap pills for hotkey chords: flat fill, no border. The plain
/// fill sits on the pane background; `on_surface` is a shade lighter
/// so pills stay visible on a highlighted row.
#[derive(Debug, Clone, PartialEq)]
pub struct KeycapTheme {
    pub bg: Color,
    pub on_surface: Color,
}

/// Flat activity cards on Home: an idle fill and a lighter selected or
/// hovered fill. Both are squares of background color, never borders.
#[derive(Debug, Clone, PartialEq)]
pub struct CardTheme {
    pub bg: Color,
    pub active_bg: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: &'static str,
    // Core semantic palette
    pub primary: Color,
    pub secondary: Color,
    pub border: Color,
    pub background: Color,
    pub surface: Color,
    pub text: Color,
    pub text_muted: Color,
    pub description: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    /// Muted green fill for success states that sit behind text.
    pub success_bg: Color,
    /// Muted fill for progress tracks and empty gauges.
    pub track: Color,
    // Syntax scale: Atom One Dark roles shared by script code and
    // Taurine's own `[vars]` / `| transformers` markup. Commands read
    // blue, quoted text green, keywords purple, numbers orange.
    pub syntax_comment: Color,
    pub syntax_keyword: Color,
    pub syntax_string: Color,
    pub syntax_entity: Color,
    pub syntax_constant: Color,
    pub syntax_type: Color,
    pub syntax_variable: Color,
    // Component sub-themes
    pub header: HeaderTheme,
    pub button: ButtonTheme,
    pub keycap: KeycapTheme,
    pub card: CardTheme,
}

pub mod default;
