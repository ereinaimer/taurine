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
}

pub mod builtin;
