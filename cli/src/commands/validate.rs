#[cfg(test)]
use taurine_core::db::crud::TriggerType;
pub use taurine_core::db::crud::{PreparedTrigger, audit_payload_tags, prepare_trigger};

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
