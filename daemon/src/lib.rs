// Licensed under the Aimer Software License (ASL)
// See LICENSE for details.

use taurine_core::db::init;
use tracing::{error, info};

pub fn start() -> Result<(), Box<dyn std::error::Error>> {
    let _conn = init::setup().map_err(|e| {
        error!("Fatal database error during daemon boot: {}", e);
        e
    })?;

    info!("Daemon initialization complete!");

    Ok(())
}
