use comfy_table::{Table, presets::NOTHING};
use taurine_core::db::init;

pub fn execute() -> Result<(), Box<dyn std::error::Error>> {
    use taurine_core::db::crud::get_all_active_automations;

    let conn = init::setup()?;
    let automations = get_all_active_automations(&conn)?;

    let mut table = Table::new();
    table.load_preset(NOTHING);
    table.set_header(vec!["TRIGGER", "OUTPUT"]);
    for (trigger, action) in automations {
        table.add_row(vec![trigger, action.output]);
    }

    println!("{}", table);
    Ok(())
}
