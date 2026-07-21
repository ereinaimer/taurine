#![allow(dead_code)]

use ratatui::style::Color;

#[derive(Debug, Clone, PartialEq)]
pub struct HeaderTheme {
    pub bg: Color,
    pub text: Color,
    pub daemon_running: Color,
    pub daemon_stopped: Color,
    pub daemon_paused: Color,
    pub daemon_starting: Color,
    pub daemon_stopping: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonTheme {
    pub active_bg: Color,
    pub inactive_bg: Color,
    pub danger_bg: Color,
    pub text: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModalTheme {
    pub overlay_bg: Color,
    pub border: Color,
    pub title_fg: Color,
    pub surface_bg: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionTheme {
    pub bg: Color,
    pub text: Color,
    pub border: Color,
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
    pub modal: ModalTheme,
    pub selection: SelectionTheme,
}

pub mod builtin;
