use crate::SortBy;
use taurine_core::db::init;
use time::OffsetDateTime;

pub fn execute(
    sort: Option<SortBy>,
    asc: bool,
    desc: bool,
    json: bool,
    tag: Option<String>,
) -> taurine_core::error::Result<()> {
    use taurine_core::db::crud::get_triggers_list;

    let conn = init::setup()?;
    let mut triggers = get_triggers_list(&conn)?;

    if let Some(ref t) = tag {
        triggers.retain(|item| {
            let tags: Vec<String> = serde_json::from_str(&item.tags).unwrap_or_default();
            tags.contains(t)
        });
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
    triggers.sort_by(|a, b| {
        let cmp = match effective_sort {
            SortBy::Alpha => a.trigger.cmp(&b.trigger),
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

    if json {
        println!("{}", serde_json::to_string(&triggers).unwrap());
        return Ok(());
    }

    // Build rows for plain output
    struct Row {
        trigger: String,
        output: String,
        sort_col: Option<String>,
        tags: String,
    }

    let rows: Vec<Row> = triggers
        .iter()
        .map(|item| {
            let display_output = if item.action_type == "script" {
                let interpreter = match item.interpreter {
                    Some(taurine_core::engine::shell::ScriptInterpreter::Bash) => "Bash",
                    Some(taurine_core::engine::shell::ScriptInterpreter::PowerShell) => {
                        "PowerShell"
                    }
                    Some(taurine_core::engine::shell::ScriptInterpreter::Python) => "Python",
                    Some(taurine_core::engine::shell::ScriptInterpreter::Node) => "Node",
                    Some(taurine_core::engine::shell::ScriptInterpreter::NodeEsm) => "Node(ESM)",
                    Some(taurine_core::engine::shell::ScriptInterpreter::Cmd) => "Cmd",
                    None => "Unknown",
                };
                let behavior = match item.behavior {
                    Some(taurine_core::engine::shell::ScriptBehavior::Inline) => "Inline",
                    Some(taurine_core::engine::shell::ScriptBehavior::Silent) => "Silent",
                    None => "Unknown",
                };
                format!("{} {}", behavior, interpreter)
            } else {
                item.output.clone()
            };

            let tags: Vec<String> = serde_json::from_str(&item.tags).unwrap_or_default();
            let tags_str = tags.join(", ");

            let sort_col = match effective_sort {
                SortBy::Usage => Some(item.usage_count.to_string()),
                SortBy::Created => Some(format_relative_time(item.created_at)),
                _ => None,
            };

            Row {
                trigger: item.trigger.clone(),
                output: display_output,
                sort_col,
                tags: tags_str,
            }
        })
        .collect();

    if rows.is_empty() {
        return Ok(());
    }

    // Calculate column widths
    let mut tw = 7; // "TRIGGER" min width
    let mut ow = 6; // "OUTPUT" min width
    let mut sw = 0; // sort column
    let mut taw = 4; // "TAGS" min width

    for r in &rows {
        tw = tw.max(r.trigger.len());
        ow = ow.max(r.output.len());
        if let Some(ref s) = r.sort_col {
            sw = sw.max(s.len());
        }
        taw = taw.max(r.tags.len());
    }

    // Print header
    let sort_header = match effective_sort {
        SortBy::Usage => "USAGE",
        SortBy::Created => "CREATED AT",
        _ => "",
    };
    sw = sw.max(sort_header.len());

    let pad = 2usize;

    if sort_header.is_empty() {
        println!(
            "{:tw$}{:pad$}{:ow$}{:pad$}TAGS",
            "TRIGGER",
            "",
            "OUTPUT",
            "",
            tw = tw,
            pad = pad,
            ow = ow,
        );
    } else {
        println!(
            "{:tw$}{:pad$}{:ow$}{:pad$}{:sw$}{:pad$}TAGS",
            "TRIGGER",
            "",
            "OUTPUT",
            "",
            sort_header,
            "",
            tw = tw,
            pad = pad,
            ow = ow,
            sw = sw,
        );
    }

    // Print separator
    let total_width = if sort_header.is_empty() {
        tw + pad + ow + pad + taw
    } else {
        tw + pad + ow + pad + sw + pad + taw
    };
    println!("{}", "-".repeat(total_width));

    // Print rows
    for r in &rows {
        if let Some(ref s) = r.sort_col {
            println!(
                "{:tw$}{:pad$}{:ow$}{:pad$}{:sw$}{:pad$}{}",
                r.trigger,
                "",
                r.output,
                "",
                s,
                "",
                r.tags,
                tw = tw,
                pad = pad,
                ow = ow,
                sw = sw,
            );
        } else {
            println!(
                "{:tw$}{:pad$}{:ow$}{:pad$}{}",
                r.trigger,
                "",
                r.output,
                "",
                r.tags,
                tw = tw,
                pad = pad,
                ow = ow,
            );
        }
    }

    Ok(())
}

fn format_relative_time(timestamp: i64) -> String {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let diff = now - timestamp;

    if diff < 60 {
        return "just now".to_string();
    }

    let mins = diff / 60;
    if mins < 60 {
        return format!("{}m ago", mins);
    }

    let hours = mins / 60;
    if hours < 24 {
        return format!("{}h ago", hours);
    }

    let days = hours / 24;
    if days < 30 {
        return format!("{}d ago", days);
    }

    let months = days / 30;
    if months < 12 {
        return format!("{}mo ago", months);
    }

    let years = days / 365;
    format!("{}y ago", years)
}

#[cfg(test)]
mod tests {
    use taurine_core::db::crud::TriggerType;
    use taurine_core::db::crud::triggers::TriggerListItem;
    use taurine_core::engine::shell::{ScriptBehavior, ScriptInterpreter};

    #[test]
    fn test_json_output_with_all_fields() {
        let items = vec![TriggerListItem {
            id: "1".to_string(),
            name: "".to_string(),
            description: None,
            trigger_type: TriggerType::Word,
            trigger: "gs".to_string(),
            output: "git status".to_string(),
            action_type: "text".to_string(),
            target_os: "all".to_string(),
            only_apps: Some("terminal".to_string()),
            except_apps: None,
            usage_count: 42,
            last_used_at: Some(1720000000),
            created_at: 1710000000,
            tags: "[\"dev\",\"git\"]".to_string(),
            script_content: None,
            interpreter: None,
            behavior: None,
        }];

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("\"trigger\":\"gs\""));
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
            trigger_type: TriggerType::Hotkey,
            trigger: "ctrl+shift+d".to_string(),
            output: "Inline Bash".to_string(),
            action_type: "script".to_string(),
            target_os: "linux".to_string(),
            only_apps: None,
            except_apps: None,
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
        assert!(json.contains("\"trigger_type\":\"hotkey\""));
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
            trigger_type: TriggerType::Regex,
            trigger: "foo".to_string(),
            output: "bar".to_string(),
            action_type: "text".to_string(),
            target_os: "win".to_string(),
            only_apps: None,
            except_apps: None,
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
    fn test_relative_time_format() {
        use super::format_relative_time;
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        assert_eq!(format_relative_time(now - 10), "just now");
        assert_eq!(format_relative_time(now - 120), "2m ago");
        assert_eq!(format_relative_time(now - 7200), "2h ago");
        assert_eq!(format_relative_time(now - 172800), "2d ago");
        assert_eq!(format_relative_time(now - 5184000), "2mo ago");
        assert_eq!(format_relative_time(now - 63072000), "2y ago");
    }

    #[test]
    fn test_alpha_sort_appears_in_json() {
        let items = vec![
            TriggerListItem {
                id: "a".to_string(),
                name: "".to_string(),
                description: None,
                trigger_type: TriggerType::Word,
                trigger: "b".to_string(),
                output: "two".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
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
                trigger_type: TriggerType::Word,
                trigger: "a".to_string(),
                output: "one".to_string(),
                action_type: "text".to_string(),
                target_os: "all".to_string(),
                only_apps: None,
                except_apps: None,
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
        sorted.sort_by(|a, b| a.trigger.cmp(&b.trigger));
        let json = serde_json::to_string(&sorted).unwrap();
        let pos_a = json.find("\"a\"").unwrap();
        let pos_b = json.rfind("\"b\"").unwrap();
        assert!(pos_a < pos_b, "alpha sort: 'a' should appear before 'b'");
    }
}
