use crate::theme::{ButtonTheme, HeaderTheme, Theme};
use ratatui::style::Color;

pub const DARK_THEME: Theme = Theme {
    name: "dark",
    primary: Color::Rgb(0x1E, 0x90, 0xFF),
    secondary: Color::Rgb(0x80, 0x80, 0x80),
    border: Color::Rgb(0x1A, 0x1A, 0x1A),
    background: Color::Rgb(0x0A, 0x0A, 0x0A),
    surface: Color::Rgb(0x1E, 0x1E, 0x1E),
    // honey: off-white body text; pure white on near-black blows out,
    // especially under bold, so the dark theme never uses it.
    text: Color::Rgb(0xCF, 0xCF, 0xCF),
    text_muted: Color::Rgb(0x80, 0x80, 0x80),
    description: Color::Rgb(0x73, 0x73, 0x73),
    accent: Color::Rgb(0x1E, 0x90, 0xFF),
    error: Color::Red,
    warning: Color::Yellow,
    success: Color::Green,
    // honey: Atom One Dark token roles; template punctuation reuses the
    // muted UI grey instead of growing another field.
    syntax_comment: Color::Rgb(0x5C, 0x63, 0x70),
    syntax_keyword: Color::Rgb(0xC6, 0x78, 0xDD),
    syntax_string: Color::Rgb(0x98, 0xC3, 0x79),
    syntax_entity: Color::Rgb(0x61, 0xAF, 0xEF),
    syntax_constant: Color::Rgb(0x7F, 0xD8, 0x8F),
    syntax_type: Color::Rgb(0xE5, 0xC0, 0x7B),
    syntax_variable: Color::Rgb(0xE0, 0x6C, 0x75),
    header: HeaderTheme {
        bg: Color::Rgb(0x1E, 0x1E, 0x1E),
        text: Color::Rgb(0xCF, 0xCF, 0xCF),
    },
    button: ButtonTheme {
        active_bg: Color::Rgb(0x3A, 0x3A, 0x3A),
        inactive_bg: Color::Rgb(0x1E, 0x1E, 0x1E),
        text: Color::Rgb(0xCF, 0xCF, 0xCF),
    },
};
