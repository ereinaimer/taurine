use crate::SortBy;
use comfy_table::{Table, TableComponent, modifiers, presets};
use taurine_core::db::init;
use time::OffsetDateTime;

pub fn execute(
    sort: Option<SortBy>,
    asc: bool,
    desc: bool,
    plain: bool,
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
    // If no sort specified, it's Alpha Asc.
    // If usage, created, recent specified, they default to Desc unless --asc is passed.
    // If alpha specified, it defaults to Asc unless --desc is passed.
    let effective_sort = sort.clone().unwrap_or(SortBy::Alpha);
    let is_desc = if desc {
        true
    } else if asc {
        false
    } else {
        // Default logic
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

    let mut table = Table::new();
    if plain {
        table.load_preset(presets::NOTHING);
    } else {
        table
            .load_preset(presets::UTF8_FULL_CONDENSED)
            .apply_modifier(modifiers::UTF8_ROUND_CORNERS);

        table.set_style(TableComponent::HeaderLines, '─');
        table.set_style(TableComponent::LeftHeaderIntersection, '├');
        table.set_style(TableComponent::MiddleHeaderIntersections, '┼');
        table.set_style(TableComponent::RightHeaderIntersection, '┤');
        table.set_style(TableComponent::VerticalLines, '│');
    }

    // Build headers based on sort option
    let mut headers = vec!["TRIGGER", "OUTPUT"];
    match sort {
        Some(SortBy::Usage) => headers.push("USAGE"),
        Some(SortBy::Created) => headers.push("CREATED AT"),
        _ => {}
    }
    headers.push("TAGS");
    table.set_header(headers);

    for item in triggers {
        let display_output = if item.action_type == "script" {
            let interpreter = match item.interpreter {
                Some(taurine_core::engine::shell::ScriptInterpreter::Bash) => "Bash",
                Some(taurine_core::engine::shell::ScriptInterpreter::PowerShell) => "PowerShell",
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
            item.output
        };

        let tags: Vec<String> = serde_json::from_str(&item.tags).unwrap_or_default();
        let tags_str = tags.join(", ");

        let mut row = vec![item.trigger, display_output];
        match sort {
            Some(SortBy::Usage) => {
                row.push(item.usage_count.to_string());
            }
            Some(SortBy::Created) => {
                row.push(format_relative_time(item.created_at));
            }
            _ => {}
        }
        row.push(tags_str);
        table.add_row(row);
    }

    println!("{table}");
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
    use comfy_table::Table;

    #[test]
    fn test_table_output_with_plain_flag() {
        // Simulating the logic used in execute() when plain is true
        let mut table = Table::new();
        table.load_preset(comfy_table::presets::NOTHING);

        table.set_header(vec!["TRIGGER", "OUTPUT"]);
        table.add_row(vec!["gs", "git status"]);

        let output = table.to_string();

        // Assertions: Ensure no box-drawing or decoration characters exist
        let decoration_chars = ['│', '─', '┌', '┐', '└', '┘', '├', '┤', '┬', '┴', '┼', '═'];
        for ch in decoration_chars {
            assert!(
                !output.contains(ch),
                "Output should not contain decoration character '{}' when --plain is used",
                ch
            );
        }

        // Ensure data is still present and separated by whitespace
        assert!(output.contains("gs"));
        assert!(output.contains("git status"));
    }
}
