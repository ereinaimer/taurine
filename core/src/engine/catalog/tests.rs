use super::*;
use crate::db::crud::TriggerAction;
use crate::engine::shell::{ScriptBehavior, ScriptInterpreter, compress, decompress};
use crate::engine::source::MemorySource;
use crate::engine::variables::{ArgMap, ExpansionStep};
use crate::keys::{LogicalKey, parse_hotkey};
use std::sync::Arc;

#[test]
fn exact_match_precedence_beats_hybrid_argument_parsing() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());

    memory.load_actions(vec![
        ("hi".to_string(), TriggerAction::text("base [0] ([mood])")),
        (
            "hi:erin".to_string(),
            TriggerAction::text("exact trigger wins"),
        ),
    ]);

    let expansion = catalog.fetch_expansion("hi:erin", false, None).unwrap();
    assert_eq!(
        expansion.steps[0],
        ExpansionStep::Text("exact trigger wins".to_string())
    );
    assert!(!expansion.is_calculation);
}

#[test]
fn raw_action_lookup_uses_case_fallback_after_exact_match_miss() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());

    memory.load_actions(vec![
        ("gm".to_string(), TriggerAction::text("lowercase")),
        ("GM".to_string(), TriggerAction::text("UPPERCASE")),
        (
            "only_low".to_string(),
            TriggerAction::text("only lowercase"),
        ),
    ]);

    assert_eq!(
        catalog.fetch_expansion("gm", false, None).unwrap().steps[0],
        ExpansionStep::Text("lowercase".to_string())
    );
    assert_eq!(
        catalog.fetch_expansion("GM", false, None).unwrap().steps[0],
        ExpansionStep::Text("UPPERCASE".to_string())
    );
    assert_eq!(
        catalog.fetch_expansion("Gm", false, None).unwrap().steps[0],
        ExpansionStep::Text("lowercase".to_string())
    );
    assert_eq!(
        catalog
            .fetch_expansion("ONLY_LOW", false, None)
            .unwrap()
            .steps[0],
        ExpansionStep::Text("only lowercase".to_string())
    );
    assert!(catalog.fetch_expansion("unknown", false, None).is_none());
    assert!(catalog.fetch_expansion("UNKNOWN", false, None).is_none());
}

#[test]
fn script_interpolation_with_positional_args_matches_current_behavior() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());

    let script = "explorer [0=C:\\Temp]";
    let compressed = compress(script).unwrap();

    let action = TriggerAction {
        output: String::new(),
        action_type: "script".to_string(),
        only_apps: None,
        except_apps: None,
        auto_case: false,
        interpreter: Some(ScriptInterpreter::PowerShell),
        behavior: Some(ScriptBehavior::Inline),
        script_binary: Some(compressed),
    };

    memory.load_actions(vec![("opendir".to_string(), action)]);

    let expansion = catalog
        .fetch_expansion("opendir:\"C:\\Temp\"", false, None)
        .unwrap();
    if let ExpansionStep::Script(md) = &expansion.steps[0] {
        let decompressed = decompress(&md.compressed_content).unwrap();
        assert_eq!(decompressed, "explorer C:\\Temp");
    } else {
        panic!("Expected script expansion");
    }
}

#[test]
fn script_interpolation_with_named_args_matches_current_behavior() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());

    let script = "curl https://[env=].example.com";
    let compressed = compress(script).unwrap();

    let action = TriggerAction {
        output: String::new(),
        action_type: "script".to_string(),
        only_apps: None,
        except_apps: None,
        auto_case: false,
        interpreter: Some(ScriptInterpreter::Bash),
        behavior: Some(ScriptBehavior::Silent),
        script_binary: Some(compressed),
    };

    memory.load_actions(vec![("api".to_string(), action)]);

    let expansion = catalog
        .fetch_expansion("api:env=prod", false, None)
        .unwrap();
    if let ExpansionStep::Script(md) = &expansion.steps[0] {
        let decompressed = decompress(&md.compressed_content).unwrap();
        assert_eq!(decompressed, "curl https://prod.example.com");
    } else {
        panic!("Expected script expansion");
    }
}

#[test]
fn math_fallback_only_runs_after_snippet_tiers_miss() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());

    memory.load_actions(vec![(
        "5+2".to_string(),
        TriggerAction::text("exact snippet"),
    )]);

    let expansion = catalog.fetch_expansion("5+2", false, None).unwrap();
    assert_eq!(
        expansion.steps[0],
        ExpansionStep::Text("exact snippet".to_string())
    );
    assert!(!expansion.is_calculation);

    let fallback = catalog.fetch_expansion("7*6", false, None).unwrap();
    assert_eq!(fallback.steps[0], ExpansionStep::Text("42".to_string()));
    assert!(fallback.is_calculation);
}

#[test]
fn math_fallback_skipped_in_instant_expand() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());
    assert!(catalog.fetch_expansion("7*6", true, None).is_none());
}

#[test]
fn test_no_nl_emoji_fallback_in_catalog() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());
    crate::settings::set_cached_inline_emoji_enabled(true);

    // NL emoji expansion is now only in the evaluator, not in catalog fallback
    assert!(catalog.fetch_expansion("rocket", false, None).is_none());
    assert!(catalog.fetch_expansion("❤️", false, None).is_none());
    assert!(catalog.fetch_expansion("heart", false, None).is_none());
}

#[test]
fn currency_words_fallback_only_runs_when_enabled() {
    let memory = Arc::new(MemorySource::new());
    let catalog = ExpansionCatalog::with_source(memory.clone());

    // Disabled by default
    crate::settings::set_cached_inline_currency_to_words_enabled(false);
    assert!(catalog.fetch_expansion("$1,200", false, None).is_none());

    // Enabled
    crate::settings::set_cached_inline_currency_to_words_enabled(true);
    let expansion = catalog.fetch_expansion("$1,200", false, None).unwrap();
    assert_eq!(
        expansion.steps[0],
        ExpansionStep::Text("One thousand two hundred dollars".to_string())
    );
    assert!(expansion.is_calculation);
}

#[test]
fn matching_triggers_returns_sorted_prefix_matches() {
    let catalog = ExpansionCatalog::new();
    catalog.load_actions(vec![
        ("gpush".to_string(), TriggerAction::text("git push")),
        ("gs".to_string(), TriggerAction::text("git status")),
        ("gco".to_string(), TriggerAction::text("git checkout")),
        ("note".to_string(), TriggerAction::text("not a g trigger")),
    ]);

    assert_eq!(
        catalog.matching_triggers("g"),
        vec!["gco".to_string(), "gpush".to_string(), "gs".to_string()]
    );
}

#[test]
fn matching_triggers_uses_case_insensitive_prefix_matching() {
    let catalog = ExpansionCatalog::new();
    catalog.load_actions(vec![
        ("gm".to_string(), TriggerAction::text("good morning")),
        ("GitHub".to_string(), TriggerAction::text("github")),
    ]);

    assert_eq!(
        catalog.matching_triggers("G"),
        vec!["GitHub".to_string(), "gm".to_string()]
    );
}

#[test]
fn hotkey_catalog_loads_actions_without_affecting_word_expansion_lookup() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![(
        "ctrl+shift+g".to_string(),
        TriggerAction::text("git status"),
    )]);

    let action = hotkeys.get_action("ctrl+shift+g").unwrap();
    assert_eq!(action.output, "git status");

    let word_catalog = ExpansionCatalog::new();
    assert!(
        word_catalog
            .fetch_expansion("ctrl+shift+g", false, None)
            .is_none()
    );
}

#[test]
fn hotkey_catalog_matches_generic_fallback_after_exact_side_miss() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![(
        "alt+m".to_string(),
        TriggerAction::text("generic alt"),
    )]);

    let (trigger, action) = hotkeys
        .match_action(parse_hotkey("ralt+m").unwrap(), None)
        .unwrap();
    assert_eq!(trigger, "alt+m");
    assert_eq!(action.output, "generic alt");
}

#[test]
fn hotkey_catalog_prefers_exact_side_specific_match() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![
        ("alt+m".to_string(), TriggerAction::text("generic alt")),
        ("ralt+m".to_string(), TriggerAction::text("right alt")),
    ]);

    let (trigger, action) = hotkeys
        .match_action(parse_hotkey("ralt+m").unwrap(), None)
        .unwrap();
    assert_eq!(trigger, "ralt+m");
    assert_eq!(action.output, "right alt");
}

#[test]
fn hotkey_catalog_exact_match_returns_configured_alias_not_canonical_trigger() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![(
        "altgr+m".to_string(),
        TriggerAction::text("configured alias"),
    )]);

    let (trigger, action) = hotkeys
        .match_action(parse_hotkey("ralt+m").unwrap(), None)
        .unwrap();
    assert_eq!(trigger, "altgr+m");
    assert_eq!(action.output, "configured alias");
}

#[test]
fn test_app_gating_prefix_rules() {
    let mut action = TriggerAction::text("dummy");

    // 1. exe: prefix (exact match, case-insensitive, strips .exe)
    action.only_apps = Some("exe:chrome,exe:firefox".to_string());

    let info_chrome = serde_json::to_string(&ActiveWindowInfo {
        exec_name: Some("Chrome.exe".to_string()),
        ..Default::default()
    })
    .unwrap();
    let info_firefox = serde_json::to_string(&ActiveWindowInfo {
        exec_name: Some("firefox".to_string()),
        ..Default::default()
    })
    .unwrap();
    let info_notepad = serde_json::to_string(&ActiveWindowInfo {
        exec_name: Some("notepad.exe".to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(is_app_allowed(&action, Some(&info_chrome)));
    assert!(is_app_allowed(&action, Some(&info_firefox)));
    assert!(!is_app_allowed(&action, Some(&info_notepad)));

    // 2. class: prefix (exact match, case-insensitive)
    action.only_apps = Some("class:CabinetWClass".to_string());
    let info_class_match = serde_json::to_string(&ActiveWindowInfo {
        class: Some("cabinetwclass".to_string()),
        ..Default::default()
    })
    .unwrap();
    let info_class_miss = serde_json::to_string(&ActiveWindowInfo {
        class: Some("Chrome_WidgetWin_1".to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(is_app_allowed(&action, Some(&info_class_match)));
    assert!(!is_app_allowed(&action, Some(&info_class_miss)));

    // 3. title: prefix (substring match, case-insensitive)
    action.only_apps = Some("title:Github,title:Google".to_string());
    let info_title_match = serde_json::to_string(&ActiveWindowInfo {
        title: Some("Taurine Pull Request - GitHub - Google Chrome".to_string()),
        ..Default::default()
    })
    .unwrap();
    let info_title_miss = serde_json::to_string(&ActiveWindowInfo {
        title: Some("Index of /docs".to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(is_app_allowed(&action, Some(&info_title_match)));
    assert!(!is_app_allowed(&action, Some(&info_title_miss)));

    // 4. Default no prefix (exe match)
    action.only_apps = Some("chrome".to_string());
    assert!(is_app_allowed(&action, Some(&info_chrome)));
    assert!(!is_app_allowed(&action, Some(&info_notepad)));

    // 5. Exclude filters
    action.only_apps = None;
    action.except_apps = Some("title:Gmail,exe:doom".to_string());

    let info_gmail = serde_json::to_string(&ActiveWindowInfo {
        title: Some("Inbox (1) - Gmail".to_string()),
        ..Default::default()
    })
    .unwrap();
    let info_doom = serde_json::to_string(&ActiveWindowInfo {
        exec_name: Some("doom.exe".to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(!is_app_allowed(&action, Some(&info_gmail)));
    assert!(!is_app_allowed(&action, Some(&info_doom)));
    assert!(is_app_allowed(&action, Some(&info_chrome)));

    // 6. Strict mode (None active window blocks if filters are active)
    assert!(!is_app_allowed(&action, None));

    // 7. Full path match (contains path separators)
    action.except_apps = Some("exe:/usr/bin/python3,exe:C:\\bin\\python.exe".to_string());
    let info_python_linux = serde_json::to_string(&ActiveWindowInfo {
        exec_path: Some("/usr/bin/python3".to_string()),
        ..Default::default()
    })
    .unwrap();
    let info_python_win = serde_json::to_string(&ActiveWindowInfo {
        exec_path: Some("c:\\bin\\python.exe".to_string()),
        ..Default::default()
    })
    .unwrap();
    let info_python_other = serde_json::to_string(&ActiveWindowInfo {
        exec_path: Some("/usr/local/bin/python3".to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(!is_app_allowed(&action, Some(&info_python_linux)));
    assert!(!is_app_allowed(&action, Some(&info_python_win)));
    assert!(is_app_allowed(&action, Some(&info_python_other)));

    // 8. Path without prefix containing colon (Windows path edge case)
    action.except_apps = Some("C:\\bin\\python.exe".to_string());
    assert!(!is_app_allowed(&action, Some(&info_python_win)));
    assert!(is_app_allowed(&action, Some(&info_python_other)));

    // 9. Slash normalization (forward vs backward slashes)
    action.except_apps = Some("exe:C:/bin/python.exe".to_string());
    assert!(!is_app_allowed(&action, Some(&info_python_win)));
}

#[test]
fn entry_has_app_filters_returns_true_when_only_apps_set() {
    let action = TriggerAction {
        only_apps: Some("chrome".to_string()),
        ..TriggerAction::text("dummy")
    };
    assert!(entry_has_app_filters(&action));
}

#[test]
fn entry_has_app_filters_returns_true_when_except_apps_set() {
    let action = TriggerAction {
        except_apps: Some("notepad".to_string()),
        ..TriggerAction::text("dummy")
    };
    assert!(entry_has_app_filters(&action));
}

#[test]
fn entry_has_app_filters_returns_false_when_no_filters() {
    let action = TriggerAction::text("dummy");
    assert!(!entry_has_app_filters(&action));
}

#[test]
fn match_action_lazy_matches_entry_without_filters_without_calling_fetcher() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![(
        "ctrl+shift+g".to_string(),
        TriggerAction::text("git status"),
    )]);

    let called = std::cell::Cell::new(false);
    let result = hotkeys.match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
        called.set(true);
        Some("chrome.exe".to_string())
    });
    assert!(
        !called.get(),
        "fetcher should not be called for filterless entry"
    );
    let (trigger, action) = result.unwrap();
    assert_eq!(trigger, "ctrl+shift+g");
    assert_eq!(action.output, "git status");
}

#[test]
fn match_action_lazy_prefers_canonical_match_over_hotkey_matches_when_both_have_filters() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![
        (
            "alt+m".to_string(),
            TriggerAction {
                output: "generic alt".to_string(),
                only_apps: Some("chrome".to_string()),
                ..TriggerAction::text("")
            },
        ),
        (
            "ralt+m".to_string(),
            TriggerAction {
                output: "right alt".to_string(),
                only_apps: Some("chrome".to_string()),
                ..TriggerAction::text("")
            },
        ),
    ]);

    let (trigger, action) = hotkeys
        .match_action_lazy(parse_hotkey("ralt+m").unwrap(), || {
            Some("chrome.exe".to_string())
        })
        .unwrap();
    assert_eq!(trigger, "ralt+m");
    assert_eq!(action.output, "right alt");
}

#[test]
fn match_action_lazy_matches_app_filtered_entry_in_correct_window() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![(
        "ctrl+shift+g".to_string(),
        TriggerAction {
            output: "only in chrome".to_string(),
            only_apps: Some("chrome".to_string()),
            ..TriggerAction::text("")
        },
    )]);

    let (trigger, action) = hotkeys
        .match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
            Some("chrome.exe".to_string())
        })
        .unwrap();
    assert_eq!(trigger, "ctrl+shift+g");
    assert_eq!(action.output, "only in chrome");
}

#[test]
fn match_action_lazy_does_not_match_app_filtered_entry_in_wrong_window() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![(
        "ctrl+shift+g".to_string(),
        TriggerAction {
            output: "chrome only".to_string(),
            only_apps: Some("chrome".to_string()),
            ..TriggerAction::text("")
        },
    )]);

    let result = hotkeys.match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
        Some("notepad.exe".to_string())
    });
    assert!(result.is_none());
}

#[test]
fn match_action_lazy_returns_none_on_empty_catalog() {
    let hotkeys = HotkeyCatalog::new();
    let result = hotkeys.match_action_lazy(parse_hotkey("ctrl+shift+g").unwrap(), || {
        Some("chrome.exe".to_string())
    });
    assert!(result.is_none());
}

#[test]
fn hotkey_catalog_has_entry_for_returns_false_when_empty() {
    let hotkeys = HotkeyCatalog::new();
    assert!(!hotkeys.has_entry_for(LogicalKey::Letter('g')));
}

#[test]
fn hotkey_catalog_has_entry_for_returns_true_when_entries_exist() {
    let hotkeys = HotkeyCatalog::new();
    hotkeys.load_actions(vec![(
        "ctrl+shift+g".to_string(),
        TriggerAction::text("git status"),
    )]);
    assert!(hotkeys.has_entry_for(LogicalKey::Letter('g')));
    assert!(!hotkeys.has_entry_for(LogicalKey::Letter('x')));
}

#[test]
fn test_regex_catalog_compilation_and_match() {
    let catalog = RegexCatalog::new();
    catalog.load_actions(vec![
        (
            "issue-(\\d+)".to_string(),
            TriggerAction::text("https://github.com/issues/[0]"),
        ),
        (
            "invalid(pattern".to_string(),
            TriggerAction::text("skipped"),
        ),
    ]);
    let matched = catalog.match_action("my issue-102", None);
    assert!(matched.is_some());
    let (trigger, action, caps) = matched.unwrap();
    assert_eq!(trigger, "issue-102");
    assert_eq!(caps, vec!["102".to_string()]);

    let arg_map = ArgMap {
        positional: caps,
        ..Default::default()
    };
    let expansion = expand_trigger_action_with_args(action, &arg_map, &trigger).unwrap();
    assert_eq!(expansion.steps.len(), 1);
    if let ExpansionStep::Text(ref text) = expansion.steps[0] {
        assert_eq!(text, "https://github.com/issues/102");
    } else {
        panic!("Expected text expansion step");
    }
}
