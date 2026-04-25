use taurine_core::db::crud::TriggerType;
use taurine_core::engine::variables::{
    ValidationError, split_system_tag, valid_modifier_hint, validate_system_tag,
};
use taurine_core::keys::{
    HotkeyPlatform, conflicts_with_taurine_global_hotkey, danger_for_platform, parse_hotkey,
};

const TAG_OPEN: u8 = b'[';
const TAG_CLOSE: u8 = b']';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TagBounds {
    start: usize,
    end: usize,
}

pub fn audit_payload_tags(payload: &str) -> taurine_core::error::Result<()> {
    let mut ptr = 0;

    while let Some(tag) = find_next_tag(payload, ptr) {
        let inner = trim_slice(&payload[tag.start + 1..tag.end]);
        let (key, default_value) = split_key_default(inner);

        if let Some((root, modifier)) = split_system_tag(key) {
            if let Some(_default) = default_value {
                return Err(taurine_core::error::Error::Config(format!(
                    "Invalid system tag [{}]: system tags cannot use default assignments. {}",
                    inner,
                    valid_modifier_hint(root)
                )));
            }

            if let Err(error) = validate_system_tag(root, modifier) {
                return Err(taurine_core::error::Error::Config(format_validation_error(
                    inner, root, modifier, &error,
                )));
            }
        }

        ptr = tag.end + 1;
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTrigger {
    pub trigger_type: TriggerType,
    pub stored_trigger: String,
}

pub fn prepare_trigger(
    trigger: &str,
    use_hotkey: bool,
    target_os: &str,
) -> taurine_core::Result<PreparedTrigger> {
    if !use_hotkey {
        return Ok(PreparedTrigger {
            trigger_type: TriggerType::Word,
            stored_trigger: trigger.to_string(),
        });
    }

    let hotkey = parse_hotkey(trigger).map_err(|error| {
        taurine_core::error::Error::Config(format!("Invalid hotkey '{}': {}", trigger, error))
    })?;
    let canonical = hotkey.canonical_string();

    if conflicts_with_taurine_global_hotkey(hotkey).is_some() {
        return Err(taurine_core::error::Error::Config(format!(
            "Hotkey '{}' conflicts with Taurine's global pause hotkey alt+`",
            canonical
        )));
    }

    for platform in desktop_platforms_for_target_os(target_os)? {
        if let Some(danger) = danger_for_platform(hotkey, *platform) {
            return Err(taurine_core::error::Error::Config(format!(
                "Hotkey '{}' is not allowed for target_os '{}': conflicts with the {} on {}",
                canonical,
                target_os,
                danger.description(),
                platform.as_label(),
            )));
        }
    }

    Ok(PreparedTrigger {
        trigger_type: TriggerType::Hotkey,
        stored_trigger: canonical,
    })
}

fn desktop_platforms_for_target_os(
    target_os: &str,
) -> taurine_core::Result<&'static [HotkeyPlatform]> {
    match target_os {
        "all" => Ok(&[
            HotkeyPlatform::Windows,
            HotkeyPlatform::Linux,
            HotkeyPlatform::Mac,
        ]),
        "win" => Ok(&[HotkeyPlatform::Windows]),
        "linux" => Ok(&[HotkeyPlatform::Linux]),
        "mac" => Ok(&[HotkeyPlatform::Mac]),
        "android" | "ios" => Err(taurine_core::error::Error::Config(format!(
            "Hotkey triggers are only supported for desktop target_os values; got '{}'",
            target_os
        ))),
        other => Err(taurine_core::error::Error::Config(format!(
            "Unsupported target_os '{}' for hotkey validation",
            other
        ))),
    }
}

trait PlatformLabel {
    fn as_label(&self) -> &'static str;
}

impl PlatformLabel for HotkeyPlatform {
    fn as_label(&self) -> &'static str {
        match self {
            HotkeyPlatform::Windows => "windows",
            HotkeyPlatform::Linux => "linux",
            HotkeyPlatform::Mac => "mac",
        }
    }
}

fn format_validation_error(
    raw_tag: &str,
    root: &str,
    modifier: Option<&str>,
    error: &ValidationError,
) -> String {
    match error {
        ValidationError::MissingModifier { .. } => format!(
            "Invalid system tag [{}]: `{}` requires a modifier. {}",
            raw_tag,
            root,
            valid_modifier_hint(root)
        ),
        ValidationError::UnexpectedModifier { .. } => format!(
            "Invalid system tag [{}]: `{}` does not accept modifier `{}`. {}",
            raw_tag,
            root,
            modifier.unwrap_or_default(),
            valid_modifier_hint(root)
        ),
        ValidationError::InvalidModifier { modifier, .. } => format!(
            "Invalid system tag [{}]: modifier `{}` is not valid for `{}`. {}",
            raw_tag,
            modifier,
            root,
            valid_modifier_hint(root)
        ),
        ValidationError::UnknownRoot(root) => {
            format!("Invalid system tag [{}]: unknown root `{}`.", raw_tag, root)
        }
    }
}

fn is_escaped(bytes: &[u8], idx: usize) -> bool {
    let mut backslashes = 0;
    let mut cursor = idx;

    while cursor > 0 && bytes[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }

    backslashes % 2 == 1
}

fn trim_slice(s: &str) -> &str {
    let trimmed = s.trim();
    let start = s.len() - s.trim_start().len();
    &s[start..start + trimmed.len()]
}

fn find_next_tag(text: &str, from: usize) -> Option<TagBounds> {
    let bytes = text.as_bytes();
    let mut ptr = from;
    let mut start = None;
    let mut depth = 0usize;

    while ptr < bytes.len() {
        match bytes[ptr] {
            TAG_OPEN if !is_escaped(bytes, ptr) => {
                if depth == 0 {
                    start = Some(ptr);
                }
                depth += 1;
            }
            TAG_CLOSE if !is_escaped(bytes, ptr) && depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    return start.map(|tag_start| TagBounds {
                        start: tag_start,
                        end: ptr,
                    });
                }
            }
            _ => {}
        }
        ptr += 1;
    }

    None
}

fn split_key_default(inner: &str) -> (&str, Option<&str>) {
    let inner = trim_slice(inner);
    let bytes = inner.as_bytes();
    let mut depth = 0usize;
    let mut ptr = 0;

    while ptr < bytes.len() {
        if bytes[ptr] == TAG_OPEN && !is_escaped(bytes, ptr) {
            depth += 1;
        } else if bytes[ptr] == TAG_CLOSE && !is_escaped(bytes, ptr) {
            depth -= 1;
        } else if bytes[ptr] == b'=' && depth == 0 {
            return (
                trim_slice(&inner[..ptr]),
                Some(trim_slice(&inner[ptr + 1..])),
            );
        }
        ptr += 1;
    }

    (inner, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_system_tags_and_literals() {
        assert!(audit_payload_tags("[time.now.upper] [env.USERPROFILE]").is_ok());
        assert!(audit_payload_tags("[net.hostname] [net.localip] [net.mac]").is_ok());
        assert!(audit_payload_tags("json = [1, 2, 3]").is_ok());
        assert!(audit_payload_tags("[name.upper]").is_ok());
    }

    #[test]
    fn rejects_invalid_system_modifier() {
        let error = audit_payload_tags("[time.india]").unwrap_err();
        assert!(error.to_string().contains("time.india"));
        assert!(error.to_string().contains("Valid modifiers"));
    }

    #[test]
    fn rejects_unknown_net_modifier() {
        let error = audit_payload_tags("[net.publicip]").unwrap_err();
        assert!(error.to_string().contains("net.publicip"));
        assert!(error.to_string().contains("hostname"));
    }

    #[test]
    fn rejects_system_default_assignment() {
        let error = audit_payload_tags("[cursor=here]").unwrap_err();
        assert!(error.to_string().contains("cannot use default assignments"));
    }

    #[test]
    fn rejects_missing_env_key() {
        let error = audit_payload_tags("[env]").unwrap_err();
        assert!(error.to_string().contains("requires a modifier"));
    }

    #[test]
    fn accepts_lorem_with_nested_dynamic_arg() {
        assert!(audit_payload_tags("[lorem.words([num=5])]").is_ok());
        assert!(audit_payload_tags("[lorem.words([random.int(3, 3)])]").is_ok());
    }

    #[test]
    fn accepts_mock_with_nested_dynamic_arg() {
        assert!(audit_payload_tags("[mock.password([len=12])]").is_ok());
    }

    #[test]
    fn prepare_trigger_defaults_to_word_when_hotkey_flag_is_absent() {
        let prepared = prepare_trigger("gs", false, "all").unwrap();
        assert_eq!(prepared.trigger_type, TriggerType::Word);
        assert_eq!(prepared.stored_trigger, "gs");
    }

    #[test]
    fn prepare_trigger_canonicalizes_hotkeys() {
        let prepared = prepare_trigger("Shift + Ctrl + G", true, "win").unwrap();
        assert_eq!(prepared.trigger_type, TriggerType::Hotkey);
        assert_eq!(prepared.stored_trigger, "ctrl+shift+g");
    }

    #[test]
    fn prepare_trigger_canonicalizes_side_specific_hotkeys() {
        let prepared = prepare_trigger("leftcontrol+altgr+k", true, "win").unwrap();
        assert_eq!(prepared.trigger_type, TriggerType::Hotkey);
        assert_eq!(prepared.stored_trigger, "lctrl+ralt+k");
    }

    #[test]
    fn prepare_trigger_rejects_malformed_hotkeys() {
        let error = prepare_trigger("ctrl+k+p", true, "linux").unwrap_err();
        assert!(error.to_string().contains("multiple base keys"));

        let error = prepare_trigger("ctrl+shift", true, "linux").unwrap_err();
        assert!(
            error.to_string().contains("missing a base key")
                || error.to_string().contains("exactly one base key")
                || error.to_string().contains("modifier")
        );
    }

    #[test]
    fn prepare_trigger_rejects_dangerous_hotkeys_for_target_os() {
        let error = prepare_trigger("ctrl+c", true, "win").unwrap_err();
        assert!(error.to_string().contains("copy shortcut"));
        assert!(error.to_string().contains("windows"));
    }

    #[test]
    fn prepare_trigger_rejects_side_specific_variants_of_dangerous_hotkeys() {
        let error = prepare_trigger("lctrl+c", true, "linux").unwrap_err();
        assert!(error.to_string().contains("copy shortcut"));

        let error = prepare_trigger("ralt+tab", true, "linux").unwrap_err();
        assert!(error.to_string().contains("application switcher"));
    }

    #[test]
    fn prepare_trigger_treats_all_as_all_desktop_platforms() {
        let error = prepare_trigger("meta+q", true, "all").unwrap_err();
        assert!(error.to_string().contains("quit-application shortcut"));
        assert!(error.to_string().contains("mac"));
    }

    #[test]
    fn prepare_trigger_rejects_taurine_pause_hotkey_only() {
        let error = prepare_trigger("alt+`", true, "all").unwrap_err();
        assert!(error.to_string().contains("global pause hotkey"));

        let error = prepare_trigger("lalt+`", true, "all").unwrap_err();
        assert!(error.to_string().contains("global pause hotkey"));

        let error = prepare_trigger("ralt+`", true, "all").unwrap_err();
        assert!(error.to_string().contains("global pause hotkey"));

        assert!(prepare_trigger("alt+enter", true, "all").is_ok());
        assert!(prepare_trigger("alt+esc", true, "all").is_ok());
    }

    #[test]
    fn prepare_trigger_rejects_mobile_hotkey_targets() {
        let error = prepare_trigger("ctrl+shift+g", true, "android").unwrap_err();
        assert!(error.to_string().contains("desktop target_os"));

        let error = prepare_trigger("ctrl+shift+g", true, "ios").unwrap_err();
        assert!(error.to_string().contains("desktop target_os"));
    }
}
