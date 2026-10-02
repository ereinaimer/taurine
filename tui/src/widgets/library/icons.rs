// Nerd Font glyphs (single-cell, no emoji). Requires a Nerd Font in the
// terminal; layout math stays char-count based.
pub(crate) const WINDOWS_ICON: &str = "\u{f17a}";
pub(crate) const MACOS_ICON: &str = "\u{f179}";
pub(crate) const LINUX_ICON: &str = "\u{f17c}";
pub(crate) const ANDROID_ICON: &str = "\u{f17b}";
pub(crate) const IOS_ICON: &str = "\u{f179}";
pub(crate) const ALL_ICON: &str = "\u{f0ac}";
pub(crate) const CHEVRON_DOWN: &str = "\u{f0d7}";
pub(crate) const ADD_ICON: &str = "\u{ea60}";
pub(crate) const INFO_ICON: &str = "\u{f129}";

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
