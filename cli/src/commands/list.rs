use crate::args::SortBy;
use taurine_core::db::crud::{TriggerListItem, get_triggers_list};
use taurine_core::db::init;

pub fn execute(
    sort: Option<SortBy>,
    asc: bool,
    desc: bool,
    json: bool,
    tags: Option<Vec<String>>,
    voice: bool,
) -> taurine_core::error::Result<()> {
    let conn = init::setup()?;
    let mut triggers = get_triggers_list(&conn)?;

    if let Some(ref wanted) = tags {
        triggers.retain(|item| matches_tags(&item.tags, wanted));
    }

    if voice {
        triggers.retain(|item| {
            item.invocations
                .iter()
                .any(|a| a.invocation_type == taurine_core::db::crud::InvocationType::Voice)
        });
        if triggers.is_empty() {
            if json {
                println!("[]");
            } else {
                println!("No active voice triggers found.");
            }
            return Ok(());
        }
    }

    // Determine default direction based on sort type
    let effective_sort = sort.clone().unwrap_or(SortBy::Alpha);
    let is_desc = if desc {
        true
    } else if asc {
        false
    } else {
        match effective_sort {
            SortBy::Alpha => false,
            SortBy::Usage | SortBy::Created | SortBy::Recent => true,
        }
    };

    // Sort the list
    if !matches!(effective_sort, SortBy::Alpha) {
        triggers.sort_by(|a, b| {
            let cmp = match effective_sort {
                SortBy::Alpha => a.display.cmp(&b.display),
                SortBy::Usage => a.usage_count.cmp(&b.usage_count),
                SortBy::Created => a.created_at.cmp(&b.created_at),
                SortBy::Recent => {
                    let a_last = a.last_used_at.unwrap_or(0);
                    let b_last = b.last_used_at.unwrap_or(0);
                    a_last.cmp(&b_last)
                }
            };

            if is_desc { cmp.reverse() } else { cmp }
        });
    }

    if json {
        println!("{}", serde_json::to_string(&triggers).unwrap());
        return Ok(());
    }

    // Build rows for plain output: one row per invocation.
    let mut rows = flatten_rows(&triggers, voice);
    if matches!(effective_sort, SortBy::Alpha) {
        rows.sort_by(|a, b| {
            let cmp = a.trigger.cmp(&b.trigger);
            if is_desc { cmp.reverse() } else { cmp }
        });
    }

    if rows.is_empty() {
        return Ok(());
    }

    // Calculate column widths
    let mut tw = 7; // "TRIGGER" min width
    let mut ow = 6; // "OUTPUT" min width

    for r in &rows {
        tw = tw.max(r.trigger.len());
        ow = ow.max(r.output.len());
    }

    // Print header
    let pad = 3usize;

    println!(
        "{:tw$}{:pad$}{:ow$}",
        "TRIGGER",
        "",
        "OUTPUT",
        tw = tw,
        pad = pad,
        ow = ow,
    );

    // Print rows
    for r in &rows {
        println!(
            "{:tw$}{:pad$}{:ow$}",
            r.trigger,
            "",
            r.output,
            tw = tw,
            pad = pad,
            ow = ow,
        );
    }

    Ok(())
}

fn matches_tags(item_tags_json: &str, wanted: &[String]) -> bool {
    let normalized: std::collections::HashSet<String> = wanted
        .iter()
        .flat_map(|s| s.split(','))
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if normalized.is_empty() {
        return true;
    }
    let stored: std::collections::HashSet<String> =
        serde_json::from_str::<Vec<String>>(item_tags_json)
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
    normalized.iter().all(|t| stored.contains(t))
}

struct Row {
    trigger: String,
    output: String,
}

fn flatten_rows(items: &[TriggerListItem], voice: bool) -> Vec<Row> {
    items
        .iter()
        .flat_map(|item| {
            let raw = if item.action_type == "script" {
                item.script_content.clone().unwrap_or_default()
            } else {
                item.output.clone()
            };
            let output = truncate(&raw.replace('\r', "").replace('\n', " "), 60);
            item.invocations
                .iter()
                .filter(|a| {
                    !voice || a.invocation_type == taurine_core::db::crud::InvocationType::Voice
                })
                .map(|a| Row {
                    trigger: a.invocation.clone(),
                    output: output.clone(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

#[cfg(test)]
mod tests {
    use taurine_core::db::crud::triggers::TriggerListItem;
    use taurine_core::db::crud::{InvocationType, TriggerAliasRow};
    use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

    fn alias_row(invocation: &str, invocation_type: InvocationType) -> TriggerAliasRow {
        TriggerAliasRow {
            id: format!("alias-{invocation}"),
            trigger_id: "entry-1".to_string(),
            invocation: invocation.to_string(),
            invocation_type,
            require_confirmation: false,
            strict_threshold: None,
            created_at: 0,
        }
    }

    #[test]
    fn test_json_output_with_all_fields() {
        let items = vec![TriggerListItem {
            id: "1".to_string(),
            name: "".to_string(),
            description: None,
            invocations: vec![alias_row("gs", InvocationType::Word)],
            display: "gs".to_string(),
            output: "git status".to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            only_apps: Some("terminal".to_string()),
            except_apps: None,
            auto_case: false,
            is_enabled: true,
            usage_count: 42,
            last_used_at: Some(1720000000),
            created_at: 1710000000,
            tags: "[\"dev\",\"git\"]".to_string(),
            script_content: None,
            interpreter: None,
            behavior: None,
        }];

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("\"display\":\"gs\""));
        assert!(json.contains("\"output\":\"git status\""));
        assert!(json.contains("\"usage_count\":42"));
        assert!(json.contains("\"only_apps\":\"terminal\""));
        assert!(json.contains("\"last_used_at\":1720000000"));
    }

    #[test]
    fn test_json_output_with_script_trigger() {
        let items = vec![TriggerListItem {
            id: "s1".to_string(),
            name: "".to_string(),
            description: Some("deploy script".to_string()),
            invocations: vec![alias_row("ctrl+shift+d", InvocationType::Hotkey)],
            display: "ctrl+shift+d".to_string(),
            output: "Inline Bash".to_string(),
            action_type: "script".to_string(),
            target_os: "linux".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            is_enabled: true,
            usage_count: 7,
            last_used_at: None,
            created_at: 1700000000,
            tags: "[\"deploy\"]".to_string(),
            script_content: Some("echo deployed".to_string()),
            interpreter: Some(ScriptInterpreter::Bash),
            behavior: Some(ScriptBehavior::Inline),
        }];

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("\"action_type\":\"script\""));
        assert!(json.contains("\"script_content\":\"echo deployed\""));
        assert!(json.contains("\"interpreter\":\"bash\""));
        assert!(json.contains("\"invocation_type\":\"hotkey\""));
    }

    #[test]
    fn test_json_output_empty_list() {
        let items: Vec<TriggerListItem> = vec![];
        let json = serde_json::to_string(&items).unwrap();
        assert_eq!(json, "[]");
    }

    #[test]
    fn test_json_output_all_nullable_fields_null() {
        let items = vec![TriggerListItem {
            id: "n1".to_string(),
            name: "".to_string(),
            description: None,
            invocations: vec![alias_row("foo", InvocationType::Regex)],
            display: "foo".to_string(),
            output: "bar".to_string(),
            action_type: "text".to_string(),
            target_os: "win".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            is_enabled: true,
            usage_count: 0,
            last_used_at: None,
            created_at: 0,
            tags: "[]".to_string(),
            script_content: None,
            interpreter: None,
            behavior: None,
        }];

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("\"script_content\":null"));
        assert!(json.contains("\"interpreter\":null"));
        assert!(json.contains("\"behavior\":null"));
        assert!(json.contains("\"description\":null"));
        assert!(json.contains("\"last_used_at\":null"));
    }

    #[test]
    fn test_truncate_short_string() {
        use super::truncate;
        assert_eq!(truncate("hello", 60), "hello");
    }

    #[test]
    fn test_truncate_long_string() {
        use super::truncate;
        let long = "a".repeat(100);
        assert_eq!(truncate(&long, 60), format!("{}...", "a".repeat(57)));
    }

    #[test]
    fn test_truncate_exact_boundary() {
        use super::truncate;
        let exact = "a".repeat(60);
        assert_eq!(truncate(&exact, 60), exact);
    }

    #[test]
    fn test_truncate_one_past_boundary() {
        use super::truncate;
        let s = "a".repeat(61);
        assert_eq!(truncate(&s, 60), format!("{}...", "a".repeat(57)));
    }

    #[test]
    fn test_script_shows_content_not_label() {
        let item = TriggerListItem {
            id: "s1".to_string(),
            name: "".to_string(),
            description: Some("script".to_string()),
            invocations: vec![alias_row("test", InvocationType::Hotkey)],
            display: "test".to_string(),
            output: "Bash Inline".to_string(),
            action_type: "script".to_string(),
            target_os: "all".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            is_enabled: true,
            usage_count: 0,
            last_used_at: None,
            created_at: 0,
            tags: "[]".to_string(),
            script_content: Some("echo hello world".to_string()),
            interpreter: Some(ScriptInterpreter::Bash),
            behavior: Some(ScriptBehavior::Inline),
        };

        let display_output = if item.action_type == "script" {
            item.script_content.clone().unwrap_or_default()
        } else {
            item.output.clone()
        };

        assert_eq!(display_output, "echo hello world");
        assert_ne!(display_output, "Bash Inline");
    }

    #[test]
    fn test_script_content_truncated_in_plain_output() {
        let item = TriggerListItem {
            id: "s2".to_string(),
            name: "".to_string(),
            description: None,
            invocations: vec![alias_row("long", InvocationType::Word)],
            display: "long".to_string(),
            output: "label".to_string(),
            action_type: "script".to_string(),
            target_os: "all".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            is_enabled: true,
            usage_count: 0,
            last_used_at: None,
            created_at: 0,
            tags: "[]".to_string(),
            script_content: Some("a".repeat(100)),
            interpreter: Some(ScriptInterpreter::PowerShell),
            behavior: Some(ScriptBehavior::Silent),
        };

        let raw = if item.action_type == "script" {
            item.script_content.clone().unwrap_or_default()
        } else {
            item.output.clone()
        };
        let truncated = super::truncate(&raw, 60);
        assert_eq!(truncated.len(), 60);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn test_alpha_sort_appears_in_json() {
        let items = vec![
            TriggerListItem {
                id: "a".to_string(),
                name: "".to_string(),
                description: None,
                invocations: vec![alias_row("b", InvocationType::Word)],
                display: "b".to_string(),
                output: "two".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                auto_case: false,
                is_enabled: true,
                usage_count: 2,
                last_used_at: None,
                created_at: 100,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            },
            TriggerListItem {
                id: "b".to_string(),
                name: "".to_string(),
                description: None,
                invocations: vec![alias_row("a", InvocationType::Word)],
                display: "a".to_string(),
                output: "one".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
                auto_case: false,
                is_enabled: true,
                usage_count: 1,
                last_used_at: None,
                created_at: 200,
                tags: "[]".to_string(),
                script_content: None,
                interpreter: None,
                behavior: None,
            },
        ];

        // Alpha sort: 'a' should appear before 'b'
        let mut sorted = items.clone();
        sorted.sort_by(|a, b| a.display.cmp(&b.display));
        let json = serde_json::to_string(&sorted).unwrap();
        let pos_a = json.find("\"a\"").unwrap();
        let pos_b = json.rfind("\"b\"").unwrap();
        assert!(pos_a < pos_b, "alpha sort: 'a' should appear before 'b'");
    }

    #[test]
    fn tags_filter_requires_all_requested() {
        assert!(super::matches_tags(
            r#"["work","email"]"#,
            &["work".to_string(), "email".to_string()]
        ));
        assert!(!super::matches_tags(
            r#"["work"]"#,
            &["work".to_string(), "email".to_string()]
        ));
        assert!(super::matches_tags(
            r#"["work","email"]"#,
            &[" Work , EMAIL ".to_string()]
        ));
        assert!(super::matches_tags(r#"["work"]"#, &["".to_string()]));
        assert!(super::matches_tags(r#"["work"]"#, &[]));
        assert!(super::matches_tags(
            r#"["Work","EMAIL"]"#,
            &["work".to_string(), "email".to_string()]
        ));
    }

    fn list_item(
        id: &str,
        display: &str,
        output: &str,
        invocations: Vec<TriggerAliasRow>,
    ) -> TriggerListItem {
        TriggerListItem {
            id: id.to_string(),
            name: "".to_string(),
            description: None,
            invocations,
            display: display.to_string(),
            output: output.to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            only_apps: None,
            except_apps: None,
            auto_case: false,
            is_enabled: true,
            usage_count: 0,
            last_used_at: None,
            created_at: 0,
            tags: "[]".to_string(),
            script_content: None,
            interpreter: None,
            behavior: None,
        }
    }

    #[test]
    fn flatten_lists_each_invocation_on_its_own_row() {
        let items = vec![list_item(
            "e1",
            "1080p",
            "1080p output",
            vec![
                alias_row("1080p", InvocationType::Word),
                alias_row("lalt+f7", InvocationType::Hotkey),
                alias_row("normal mode", InvocationType::Voice),
            ],
        )];

        let rows = super::flatten_rows(&items, false);
        let triggers: Vec<&str> = rows.iter().map(|r| r.trigger.as_str()).collect();
        assert_eq!(triggers, vec!["1080p", "lalt+f7", "normal mode"]);
        for r in &rows {
            assert!(!r.trigger.contains("(+"));
            assert!(!r.trigger.contains(':'));
        }
        assert!(rows.iter().all(|r| r.output == rows[0].output));
    }

    #[test]
    fn voice_flatten_shows_only_voice_without_confirm() {
        let mut confirm = alias_row("do it", InvocationType::Voice);
        confirm.require_confirmation = true;
        let items = vec![
            list_item(
                "w1",
                "type me",
                "word out",
                vec![alias_row("type me", InvocationType::Word)],
            ),
            list_item(
                "h1",
                "lalt+f7",
                "hotkey out",
                vec![alias_row("lalt+f7", InvocationType::Hotkey)],
            ),
            list_item("v1", "do it", "voice out", vec![confirm]),
            list_item(
                "v2",
                "normal mode",
                "voice out",
                vec![alias_row("normal mode", InvocationType::Voice)],
            ),
        ];

        let rows = super::flatten_rows(&items, true);
        let triggers: Vec<&str> = rows.iter().map(|r| r.trigger.as_str()).collect();
        assert_eq!(triggers, vec!["do it", "normal mode"]);
        for r in &rows {
            assert!(!r.trigger.contains("(confirm)"));
            assert!(!r.trigger.contains(':'));
        }
    }

    #[test]
    fn alpha_sort_orders_flattened_rows_by_trigger() {
        let items = vec![
            list_item(
                "z1",
                "zzz",
                "z out",
                vec![alias_row("zzz", InvocationType::Word)],
            ),
            list_item(
                "a1",
                "aaa",
                "a out",
                vec![alias_row("aaa", InvocationType::Word)],
            ),
        ];

        let mut rows = super::flatten_rows(&items, false);
        rows.sort_by(|a, b| a.trigger.cmp(&b.trigger));
        let triggers: Vec<&str> = rows.iter().map(|r| r.trigger.as_str()).collect();
        assert_eq!(triggers, vec!["aaa", "zzz"]);
    }
}
