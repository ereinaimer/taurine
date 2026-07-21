use ratatui::style::Color;

pub struct Theme {
    #[allow(dead_code)]
    pub name: &'static str,
    pub bg: Color,
    pub text: Color,
    pub muted: Color,
    pub highlight: Color,
    pub border: Color,
    pub error: Color,
    pub button_active: Color,
}

pub const DARK_THEME: Theme = Theme {
    name: "dark",
    bg: Color::Rgb(0x0A, 0x0A, 0x0A),
    text: Color::Rgb(0xE5, 0xE5, 0xE5),
    muted: Color::Rgb(0x80, 0x80, 0x80),
    highlight: Color::Rgb(0x1E, 0x1E, 0x1E),
    border: Color::Rgb(0x1E, 0x1E, 0x1E),
    error: Color::Red,
    button_active: Color::Rgb(0x3A, 0x3A, 0x3A),
};
