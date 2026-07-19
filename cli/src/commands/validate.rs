#[cfg(test)]
use taurine_core::db::crud::TriggerType;
pub use taurine_core::db::crud::{
    PreparedTrigger, audit_payload_tags, audit_payload_tags_with_trigger_type, prepare_trigger,
};
use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

pub fn format_automation_log(
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
            ScriptInterpreter::NodeEsm => "node-esm",
            ScriptInterpreter::Cmd => "cmd",
        };
        parts.push(format!(
            "{} {} script automation using {} for '{}'",
            action, behavior_str, lang_str, trigger
        ));
    } else {
        parts.push(format!("{} automation for '{}'", action, trigger));
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
        assert!(audit_payload_tags("[time.utc | upper] [env(USERPROFILE)]").is_ok());
        assert!(audit_payload_tags("[net.ip] [net.lip] [net.online] [net.port(8080)]").is_ok());
        assert!(audit_payload_tags("json = \\[1, 2, 3\\]").is_ok());
        assert!(audit_payload_tags("[name=John | upper]").is_ok());
        assert!(audit_payload_tags("[clip | ai(\"summarize\")]").is_ok());
    }

    #[test]
    fn rejects_invalid_system_modifier() {
        let error = audit_payload_tags("[time.india]").unwrap_err();
        assert!(error.to_string().contains("time.india"));
        assert!(error.to_string().contains("Valid modifiers"));
    }

    #[test]
    fn rejects_unknown_net_modifier() {
        let error = audit_payload_tags("[net.unknown]").unwrap_err();
        assert!(error.to_string().contains("net.unknown"));
        assert!(error.to_string().contains("ip"));
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
        assert!(audit_payload_tags("[lorem.word([num=5])]").is_ok());
        assert!(audit_payload_tags("[lorem.word([random.int(3, 3)])]").is_ok());
    }

    #[test]
    fn rejects_missing_default_assignment() {
        let error = audit_payload_tags("[name | upper]").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("dynamic variables must have a default value assignment")
        );

        let error2 = audit_payload_tags("json = [1, 2, 3]").unwrap_err();
        assert!(
            error2
                .to_string()
                .contains("dynamic variables must have a default value assignment")
        );
    }

    #[test]
    fn rejects_empty_default_assignment() {
        let error = audit_payload_tags("[name=]").unwrap_err();
        assert!(
            error
                .to_string()
                .contains("default assignments cannot be empty")
        );

        let error2 = audit_payload_tags("[name=\"\"]").unwrap_err();
        assert!(
            error2
                .to_string()
                .contains("default assignments cannot be empty")
        );

        let error3 = audit_payload_tags("[name=   | upper]").unwrap_err();
        assert!(
            error3
                .to_string()
                .contains("default assignments cannot be empty")
        );
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

    #[test]
    fn prepare_trigger_rejects_word_triggers_with_spaces_or_newlines() {
        let error_space = prepare_trigger("hello world", false, "all").unwrap_err();
        assert!(
            error_space
                .to_string()
                .contains("cannot contain spaces or newlines")
        );

        let error_newline = prepare_trigger("hello\nworld", false, "all").unwrap_err();
        assert!(
            error_newline
                .to_string()
                .contains("cannot contain spaces or newlines")
        );

        let error_cr = prepare_trigger("hello\rworld", false, "all").unwrap_err();
        assert!(
            error_cr
                .to_string()
                .contains("cannot contain spaces or newlines")
        );
    }

    #[test]
    fn accepts_nested_and_reused_variables() {
        assert!(audit_payload_tags("Status: [http.status(https://httpbin.org/status/200)] | UA: [[http.get(https://httpbin.org/headers)] | json.get('headers.User-Agent') | truncate(15)]").is_ok());
        assert!(audit_payload_tags("User [name='Developer'] checked [url='httpbin.org/json'] at [time.utc.format(HH:mm)] UTC. Title of JSON: [http.get([url]) | json.get('slideshow.title') | upper]").is_ok());
    }

    #[test]
    fn test_template_syntax_spec_compliance_validation() {
        // Positive Test Cases
        assert!(audit_payload_tags("Hello [0=friend]! You live in [1='San Francisco'] and work as [role='Software Engineer'].").is_ok());
        assert!(
            audit_payload_tags_with_trigger_type(
                "Hello [0]! You live in [1] and [2]",
                TriggerType::Regex
            )
            .is_ok()
        );
        assert!(audit_payload_tags("Escaped brackets: \\[0=ignored\\] | Literal pipe: [0='default value' \\| upper] | Parsed pipe: [0='hello' | upper]").is_ok());
        assert!(audit_payload_tags("Local: [date] [time] | UTC +1w: [date.utc.calc(+1w).format('Today is' dddd, MMMM D, YYYY)] | UTC Time -2h: [time.utc.calc(-2h).format(hh:mm A)] | Cased AM/PM: [time.format(A) | lower]").is_ok());
        assert!(audit_payload_tags("User (Title Case): [env(USERNAME) | title] | Home Path (Lowercase): [env(USERPROFILE) | lower]").is_ok());
        assert!(audit_payload_tags("Full Content: [file.read(~/taurine_test.txt) | trim] | Line 2: [file.read_line(~/taurine_test.txt, 2) | upper] | Lines 1-3: [file.read_line(~/taurine_test.txt, 1, 3)]").is_ok());
        assert!(audit_payload_tags("Latest (Slugified): [clip | slug] | Second: [clip(0) | trim] | Third (Upper): [clip(1) | upper] | Empty index: [clip(2) | squote]").is_ok());
        assert!(audit_payload_tags("Cwd Path: [exec.powershell((Get-Location).Path) | trim] | Cmd Command: [exec.cmd(echo hello from cmd) | upper] | Silent Task: [exec.silent.powershell(echo 'background task')]").is_ok());
        assert!(audit_payload_tags("Status: [http.status(https://httpbin.org/status/200)] | UA: [http.get(https://httpbin.org/headers) | json.get('headers.User-Agent') | truncate(15)]").is_ok());
        assert!(audit_payload_tags("Status: [http.status(https://httpbin.org/status/200)] | UA: [[http.get(https://httpbin.org/headers)] | json.get('headers.User-Agent') | truncate(15)]").is_ok());
        assert!(audit_payload_tags("Int (10-50): [random.int(10, 50)] | Pass (12): [random.pass(12)] | Choice: [random.choice(apple, banana, cherry) | title] | Lorem (Dynamic Count): [lorem.word([random.int(2, 4)]) | kebab]").is_ok());
        assert!(audit_payload_tags("Name: [mock.name | upper] | Email: [mock.email] | Address: [mock.address | title] | Job Title: [mock.job_title | kebab]").is_ok());
        assert!(audit_payload_tags("Output: [use('testinner') | upper] | Date: [date]").is_ok());
        assert!(audit_payload_tags("User [name='Developer'] checked [url='httpbin.org/json'] at [time.utc.format(HH:mm)] UTC. Title of JSON: [http.get([url]) | json.get('slideshow.title') | upper]").is_ok());
        assert!(audit_payload_tags("[0=first][key(tab)][delay(100ms)][1=second][key(tab)][delay(50)][2=third][key(enter)]").is_ok());

        // Negative Test Cases
        assert!(audit_payload_tags("User: [name=]").is_err());
        assert!(audit_payload_tags("User: [name]").is_err());
        assert!(audit_payload_tags("Hello [0]!").is_err());
        assert!(audit_payload_tags("User: [my.custom.var=there]").is_err());
        assert!(audit_payload_tags("Start: [cursor] End: [cursor]").is_err());
        assert!(audit_payload_tags("Hello: [cursor] [key(tab)]").is_err());
        assert!(audit_payload_tags("Time: [time=12:00]").is_err());
    }

    #[test]
    fn test_format_automation_log_sentences() {
        use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

        // Text automation tests
        assert_eq!(
            format_automation_log("Added", "ctrl+alt+p", None, "all", None, None),
            "Added automation for 'ctrl+alt+p'."
        );
        assert_eq!(
            format_automation_log("Updated", "ctrl+alt+p", None, "win", None, None),
            "Updated automation for 'ctrl+alt+p' on Windows."
        );
        assert_eq!(
            format_automation_log(
                "Added",
                "ctrl+alt+p",
                None,
                "mac",
                Some("exe:notepad"),
                None
            ),
            "Added automation for 'ctrl+alt+p' on macOS when active in 'exe:notepad'."
        );
        assert_eq!(
            format_automation_log("Added", "ctrl+alt+p", None, "linux", None, Some("exe:code")),
            "Added automation for 'ctrl+alt+p' on Linux except when active in 'exe:code'."
        );
        assert_eq!(
            format_automation_log(
                "Added",
                "ctrl+alt+p",
                None,
                "win",
                Some("exe:notepad"),
                Some("exe:code")
            ),
            "Added automation for 'ctrl+alt+p' on Windows when active in 'exe:notepad' and except when active in 'exe:code'."
        );

        // Script automation tests
        assert_eq!(
            format_automation_log(
                "Added",
                "ctrl+alt+p",
                Some((ScriptBehavior::Inline, ScriptInterpreter::Bash)),
                "all",
                None,
                None
            ),
            "Added inline script automation using bash for 'ctrl+alt+p'."
        );
        assert_eq!(
            format_automation_log(
                "Updated",
                "ctrl+alt+p",
                Some((ScriptBehavior::Silent, ScriptInterpreter::PowerShell)),
                "win",
                Some("exe:notepad"),
                None
            ),
            "Updated silent script automation using powershell for 'ctrl+alt+p' on Windows when active in 'exe:notepad'."
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

        // Traditional invalid dot-namespace variable still fails if we treat it as word payload
        assert!(super::audit_payload_tags("User: [my.custom.var=there]").is_err());

        // System variables and defined variables in scripts should still be checked
        assert!(audit_script_payload_tags("[time.invalid_modifier]", TriggerType::Word).is_err());
        assert!(audit_script_payload_tags("[my_var]", TriggerType::Word).is_ok()); // undefined var is allowed as literal text in scripts
    }
}
