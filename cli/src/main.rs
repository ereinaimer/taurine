// Licensed under the Aimer Software License (ASL). See LICENSE for details.

use taurine_core::db;

fn main() {
    println!("Initializing Taurine database...");
    
    match db::init_db() {
        Ok(_) => {
            let path = taurine_core::paths::get_db_path();
            println!("Database initialized successfully at: {}", path.display());
        }
        Err(e) => {
            eprintln!("Failed to initialize database: {}", e);
            std::process::exit(1);
        }
    }
}
