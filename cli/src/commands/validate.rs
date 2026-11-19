#[cfg(test)]
use taurine_core::db::crud::TriggerType;
pub use taurine_core::db::crud::{
    PreparedTrigger, audit_payload_tags, audit_payload_tags_with_trigger_type, prepare_trigger,
};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

pub fn format_trigger_log(
    action: &str,
    trigger: &str,
    script_info: Option<(ScriptBehavior, ScriptInterpreter)>,
    os: &str,
    include_apps: Option<&str>,
    exclude_apps: Option<&str>,
) -> String {
    let mut parts = Vec::new();

    // Base action & type
    if let Some((behavior, interpreter)) = script_info {
        let behavior_str = match behavior {
            ScriptBehavior::Inline => "inline",
            ScriptBehavior::Silent => "silent",
        };
        let lang_str = match interpreter {
            ScriptInterpreter::Bash => "bash",
            ScriptInterpreter::PowerShell => "powershell",
            ScriptInterpreter::Python => "python",
            ScriptInterpreter::Node => "node",
            ScriptInterpreter::Cmd => "cmd",
        };
        parts.push(format!(
            "{} {} script trigger using {} for '{}'",
            action, behavior_str, lang_str, trigger
        ));
    } else {
        parts.push(format!("{} trigger for '{}'", action, trigger));
    }

    // Target OS filter
    if os != "all" {
        let os_name = match os {
            "win" => "Windows",
            "mac" => "macOS",
            "linux" => "Linux",
            "android" => "Android",
            "ios" => "iOS",
            other => other,
        };
        parts.push(format!("on {}", os_name));
    }

    // App filters
    match (include_apps, exclude_apps) {
        (Some(inc), Some(exc)) if !inc.is_empty() && !exc.is_empty() => {
            parts.push(format!(
                "when active in '{}' and except when active in '{}'",
                inc, exc
            ));
        }
        (Some(inc), _) if !inc.is_empty() => {
            parts.push(format!("when active in '{}'", inc));
        }
        (_, Some(exc)) if !exc.is_empty() => {
            parts.push(format!("except when active in '{}'", exc));
        }
        _ => {}
    }

    // Join sections and terminate with a period
    format!("{}.", parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_system_tags_and_literals() {
        assert!(audit_payload_tags("[time utc | case(upper)] [env USERPROFILE]").is_ok());
        assert!(audit_payload_tags("[ip]").is_ok());
        assert!(audit_payload_tags("[ip public]").is_err());
        assert!(audit_payload_tags("[ip online]").is_err());
        assert!(audit_payload_tags("json = \\[1, 2, 3\\]").is_ok());
        assert!(audit_payload_tags("[$name=\"John\" | case(upper)]").is_ok());
        assert!(audit_payload_tags("[clip | ai(\"summarize\")]").is_ok());
        assert!(audit_payload_tags(
            "[run \"curl https://ipinfo.io/json\" | json(\"ip\") | case(upper) | strip(whitespace)]"
        ).is_ok());
    }

    #[test]
    fn rejects_invalid_system_modifier() {
        let error = audit_payload_tags("[date invalid_param=123]").unwrap_err();
        assert!(error.to_string().contains("date"));
    }

    #[test]
    fn rejects_unknown_transformer_names() {
        let error = audit_payload_tags("[$name=\"John\" | bogus_transform]").unwrap_err();
        assert!(error.to_string().contains("unknown transformer"));
        assert!(error.to_string().contains("bogus_transform"));

        let legacy = audit_payload_tags("[clip | firstline]").unwrap_err();
        assert!(legacy.to_string().contains("unknown transformer"));

        assert!(audit_payload_tags("[$name=\"John\" | case(upper)]").is_ok());
        assert!(audit_payload_tags("[$name=\"John\" | upper]").is_ok());
        assert!(audit_payload_tags("[clip | lines(first)]").is_ok());
        assert!(audit_payload_tags("[clip | lines(sort, \"numeric\")]").is_ok());
    }

    #[test]
    fn rejects_unknown_net_modifier() {
        let error = audit_payload_tags("[ip bogus]").unwrap_err();
        assert!(error.to_string().contains("ip"));
    }

    #[test]
    fn rejects_system_default_assignment() {
        let error = audit_payload_tags("[cursor=here]").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("system tags cannot have defaults")
        );
    }

    #[test]
    fn rejects_missing_env_key() {
        let error = audit_payload_tags("[env]").unwrap_err();
        assert!(
            error.to_string().contains("needs a modifier") || error.to_string().contains("env")
        );
    }

    #[test]
    fn accepts_lorem_with_nested_dynamic_arg() {
        assert!(audit_payload_tags("[lorem words [$num=\"5\"]]").is_ok());
        assert!(audit_payload_tags("[lorem words [random 3 3]]").is_ok());
    }

    #[test]
    fn rejects_missing_default_assignment() {
        let error = audit_payload_tags("[name | case(upper)]").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("dynamic variables need a default")
        );

        let error2 = audit_payload_tags("json = [1, 2, 3]").unwrap_err();
        assert!(
            error2
                .to_string()
                .contains("dynamic variables need a default")
        );
    }

    #[test]
    fn rejects_empty_default_assignment() {
        let error = audit_payload_tags("[$name=]").unwrap_err();
        assert!(error.to_string().contains("default value cannot be empty"));

        let error3 = audit_payload_tags("[$name=   | case(upper)]").unwrap_err();
        assert!(error3.to_string().contains("default value cannot be empty"));

        // Explicit empty quotes are allowed as empty string defaults
        assert!(audit_payload_tags("[$name=\"\"]").is_ok());
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
        assert!(error.to_string().to_lowercase().contains("windows"));
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

    #[test]
    fn prepare_trigger_allows_word_triggers_with_spaces() {
        assert!(prepare_trigger("hello world", false, "all").is_ok());
    }

    #[test]
    fn prepare_trigger_rejects_word_triggers_with_newlines() {
        let error_newline = prepare_trigger("hello\nworld", false, "all").unwrap_err();
        assert!(
            error_newline
                .to_string()
                .contains("cannot contain newlines")
        );

        let error_cr = prepare_trigger("hello\rworld", false, "all").unwrap_err();
        assert!(error_cr.to_string().contains("cannot contain newlines"));
    }

    #[test]
    fn accepts_nested_and_reused_variables() {
        assert!(audit_payload_tags("Status: [run \"curl -s -o /dev/null -w '%{http_code}' https://httpbin.org/status/200\"] | UA: [[run \"curl -s https://httpbin.org/headers\"] | json('headers.User-Agent') | truncate(15)]").is_ok());
        assert!(audit_payload_tags("User [$name='Developer'] checked [$url='httpbin.org/json'] at [time utc HH:mm] UTC. Title of JSON: [run \"curl -s https://httpbin.org/json\" | json('slideshow.title') | case(upper)]").is_ok());
    }

    #[test]
    fn test_template_syntax_spec_compliance_validation() {
        // Positive Test Cases
        assert!(audit_payload_tags("Hello [$name='friend']! You live in [$city='San Francisco'] and work as [$role='Software Engineer'].").is_ok());
        assert!(
            audit_payload_tags_with_trigger_type(
                "Hello [$1]! You live in [$2] and [$3]",
                TriggerType::Regex
            )
            .is_ok()
        );
        assert!(audit_payload_tags("Escaped brackets: \\[$arg=\"ignored\"\\] | Literal pipe: [$arg='default value' \\| case(upper)] | Parsed pipe: [$arg='hello' | case(upper)]").is_ok());
        assert!(audit_payload_tags("Local: [date] [time] | UTC +1w: [date +1w utc \"'Today is' dddd, MMMM D, YYYY\"] | UTC Time -2h: [time -2h hh:mm A utc] | Cased AM/PM: [time A | case(lower)]").is_ok());
        assert!(audit_payload_tags("User (Title Case): [env USERNAME | case(title)] | Home Path (Lowercase): [env USERPROFILE | case(lower)]").is_ok());
        assert!(audit_payload_tags("Full Content: [file read \"~/taurine_test.txt\" | strip(whitespace)] | Line 2: [file line \"~/taurine_test.txt\" 2 | case(upper)] | Lines 1-3: [file lines \"~/taurine_test.txt\" 1 3]").is_ok());
        assert!(audit_payload_tags("Latest (Slugified): [clip | case(slug)] | Second: [clip | strip(whitespace)] | Third (Upper): [clip 1 | case(upper)] | Empty index: [clip 2 | wrap(singlequote)]").is_ok());
        assert!(audit_payload_tags("Cwd Path: [run \"(Get-Location).Path\" | strip(whitespace)] | Cmd Command: [run \"echo hello from cmd\" | case(upper)] | Silent Task: [run silent \"echo 'background task'\"]").is_ok());
        assert!(audit_payload_tags("Status: [run \"curl -s https://httpbin.org/status/200\"] | UA: [run \"curl -s https://httpbin.org/headers\" | json('headers.User-Agent') | truncate(15)]").is_ok());
        assert!(audit_payload_tags("Status: [run \"curl -s https://httpbin.org/status/200\"] | UA: [[run \"curl -s https://httpbin.org/headers\"] | json('headers.User-Agent') | truncate(15)]").is_ok());
        assert!(audit_payload_tags("Int (10-50): [random int 10 50] | Pass (12): [random pass 12] | Choice: [random choice apple banana cherry | case(title)] | Lorem (Dynamic Count): [lorem words [random 2 4] | case(kebab)]").is_ok());
        assert!(
            audit_payload_tags("Output: [use \"testinner\" | case(upper)] | Date: [date]").is_ok()
        );
        assert!(audit_payload_tags("User [$name='Developer'] checked [$url='httpbin.org/json'] at [time utc HH:mm] UTC. Title of JSON: [run \"curl -s https://httpbin.org/json\" | json('slideshow.title') | case(upper)]").is_ok());
        assert!(audit_payload_tags("[$first=\"first\"][tab][wait 100ms][$second=\"second\"][tab][wait 50ms][$third=\"third\"][enter]").is_ok());

        // Negative Test Cases
        assert!(audit_payload_tags("User: [name=]").is_err());
        assert!(audit_payload_tags("User: [name]").is_err());
        assert!(audit_payload_tags("Hello [0]!").is_err());
        assert!(audit_payload_tags("User: [my.custom.var=there]").is_err());
        assert!(audit_payload_tags("Start: [cursor] End: [cursor]").is_err());
        assert!(audit_payload_tags("Hello: [cursor] [tab]").is_err());
        assert!(audit_payload_tags("Time: [time=12:00]").is_err());
    }

    #[test]
    fn test_format_trigger_log_sentences() {
        use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

        // Text trigger tests
        assert_eq!(
            format_trigger_log("Added", "ctrl+alt+p", None, "all", None, None),
            "Added trigger for 'ctrl+alt+p'."
        );
        assert_eq!(
            format_trigger_log("Updated", "ctrl+alt+p", None, "win", None, None),
            "Updated trigger for 'ctrl+alt+p' on Windows."
        );
        assert_eq!(
            format_trigger_log(
                "Added",
                "ctrl+alt+p",
                None,
                "mac",
                Some("exe:notepad"),
                None
            ),
            "Added trigger for 'ctrl+alt+p' on macOS when active in 'exe:notepad'."
        );
        assert_eq!(
            format_trigger_log("Added", "ctrl+alt+p", None, "linux", None, Some("exe:code")),
            "Added trigger for 'ctrl+alt+p' on Linux except when active in 'exe:code'."
        );
        assert_eq!(
            format_trigger_log(
                "Added",
                "ctrl+alt+p",
                None,
                "win",
                Some("exe:notepad"),
                Some("exe:code")
            ),
            "Added trigger for 'ctrl+alt+p' on Windows when active in 'exe:notepad' and except when active in 'exe:code'."
        );

        // Script trigger tests
        assert_eq!(
            format_trigger_log(
                "Added",
                "ctrl+alt+p",
                Some((ScriptBehavior::Inline, ScriptInterpreter::Bash)),
                "all",
                None,
                None
            ),
            "Added inline script trigger using bash for 'ctrl+alt+p'."
        );
        assert_eq!(
            format_trigger_log(
                "Updated",
                "ctrl+alt+p",
                Some((ScriptBehavior::Silent, ScriptInterpreter::PowerShell)),
                "win",
                Some("exe:notepad"),
                None
            ),
            "Updated silent script trigger using powershell for 'ctrl+alt+p' on Windows when active in 'exe:notepad'."
        );
    }

    #[test]
    fn test_script_payload_lax_validation() {
        use taurine_core::db::crud::audit_script_payload_tags;

        // Validating PowerShell script payload with type literals using audit_script_payload_tags should pass
        let powershell_script = r#"Add-Type -Assembly System.Windows.Forms; [System.Windows.Forms.Application]::SetSuspendState("Suspend", $false, $false)"#;
        assert!(audit_script_payload_tags(powershell_script, TriggerType::Word).is_ok());

        // Array brackets in scripts should also pass
        let array_script = r#"val = args[0]; echo $val"#;
        assert!(audit_script_payload_tags(array_script, TriggerType::Word).is_ok());

        // Traditional invalid dot-namespace variable still fails when treated as word payload
        assert!(super::audit_payload_tags("User: [my.custom.var=there]").is_err());

        // System variables and defined variables in scripts should still be checked
        assert!(audit_script_payload_tags("[date YYYY-MM-DD]", TriggerType::Word).is_ok());
        assert!(audit_script_payload_tags("[date 1d]", TriggerType::Word).is_err());
        assert!(audit_script_payload_tags("[chrono(YYYY-MM-DD)]", TriggerType::Word).is_err());
        assert!(audit_script_payload_tags("[my_var]", TriggerType::Word).is_ok()); // undefined var is allowed as literal text in scripts
    }
}
