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
    // honey: GitHub-dark syntax scale; pastel by design, the saturated
    // UI palette stays reserved for states and chrome.
    syntax_comment: Color::Rgb(0x8B, 0x94, 0x9E),
    syntax_keyword: Color::Rgb(0xFF, 0x7B, 0x72),
    syntax_string: Color::Rgb(0xA5, 0xD6, 0xFF),
    syntax_entity: Color::Rgb(0xD2, 0xA8, 0xFF),
    syntax_constant: Color::Rgb(0x79, 0xC0, 0xFF),
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
