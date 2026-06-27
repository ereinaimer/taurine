#![windows_subsystem = "windows"]

use std::env;
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x08000000;

fn main() {
    let exe_path = env::current_exe().expect("Failed to get current executable path");
    let path_file = exe_path.with_extension("path");
    
    let target_exe = match std::fs::read_to_string(&path_file) {
        Ok(path) => path.trim().to_string(),
        Err(_) => {
            let args: Vec<String> = env::args().collect();
            if args.len() >= 2 {
                args[1].clone()
            } else {
                return;
            }
        }
    };
    
    let _ = Command::new(&target_exe)
        .arg("--daemon")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}
