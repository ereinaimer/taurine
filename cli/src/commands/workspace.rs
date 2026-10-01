use taurine_core::db::crud::{create_workspace, delete_workspace, get_workspaces};
use taurine_core::db::init;
use taurine_core::error::Result;

/// Lists all active workspaces.
pub fn execute_list(json: bool) -> Result<()> {
    let conn = init::setup()?;
    let workspaces = get_workspaces(&conn)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&workspaces)?);
        return Ok(());
    }

    println!("\nWorkspaces:");
    println!("{:<38} {:<20} {:<10}", "ID", "Name", "Default");
    println!("{}", "-".repeat(70));
    for ws in &workspaces {
        let default_mark = if ws.is_default { "yes" } else { "no" };
        println!("{:<38} {:<20} {:<10}", ws.id, ws.name, default_mark);
    }
    println!();
    Ok(())
}

/// Creates a new workspace.
pub fn execute_add(name: &str, json: bool) -> Result<()> {
    let conn = init::setup()?;
    let ws = create_workspace(&conn, name)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&ws)?);
    } else {
        println!("Created workspace '{}' ({})", ws.name, ws.id);
    }
    Ok(())
}

/// Deletes an existing workspace by name or ID.
pub fn execute_delete(id_or_name: &str, json: bool) -> Result<()> {
    let conn = init::setup()?;
    delete_workspace(&conn, id_or_name)?;

    if json {
        println!(
            "{}",
            serde_json::json!({ "status": "deleted", "target": id_or_name })
        );
    } else {
        println!("Deleted workspace '{id_or_name}'.");
    }
    Ok(())
}
