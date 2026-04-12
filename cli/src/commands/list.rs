use crate::SortBy;
use comfy_table::{Table, TableComponent, modifiers, presets};
use taurine_core::db::init;
use time::OffsetDateTime;

pub fn execute(sort: Option<SortBy>, asc: bool, desc: bool) -> taurine_core::error::Result<()> {
    use taurine_core::db::crud::get_automations_list;

    let conn = init::setup()?;
    let mut automations = get_automations_list(&conn)?;

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
    automations.sort_by(|a, b| {
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

    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .apply_modifier(modifiers::UTF8_ROUND_CORNERS);

    table.set_style(TableComponent::HeaderLines, '─');
    table.set_style(TableComponent::LeftHeaderIntersection, '├');
    table.set_style(TableComponent::MiddleHeaderIntersections, '┼');
    table.set_style(TableComponent::RightHeaderIntersection, '┤');
    table.set_style(TableComponent::VerticalLines, '│');

    // Build headers based on sort option
    let mut headers = vec!["TRIGGER", "OUTPUT"];
    match sort {
        Some(SortBy::Usage) => headers.push("USAGE"),
        Some(SortBy::Created) => headers.push("CREATED AT"),
        _ => {}
    }
    table.set_header(headers);

    for auto in automations {
        let mut row = vec![auto.trigger, auto.output];
        match sort {
            Some(SortBy::Usage) => {
                row.push(auto.usage_count.to_string());
            }
            Some(SortBy::Created) => {
                // Formatting timestamp as a human-readable string using the time crate
                let dt = OffsetDateTime::from_unix_timestamp(auto.created_at)
                    .unwrap_or(OffsetDateTime::UNIX_EPOCH);
                // "2024-04-12" or similar
                let date_str = format!("{}-{:02}-{:02}", dt.year(), u8::from(dt.month()), dt.day());
                row.push(date_str);
            }
            _ => {}
        }
        table.add_row(row);
    }

    println!("{table}");
    Ok(())
}
