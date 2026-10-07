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
    // Template palette: desaturated GitHub-dark tones for Taurine's own
    // `[vars]` and `| transformers` markup, so template chrome never
    // shouts over script code.
    pub template_bracket: Color,
    pub template_name: Color,
    pub template_transformer: Color,
    pub template_value: Color,
    // Component sub-themes
    pub header: HeaderTheme,
    pub button: ButtonTheme,
}

pub mod builtin;
