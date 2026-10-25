// Nerd Font glyphs (single-cell, no emoji). Requires a Nerd Font in the
// terminal; layout math stays char-count based.
pub(crate) const WINDOWS_ICON: &str = "\u{f17a}";
pub(crate) const MACOS_ICON: &str = "\u{f179}";
pub(crate) const LINUX_ICON: &str = "\u{f17c}";
pub(crate) const ANDROID_ICON: &str = "\u{f17b}";
pub(crate) const IOS_ICON: &str = "\u{f179}";
pub(crate) const ALL_ICON: &str = "\u{f0ac}";
pub(crate) const CHEVRON_DOWN: &str = "\u{f0d7}";
pub(crate) const CHEVRON_UP: &str = "\u{f0d8}";
/// Close X: single-cell MDI glyph marking a focused removable row.
pub(crate) const CLOSE_ICON: &str = "\u{f00d}";
/// Add: single-cell Nerd Font plus used by add buttons.
pub(crate) const ADD_ICON: &str = "\u{ea60}";
/// Padded three-cell add chip for inline property slots.
pub(crate) const ADD_CHIP: &str = " \u{ea60} ";
/// Collapse-left arrow for the command palette's left panel toggle.
pub(crate) const PANEL_LEFT_ICON: &str = "\u{f104}";
/// Collapse-right arrow for the command palette's right panel toggle.
pub(crate) const PANEL_RIGHT_ICON: &str = "\u{f105}";
/// Restore arrow for resetting the pane layout.
pub(crate) const RESET_LAYOUT_ICON: &str = "\u{f01e}";
/// Download arrow for importing triggers.
pub(crate) const IMPORT_ICON: &str = "\u{f019}";
/// Upload arrow for exporting triggers.
pub(crate) const EXPORT_ICON: &str = "\u{f093}";
/// Cog for the settings overlay command.
pub(crate) const SETTINGS_ICON: &str = "\u{f013}";
/// Refresh arrow for restarting Taurine.
pub(crate) const RESTART_ICON: &str = "\u{f021}";
/// Power glyph for quitting the TUI.
pub(crate) const QUIT_ICON: &str = "\u{f011}";

pub(crate) fn os_icon(target_os_display: &str) -> &'static str {
    match taurine_core::db::TargetOs::parse_str(target_os_display) {
        Some(taurine_core::db::TargetOs::Windows) => WINDOWS_ICON,
        Some(taurine_core::db::TargetOs::MacOs) => MACOS_ICON,
        Some(taurine_core::db::TargetOs::Linux) => LINUX_ICON,
        Some(taurine_core::db::TargetOs::Android) => ANDROID_ICON,
        Some(taurine_core::db::TargetOs::Ios) => IOS_ICON,
        _ => ALL_ICON,
    }
}
