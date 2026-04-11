use comfy_table::{Table, TableComponent, modifiers, presets};
use taurine_core::db::init;

pub fn execute() -> taurine_core::error::Result<()> {
    use taurine_core::db::crud::get_all_active_automations;

    let conn = init::setup()?;
    let automations = get_all_active_automations(&conn)?;

    let mut table = Table::new();

    table
        .load_preset(presets::UTF8_FULL_CONDENSED)
        .apply_modifier(modifiers::UTF8_ROUND_CORNERS);

    table.set_style(TableComponent::HeaderLines, '─');
    table.set_style(TableComponent::LeftHeaderIntersection, '├');
    table.set_style(TableComponent::MiddleHeaderIntersections, '┼');
    table.set_style(TableComponent::RightHeaderIntersection, '┤');
    table.set_style(TableComponent::VerticalLines, '│');

    table.set_header(vec!["TRIGGER", "OUTPUT"]);
    for (trigger, action) in automations {
        table.add_row(vec![trigger, action.output]);
    }

    println!("{table}");
    Ok(())
}
