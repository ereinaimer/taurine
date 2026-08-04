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
    pub dark: bool,
    // Core semantic palette
    pub primary: Color,
    pub secondary: Color,
    pub border: Color,
    pub background: Color,
    pub surface: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub error: Color,
    pub warning: Color,
    pub success: Color,
    // Component sub-themes
    pub header: HeaderTheme,
    pub button: ButtonTheme,
}

pub mod builtin;
